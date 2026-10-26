//! The client side of the handshake: its messages, their reassembly from
//! fragments, and the flights sent until they are answered.

use super::record::{Cipher, plain_record, premaster, prf, records, same};
use super::{
    AHEAD, ALERT, CHANGE_CIPHER_SPEC, CLIENT_HELLO, CLIENT_KEY_EXCHANGE, DATAGRAM, FINISHED,
    FIRST_WAIT, HANDSHAKE, HANDSHAKE_HEADER, HELLO_VERIFY_REQUEST, MAX_MESSAGE, MAX_SEQ, MAX_WAIT,
    SERVER_HELLO, SERVER_HELLO_DONE, SERVER_KEY_EXCHANGE, SUITE, VERIFY_DATA, VERSION, check_alert,
    is_timeout, malformed, unexpected,
};
use ring::digest::{self, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::BTreeMap;
use std::io;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

/// A ClientHello body: our one suite, null compression, no extensions.
pub(super) fn client_hello(random: &[u8; 32], cookie: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(48 + cookie.len());
    out.extend_from_slice(&VERSION);
    out.extend_from_slice(random);
    out.push(0); // no session to resume
    // Cookies are at most 255 bytes: the HelloVerifyRequest parser saw to it.
    out.push(cookie.len() as u8);
    out.extend_from_slice(cookie);
    out.extend_from_slice(&2u16.to_be_bytes());
    out.extend_from_slice(&SUITE);
    out.extend_from_slice(&[1, 0]); // null compression only
    out
}

/// The cookie in a HelloVerifyRequest body.
fn hello_verify_cookie(body: &[u8]) -> io::Result<&[u8]> {
    match body {
        [0xFE, _, len, cookie @ ..] if cookie.len() == usize::from(*len) => Ok(cookie),
        _ => Err(malformed("HelloVerifyRequest")),
    }
}

/// The server random in a ServerHello body, once it is checked to have
/// picked DTLS 1.2 and our suite.
pub(super) fn server_hello_random(body: &[u8]) -> io::Result<[u8; 32]> {
    let bad = || malformed("ServerHello");
    if body.len() < 35 {
        return Err(bad());
    }
    if body[..2] != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the server would not speak DTLS 1.2",
        ));
    }
    let mut random = [0u8; 32];
    random.copy_from_slice(&body[2..34]);
    let sid = usize::from(body[34]);
    let rest = body.get(35 + sid..).ok_or_else(bad)?;
    // Any extensions after these are ignored: none were offered, so none can
    // change what follows.
    match rest {
        [s0, s1, 0, ..] if [*s0, *s1] == SUITE => Ok(random),
        [_, _, _, ..] => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the server chose a cipher suite or compression that was not offered",
        )),
        _ => Err(bad()),
    }
}

/// A ClientKeyExchange body: just the identity.
fn client_key_exchange(identity: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + identity.len());
    out.extend_from_slice(&(identity.len() as u16).to_be_bytes());
    out.extend_from_slice(identity);
    out
}

/// A handshake message whose last bytes have arrived.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Message {
    pub(super) msg_type: u8,
    pub(super) seq: u16,
    /// The record epoch it came in: a Finished must be encrypted.
    pub(super) epoch: u16,
    pub(super) body: Vec<u8>,
}

impl Message {
    /// The message as the transcript hashes it: one unfragmented whole.
    fn hash_into(&self, transcript: &mut digest::Context) {
        transcript.update(&handshake_header(self.msg_type, self.seq, self.body.len()));
        transcript.update(&self.body);
    }
}

/// A handshake header for an unfragmented message.
pub(super) fn handshake_header(msg_type: u8, seq: u16, len: usize) -> [u8; HANDSHAKE_HEADER] {
    let len = (len as u32).to_be_bytes();
    let mut h = [0u8; HANDSHAKE_HEADER];
    h[0] = msg_type;
    h[1..4].copy_from_slice(&len[1..]);
    h[4..6].copy_from_slice(&seq.to_be_bytes());
    // fragment_offset stays 0
    h[9..12].copy_from_slice(&len[1..]);
    h
}

