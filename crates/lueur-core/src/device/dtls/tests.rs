use super::handshake::*;
use super::record::*;
use super::*;

fn unhex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn prf_matches_the_tls12_sha256_test_vector() {
    let secret = unhex("9b be 43 6b a9 40 f0 17 b1 76 52 84 9a 71 db 35");
    let seed = unhex("a0 ba 9f 93 6c da 31 18 27 a6 f7 96 ff d5 19 8c");
    let expected = unhex(
        "e3 f2 29 ba 72 7b e1 7b 8d 12 26 20 55 7c d4 53 c2 aa b2 1d 07 c3 d4 95 32 9b 52 d4
         e6 1e db 5a 6b 30 17 91 e9 0d 35 c9 c9 a4 6b 4e 14 ba f9 af 0f a0 22 f7 07 7d ef 17
         ab fd 37 97 c0 56 4b ab 4f bc 91 66 6e 9d ef 9b 97 fc e3 4f 79 67 89 ba a4 80 82 d1
         22 ee 42 c5 a7 2e 5a 51 10 ff f7 01 87 34 7b 66",
    );
    let mut out = vec![0u8; expected.len()];
    prf(&secret, b"test label", &seed, &mut out);
    assert_eq!(out, expected);
}

#[test]
fn psk_premaster_pads_with_zeros() {
    assert_eq!(premaster(&[0xAA, 0xBB]), [0, 2, 0, 0, 0, 2, 0xAA, 0xBB]);
}

#[test]
fn records_round_trip_and_reject_tampering() {
    let key = [7u8; 16];
    let iv = [1, 2, 3, 4];
    let ours = Cipher::new(&key, &iv).unwrap();
    let theirs = Cipher::new(&key, &iv).unwrap();
    let wire = ours.seal(1, 5, APPLICATION_DATA, b"colour").unwrap();
    assert_eq!(wire.len(), RECORD_HEADER + EXPLICIT_NONCE + 6 + TAG);
    assert_eq!(&wire[3..11], &[0, 1, 0, 0, 0, 0, 0, 5]);
    // The explicit nonce is the epoch and sequence number.
    assert_eq!(&wire[13..21], &wire[3..11]);

    let recs = records(&wire);
    assert_eq!(recs.len(), 1);
    assert_eq!((recs[0].epoch, recs[0].seq), (1, 5));
    assert_eq!(theirs.open(&recs[0]).as_deref(), Some(&b"colour"[..]));

    // Same bytes claimed under another sequence number: the AAD differs.
    let mut moved = wire.clone();
    moved[10] = 6;
    assert!(theirs.open(&records(&moved)[0]).is_none());
    let mut flipped = wire;
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    assert!(theirs.open(&records(&flipped)[0]).is_none());
}

#[test]
fn client_hello_offers_one_suite_and_no_extensions() {
    let random = [9u8; 32];
    let body = client_hello(&random, &[0xC0, 0x0C]);
    assert_eq!(&body[..2], &VERSION);
    assert_eq!(&body[2..34], &random);
    assert_eq!(body[34], 0, "no session id");
    assert_eq!(&body[35..38], &[2, 0xC0, 0x0C], "the cookie");
    assert_eq!(&body[38..42], &[0, 2, 0x00, 0xA8], "one suite");
    assert_eq!(&body[42..], &[1, 0], "null compression, then nothing");

    let msg = handshake_header(CLIENT_HELLO, 1, body.len());
    assert_eq!(msg, [1, 0, 0, 44, 0, 1, 0, 0, 0, 0, 0, 44]);
}