/// A handshake message being put back together from its fragments.
struct Partial {
    msg_type: u8,
    epoch: u16,
    body: Vec<u8>,
    have: Vec<bool>,
    missing: usize,
}

/// Handshake messages in order, from fragments that come in any order and
/// any number of times.
#[derive(Default)]
pub(super) struct Reassembler {
    /// The next message_seq to hand out; `None` takes whichever is first
    /// complete, as a server may number its reply to a cookie either way.
    next: Option<u16>,
    partials: BTreeMap<u16, Partial>,
}

impl Reassembler {
    /// Take in every fragment of one record's body.
    pub(super) fn feed(&mut self, epoch: u16, mut body: &[u8]) {
        while body.len() >= HANDSHAKE_HEADER {
            let u24 =
                |b: &[u8]| usize::from(b[0]) << 16 | usize::from(b[1]) << 8 | usize::from(b[2]);
            let msg_type = body[0];
            let len = u24(&body[1..4]);
            let seq = u16::from_be_bytes([body[4], body[5]]);
            let offset = u24(&body[6..9]);
            let frag_len = u24(&body[9..12]);
            let Some(fragment) = body.get(HANDSHAKE_HEADER..HANDSHAKE_HEADER + frag_len) else {
                return;
            };
            body = &body[HANDSHAKE_HEADER + frag_len..];
            if len > MAX_MESSAGE || offset + frag_len > len {
                continue;
            }
            self.insert(msg_type, seq, epoch, len, offset, fragment);
        }
    }

    fn insert(
        &mut self,
        msg_type: u8,
        seq: u16,
        epoch: u16,
        len: usize,
        offset: usize,
        fragment: &[u8],
    ) {
        // Old messages are resends of what was already handled; far-future
        // ones would only let a peer make us hold memory.
        let wanted = match self.next {
            Some(next) => seq >= next && seq - next < AHEAD,
            None => self.partials.len() < usize::from(AHEAD) || self.partials.contains_key(&seq),
        };
        if !wanted {
            return;
        }
        let partial = self.partials.entry(seq).or_insert_with(|| Partial {
            msg_type,
            epoch,
            body: vec![0; len],
            have: vec![false; len],
            missing: len,
        });
        // A fragment that disagrees with the first one seen is someone
        // else's message; drop it rather than mix the two.
        if partial.msg_type != msg_type || partial.epoch != epoch || partial.body.len() != len {
            return;
        }
        for (i, &byte) in fragment.iter().enumerate() {
            let at = offset + i;
            if !partial.have[at] {
                partial.have[at] = true;
                partial.body[at] = byte;
                partial.missing -= 1;
            }
        }
    }

    /// The next whole message, if it has arrived.
    pub(super) fn pop(&mut self) -> Option<Message> {
        let seq = match self.next {
            Some(next) => next,
            // The lowest held: a later message must not overtake it.
            None => *self.partials.keys().next()?,
        };
        if self.partials.get(&seq)?.missing != 0 {
            return None;
        }
        let p = self.partials.remove(&seq)?;
        let next = seq.wrapping_add(1);
        self.next = Some(next);
        self.partials.retain(|&s, _| s >= next);
        Some(Message {
            msg_type: p.msg_type,
            seq,
            epoch: p.epoch,
            body: p.body,
        })
    }
}

/// A record of the flight being (re)sent: built afresh each time, so a
/// resend gets new sequence numbers.
struct Out {
    ctype: u8,
    epoch: u16,
    body: Vec<u8>,
}

/// The client side of one handshake.
pub(super) struct Handshake<'a> {
    socket: &'a UdpSocket,
    deadline: Instant,
    client_random: [u8; 32],
    /// The next record sequence number, per epoch.
    pub(super) write_seq: [u64; 2],
    /// The next handshake message_seq we send.
    message_seq: u16,
    incoming: Reassembler,
    /// SHA-256 over every message since the ClientHello that got through
    /// (RFC 6347 §4.2.6: not the first one, nor the HelloVerifyRequest).
    transcript: digest::Context,
    write: Option<Cipher>,
    read: Option<Cipher>,
    flight: Vec<Out>,
    wait: Duration,
    resend_at: Instant,
}