#[test]
fn server_hello_must_pick_our_suite() {
    let mut body = VERSION.to_vec();
    body.extend([5u8; 32]);
    body.push(0);
    body.extend([0x00, 0xA8, 0]);
    assert_eq!(server_hello_random(&body).unwrap(), [5u8; 32]);
    body[35] = 0x8C;
    assert_eq!(
        server_hello_random(&body).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    assert!(server_hello_random(&body[..20]).is_err());
}

fn fragment(msg_type: u8, seq: u16, whole: &[u8], offset: usize, len: usize) -> Vec<u8> {
    let mut h = handshake_header(msg_type, seq, whole.len()).to_vec();
    h[6..9].copy_from_slice(&(offset as u32).to_be_bytes()[1..]);
    h[9..12].copy_from_slice(&(len as u32).to_be_bytes()[1..]);
    h.extend_from_slice(&whole[offset..offset + len]);
    h
}

#[test]
fn fragments_reassemble_in_any_order() {
    let mut r = Reassembler::default();
    let hello = b"server hello body".to_vec();
    // Two messages in one record, the first in two pieces out of order,
    // with an overlapping resend.
    let mut record = fragment(SERVER_HELLO, 1, &hello, 6, 11);
    record.extend(fragment(SERVER_HELLO_DONE, 2, &[], 0, 0));
    r.feed(0, &record);
    assert!(r.pop().is_none(), "the ServerHello is incomplete");
    r.feed(0, &fragment(SERVER_HELLO, 1, &hello, 0, 8));

    let first = r.pop().unwrap();
    assert_eq!(
        (first.msg_type, first.seq, first.body),
        (SERVER_HELLO, 1, hello)
    );
    let done = r.pop().unwrap();
    assert_eq!((done.msg_type, done.seq), (SERVER_HELLO_DONE, 2));
    assert!(r.pop().is_none());

    // A resend of something already handled is ignored.
    r.feed(0, &fragment(SERVER_HELLO_DONE, 2, &[], 0, 0));
    assert!(r.pop().is_none());
}

#[test]
fn fatal_alerts_become_errors() {
    let e = check_alert(&[2, 115]).unwrap_err();
    assert_eq!(alert(&e), Some(Alert(115)));
    assert!(e.to_string().contains("unknown_psk_identity"));
    assert!(
        check_alert(&[1, 0]).is_err(),
        "close_notify ends the session"
    );
    assert!(check_alert(&[1, 100]).is_ok(), "warnings pass");
}

mod openssl {
    //! Against `openssl s_server`, which must be on the PATH.

    use super::super::*;
    use std::io::{Read, Write};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};

    const PSK_HEX: &str = "0123456789abcdef0123456789abcdef";
    const TIMEOUT: Duration = Duration::from_secs(10);

    /// A DTLS server that prints what it receives and sends "pong" lines.
    struct Server {
        child: Child,
        addr: SocketAddr,
        output: Receiver<Vec<u8>>,
        seen: Vec<u8>,
    }

    impl Server {
        fn start(extra: &[&str]) -> Self {
            let port = UdpSocket::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let accept = format!("127.0.0.1:{port}");
            let mut child = Command::new("openssl")
                .args(["s_server", "-dtls1_2", "-nocert", "-psk", PSK_HEX])
                .args(["-psk_identity", "lueur", "-cipher", "PSK-AES128-GCM-SHA256"])
                .args(["-accept", &accept, "-quiet"])
                .args(extra)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("openssl on the PATH");

            // On Windows s_server reads its (piped) stdin before every
            // look at the socket, blocking until a line comes, and drops
            // the session at end of input. So it is fed a line every
            // tenth of a second, which it sends to the client.
            let mut stdin = child.stdin.take().unwrap();
            std::thread::spawn(move || {
                while stdin
                    .write_all(b"pong\n")
                    .and_then(|()| stdin.flush())
                    .is_ok()
                {
                    std::thread::sleep(Duration::from_millis(100));
                }
            });

            let mut stdout = child.stdout.take().unwrap();
            let (tx, output) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = stdout.read(&mut buf) {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            Self {
                child,
                addr: format!("127.0.0.1:{port}").parse().unwrap(),
                output,
                seen: Vec::new(),
            }
        }

        /// Whether the server printed `needle` within the timeout.
        fn printed(&mut self, needle: &[u8]) -> bool {
            let deadline = Instant::now() + TIMEOUT;
            while !self.seen.windows(needle.len()).any(|w| w == needle) {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.output.recv_timeout(left) {
                    Ok(chunk) => self.seen.extend(chunk),
                    Err(_) => return false,
                }
            }
            true
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn psk() -> Vec<u8> {
        super::unhex(PSK_HEX)
    }

    fn round_trip(extra: &[&str]) {
        let mut server = Server::start(extra);
        // The server may not be listening yet: the ClientHello is resent
        // until it is.
        let mut client = DtlsClient::connect(server.addr, b"lueur", &psk(), TIMEOUT).unwrap();
        client.send(b"hello from lueur\n").unwrap();
        assert!(
            server.printed(b"hello from lueur"),
            "server printed {:?}",
            String::from_utf8_lossy(&server.seen)
        );
        let mut buf = [0u8; 64];
        let n = client.recv(&mut buf).unwrap();
        assert!(
            buf[..n].starts_with(b"pong"),
            "got {:?}",
            String::from_utf8_lossy(&buf[..n])
        );
        client.close();
    }

    #[test]
    #[ignore = "needs openssl on the PATH"]
    fn openssl_handshake_both_ways() {
        round_trip(&[]);
    }

    #[test]
    #[ignore = "needs openssl on the PATH"]
    fn openssl_with_an_identity_hint() {
        // The hint makes the server send a ServerKeyExchange.
        round_trip(&["-psk_hint", "a hint"]);
    }

    #[test]
    #[ignore = "needs openssl on the PATH"]
    fn openssl_handshake_in_fragments() {
        // The smallest MTU OpenSSL allows, and a hint that will not fit
        // beside the ServerHello: the ServerKeyExchange comes in pieces.
        let hint = "h".repeat(120);
        round_trip(&["-mtu", "256", "-psk_hint", &hint]);
    }

    #[test]
    #[ignore = "needs openssl on the PATH"]
    fn openssl_refuses_a_wrong_key() {
        let server = Server::start(&[]);
        let wait = Duration::from_secs(3);
        let e = DtlsClient::connect(server.addr, b"lueur", &[0x55; 16], wait).unwrap_err();
        // Our Finished does not authenticate under the server's key, so it
        // is dropped unanswered, as DTLS drops any such record.
        assert_eq!(e.kind(), io::ErrorKind::TimedOut, "{e}");
    }
}