impl<'a> Handshake<'a> {
    pub(super) fn new(socket: &'a UdpSocket, deadline: Instant) -> io::Result<Self> {
        let mut client_random = [0u8; 32];
        SystemRandom::new()
            .fill(&mut client_random)
            .map_err(|_| io::Error::other("no system randomness"))?;
        Ok(Self {
            socket,
            deadline,
            client_random,
            write_seq: [0; 2],
            message_seq: 0,
            incoming: Reassembler::default(),
            transcript: digest::Context::new(&SHA256),
            write: None,
            read: None,
            flight: Vec::new(),
            wait: FIRST_WAIT,
            resend_at: Instant::now(),
        })
    }

    /// The handshake, ending in the client's and server's record ciphers.
    pub(super) fn run(&mut self, identity: &[u8], psk: &[u8]) -> io::Result<(Cipher, Cipher)> {
        let hello = self.message(CLIENT_HELLO, &client_hello(&self.client_random, &[]));
        self.send_flight(vec![Out {
            ctype: HANDSHAKE,
            epoch: 0,
            body: hello,
        }])?;

        let server_random = loop {
            let msg = self.next_message()?;
            match (msg.msg_type, msg.epoch) {
                (HELLO_VERIFY_REQUEST, 0) => {
                    // The server wants proof we can receive at our address
                    // before it does any work: say hello again, with its cookie.
                    let cookie = hello_verify_cookie(&msg.body)?.to_vec();
                    self.transcript = digest::Context::new(&SHA256);
                    self.incoming = Reassembler::default();
                    let hello =
                        self.message(CLIENT_HELLO, &client_hello(&self.client_random, &cookie));
                    self.send_flight(vec![Out {
                        ctype: HANDSHAKE,
                        epoch: 0,
                        body: hello,
                    }])?;
                }
                (SERVER_HELLO, 0) => {
                    msg.hash_into(&mut self.transcript);
                    break server_hello_random(&msg.body)?;
                }
                _ => return Err(unexpected(msg.msg_type)),
            }
        };

        loop {
            let msg = self.next_message()?;
            match (msg.msg_type, msg.epoch) {
                // Only an identity hint, which a bridge's identity never needs.
                (SERVER_KEY_EXCHANGE, 0) => msg.hash_into(&mut self.transcript),
                (SERVER_HELLO_DONE, 0) if msg.body.is_empty() => {
                    msg.hash_into(&mut self.transcript);
                    break;
                }
                _ => return Err(unexpected(msg.msg_type)),
            }
        }

        let randoms = [self.client_random, server_random].concat();
        let mut master = [0u8; 48];
        prf(&premaster(psk), b"master secret", &randoms, &mut master);
        let mut block = [0u8; 40];
        let swapped = [server_random, self.client_random].concat();
        prf(&master, b"key expansion", &swapped, &mut block);
        let write = Cipher::new(&block[..16], &block[32..36])?;
        let read = Cipher::new(&block[16..32], &block[36..40])?;

        let key_exchange = self.message(CLIENT_KEY_EXCHANGE, &client_key_exchange(identity));
        let mut verify = [0u8; VERIFY_DATA];
        prf(
            &master,
            b"client finished",
            self.transcript.clone().finish().as_ref(),
            &mut verify,
        );
        let finished = self.message(FINISHED, &verify);
        let mut expected = [0u8; VERIFY_DATA];
        prf(
            &master,
            b"server finished",
            self.transcript.clone().finish().as_ref(),
            &mut expected,
        );

        self.write = Some(write);
        self.read = Some(read);
        self.send_flight(vec![
            Out {
                ctype: HANDSHAKE,
                epoch: 0,
                body: key_exchange,
            },
            Out {
                ctype: CHANGE_CIPHER_SPEC,
                epoch: 0,
                body: vec![1],
            },
            Out {
                ctype: HANDSHAKE,
                epoch: 1,
                body: finished,
            },
        ])?;

        let msg = self.next_message()?;
        if msg.msg_type != FINISHED || msg.epoch != 1 {
            return Err(unexpected(msg.msg_type));
        }
        if !same(&msg.body, &expected) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the server's Finished does not match: it holds another key",
            ));
        }
        match (self.write.take(), self.read.take()) {
            (Some(write), Some(read)) => Ok((write, read)),
            _ => Err(io::Error::other("DTLS keys went missing")),
        }
    }

    /// Our next handshake message, whole, added to the transcript.
    fn message(&mut self, msg_type: u8, body: &[u8]) -> Vec<u8> {
        let mut out = handshake_header(msg_type, self.message_seq, body.len()).to_vec();
        out.extend_from_slice(body);
        self.transcript.update(&out);
        self.message_seq += 1;
        out
    }

    fn send_flight(&mut self, flight: Vec<Out>) -> io::Result<()> {
        self.flight = flight;
        self.wait = FIRST_WAIT;
        self.resend_at = Instant::now() + self.wait;
        self.transmit()
    }

    /// Send the current flight, in one datagram (it is a few hundred bytes).
    fn transmit(&mut self) -> io::Result<()> {
        let mut datagram = Vec::new();
        for out in &self.flight {
            let seq = self.write_seq[usize::from(out.epoch)];
            if seq > MAX_SEQ {
                return Err(io::Error::other(
                    "the DTLS handshake ran out of sequence numbers",
                ));
            }
            self.write_seq[usize::from(out.epoch)] += 1;
            match (&self.write, out.epoch) {
                (_, 0) => datagram.extend(plain_record(out.ctype, seq, &out.body)),
                (Some(cipher), _) => {
                    datagram.extend(cipher.seal(out.epoch, seq, out.ctype, &out.body)?)
                }
                (None, _) => return Err(io::Error::other("no DTLS key for epoch 1")),
            }
        }
        self.socket.send(&datagram)?;
        Ok(())
    }

    /// The server's next handshake message, resending our flight while it is
    /// slow to come.
    fn next_message(&mut self) -> io::Result<Message> {
        let mut datagram = vec![0u8; DATAGRAM];
        loop {
            if let Some(msg) = self.incoming.pop() {
                return Ok(msg);
            }
            let now = Instant::now();
            if now >= self.deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the DTLS handshake did not finish in time",
                ));
            }
            if now >= self.resend_at {
                self.wait = (self.wait * 2).min(MAX_WAIT);
                self.resend_at = now + self.wait;
                self.transmit()?;
                continue;
            }
            let until = self.resend_at.min(self.deadline);
            let left = until
                .saturating_duration_since(now)
                .max(Duration::from_millis(1));
            self.socket.set_read_timeout(Some(left))?;
            let n = match self.socket.recv(&mut datagram) {
                Ok(n) => n,
                // On Windows an ICMP "port unreachable" (no server listening
                // yet) surfaces as a reset on the next read; the resend timer
                // keeps trying until the deadline.
                Err(e) if is_timeout(&e) || e.kind() == io::ErrorKind::ConnectionReset => continue,
                Err(e) => return Err(e),
            };
            for rec in records(&datagram[..n]) {
                match (rec.epoch, rec.ctype) {
                    (0, ALERT) => check_alert(rec.body)?,
                    (0, HANDSHAKE) => self.incoming.feed(0, rec.body),
                    (1, ALERT | HANDSHAKE) => {
                        let plain = self.read.as_ref().and_then(|read| read.open(&rec));
                        match (plain, rec.ctype) {
                            (Some(p), ALERT) => check_alert(&p)?,
                            (Some(p), _) => self.incoming.feed(1, &p),
                            (None, _) => {}
                        }
                    }
                    // The server's ChangeCipherSpec carries nothing we need:
                    // its Finished proves the switch, by authenticating.
                    _ => {}
                }
            }
        }
    }
}
