//! DTLS 1.2 with a pre-shared key, client side only: the transport of the Hue
//! Entertainment API, which streams colours to a bridge over UDP.
//!
//! Exactly one cipher suite is spoken, the one a bridge asks for:
//! TLS_PSK_WITH_AES_128_GCM_SHA256. No certificates, no extensions, no
//! renegotiation and no resumption: a stream handshakes once, sends records
//! until it stops, and starts over if anything goes wrong. rustls has no
//! DTLS, and a second TLS stack would bring far more than this needs.
//!
//! The handshake (RFC 6347 over RFC 5246, PSK key exchange from RFC 4279):
//!
//! ```text
//! ClientHello                 -->
//!                             <--  HelloVerifyRequest (cookie)
//! ClientHello (cookie)        -->
//!                             <--  ServerHello, [ServerKeyExchange],
//!                                  ServerHelloDone
//! ClientKeyExchange,
//! ChangeCipherSpec, Finished  -->
//!                             <--  ChangeCipherSpec, Finished
//! ```
//!
//! UDP may lose, duplicate or reorder any of it, so a flight is sent again
//! until the answer to it arrives, and handshake messages are reassembled
//! from whatever fragments come in, in any order.

use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::digest::{self, SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// DTLS 1.2 on the wire (the ones' complement of 1.2).
const VERSION: [u8; 2] = [0xFE, 0xFD];
/// TLS_PSK_WITH_AES_128_GCM_SHA256 (RFC 5487).
const SUITE: [u8; 2] = [0x00, 0xA8];

const CHANGE_CIPHER_SPEC: u8 = 20;
const ALERT: u8 = 21;
const HANDSHAKE: u8 = 22;
const APPLICATION_DATA: u8 = 23;

const CLIENT_HELLO: u8 = 1;
const SERVER_HELLO: u8 = 2;
const HELLO_VERIFY_REQUEST: u8 = 3;
const SERVER_KEY_EXCHANGE: u8 = 12;
const SERVER_HELLO_DONE: u8 = 14;
const CLIENT_KEY_EXCHANGE: u8 = 16;
const FINISHED: u8 = 20;

const RECORD_HEADER: usize = 13;
const HANDSHAKE_HEADER: usize = 12;
/// The per-record part of the GCM nonce, sent in front of the ciphertext.
const EXPLICIT_NONCE: usize = 8;
const TAG: usize = 16;
const VERIFY_DATA: usize = 12;

/// The largest plaintext a record may carry (RFC 5246 §6.2.1).
pub const MAX_PLAINTEXT: usize = 1 << 14;
/// Room for the largest record a peer may send.
const DATAGRAM: usize = MAX_PLAINTEXT + 2048;
/// Handshake messages larger than this are refused. A PSK server's are tens
/// of bytes; the limit only bounds what a hostile one can make us hold.
const MAX_MESSAGE: usize = 4096;
/// How many handshake messages are held while waiting for an earlier one.
const AHEAD: u16 = 8;
/// The last record sequence number (48 bits) before the epoch would wrap.
const MAX_SEQ: u64 = (1 << 48) - 1;

/// How long a flight waits for its answer before it is sent again. RFC 6347
/// suggests a second; on a LAN an answer takes milliseconds, so a lost
/// datagram should not cost one.
const FIRST_WAIT: Duration = Duration::from_millis(400);
/// The wait doubles after each resend, up to this.
const MAX_WAIT: Duration = Duration::from_secs(4);

/// A DTLS alert from the peer, carried inside the [`io::Error`] a call fails
/// with (see [`alert`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alert(pub u8);

impl Alert {
    /// The alert's name in the TLS registry, or "unknown".
    pub fn name(self) -> &'static str {
        match self.0 {
            0 => "close_notify",
            10 => "unexpected_message",
            20 => "bad_record_mac",
            22 => "record_overflow",
            40 => "handshake_failure",
            47 => "illegal_parameter",
            50 => "decode_error",
            51 => "decrypt_error",
            70 => "protocol_version",
            71 => "insufficient_security",
            80 => "internal_error",
            90 => "user_canceled",
            110 => "unsupported_extension",
            115 => "unknown_psk_identity",
            _ => "unknown",
        }
    }
}

impl fmt::Display for Alert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the peer sent a {} alert ({})", self.name(), self.0)
    }
}

impl std::error::Error for Alert {}

/// The alert `e` reports, if it is one.
pub fn alert(e: &io::Error) -> Option<Alert> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<Alert>())
        .copied()
}

/// A DTLS 1.2 session to one server, past its handshake.
pub struct DtlsClient {
    socket: UdpSocket,
    write: Cipher,
    read: Cipher,
    /// The next epoch-1 record sequence number to send.
    seq: u64,
    timeout: Duration,
    closed: bool,
}

impl fmt::Debug for DtlsClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DtlsClient")
            .field("peer", &self.socket.peer_addr().ok())
            .field("seq", &self.seq)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl DtlsClient {
    /// Handshake with `addr`, as `identity`, with the pre-shared `psk` (raw
    /// bytes). The whole handshake must finish within `timeout`, which is
    /// also how long [`recv`](Self::recv) waits afterwards.
    pub fn connect(
        addr: SocketAddr,
        identity: &[u8],
        psk: &[u8],
        timeout: Duration,
    ) -> io::Result<Self> {
        if identity.is_empty() || identity.len() > usize::from(u16::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a PSK identity is 1 to 65535 bytes",
            ));
        }
        if psk.is_empty() || psk.len() > usize::from(u16::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a pre-shared key is 1 to 65535 bytes",
            ));
        }
        let local: SocketAddr = match addr {
            SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
            SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let socket = UdpSocket::bind(local)?;
        // Connected, so datagrams from anyone else never reach us.
        socket.connect(addr)?;

        let mut hs = Handshake::new(&socket, Instant::now() + timeout)?;
        let (write, read) = hs.run(identity, psk)?;
        let seq = hs.write_seq[1];
        Ok(Self {
            socket,
            write,
            read,
            seq,
            timeout,
            closed: false,
        })
    }

    /// Send one application_data record carrying `payload`.
    pub fn send(&mut self, payload: &[u8]) -> io::Result<()> {
        self.send_record(APPLICATION_DATA, payload)
    }

    fn send_record(&mut self, ctype: u8, payload: &[u8]) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the DTLS session is closed",
            ));
        }
        if payload.len() > MAX_PLAINTEXT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a DTLS record carries at most 16 KiB",
            ));
        }
        // A sequence number must never repeat under one key: it is the nonce.
        if self.seq > MAX_SEQ {
            return Err(io::Error::other(
                "the DTLS session ran out of sequence numbers",
            ));
        }
        let record = self.write.seal(1, self.seq, ctype, payload)?;
        self.seq += 1;
        self.socket.send(&record)?;
        Ok(())
    }

    /// Read one application_data record, waiting up to the socket timeout
    /// (used by tests; Hue never answers). A longer record is cut to `buf`.
    pub fn recv(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + self.timeout;
        let mut datagram = vec![0u8; DATAGRAM];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "no DTLS record arrived",
                ));
            }
            self.socket.set_read_timeout(Some(left))?;
            let n = match self.socket.recv(&mut datagram) {
                Ok(n) => n,
                Err(e) if is_timeout(&e) => continue,
                Err(e) => return Err(e),
            };
            for rec in records(&datagram[..n]) {
                match (rec.epoch, rec.ctype) {
                    (0, ALERT) => check_alert(rec.body)?,
                    (1, _) => {
                        // Forged or damaged records are dropped, as RFC 6347
                        // asks: answering them would let anyone end the session.
                        let Some(plain) = self.read.open(&rec) else {
                            continue;
                        };
                        match rec.ctype {
                            ALERT => check_alert(&plain)?,
                            APPLICATION_DATA => {
                                let len = plain.len().min(buf.len());
                                buf[..len].copy_from_slice(&plain[..len]);
                                return Ok(len);
                            }
                            // A resent Finished: the server lost nothing we
                            // can still help with.
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Send close_notify; also done on Drop (best effort, ignore errors).
    pub fn close(&mut self) {
        if !self.closed {
            // A warning-level close_notify.
            let _ = self.send_record(ALERT, &[1, 0]);
            self.closed = true;
        }
    }
}

impl Drop for DtlsClient {
    fn drop(&mut self) {
        self.close();
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Fail on a fatal alert or close_notify; warnings are let through.
fn check_alert(body: &[u8]) -> io::Result<()> {
    match body {
        [level, description] if *level == 2 || *description == 0 => Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            Alert(*description),
        )),
        _ => Ok(()),
    }
}

fn malformed(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("malformed DTLS {what}"))
}

fn unexpected(msg_type: u8) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("unexpected DTLS handshake message (type {msg_type})"),
    )
}

/// One record, as read off the wire.
struct Record<'a> {
    ctype: u8,
    epoch: u16,
    seq: u64,
    body: &'a [u8],
}

/// The records in a datagram, up to the first one that does not parse.
fn records(mut datagram: &[u8]) -> Vec<Record<'_>> {
    let mut out = Vec::new();
    while datagram.len() >= RECORD_HEADER {
        let len = usize::from(u16::from_be_bytes([datagram[11], datagram[12]]));
        // Any DTLS version: a HelloVerifyRequest may come as DTLS 1.0.
        if datagram[1] != 0xFE || datagram.len() < RECORD_HEADER + len {
            break;
        }
        let mut epoch_seq = [0u8; 8];
        epoch_seq.copy_from_slice(&datagram[3..11]);
        let epoch_seq = u64::from_be_bytes(epoch_seq);
        out.push(Record {
            ctype: datagram[0],
            epoch: (epoch_seq >> 48) as u16,
            seq: epoch_seq & MAX_SEQ,
            body: &datagram[RECORD_HEADER..RECORD_HEADER + len],
        });
        datagram = &datagram[RECORD_HEADER + len..];
    }
    out
}

/// A plaintext record (epoch 0).
fn plain_record(ctype: u8, seq: u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECORD_HEADER + body.len());
    out.push(ctype);
    out.extend_from_slice(&VERSION);
    // Epoch 0 in the top two bytes: seq is at most 48 bits.
    out.extend_from_slice(&seq.to_be_bytes());
    // Bodies are our own handshake messages, far below 64 KiB.
    out.extend_from_slice(&(body.len() as u16).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// One direction's AES-128-GCM state.
struct Cipher {
    key: LessSafeKey,
    iv: [u8; 4],
}

impl Cipher {
    fn new(key: &[u8], iv: &[u8]) -> io::Result<Self> {
        let key = UnboundKey::new(&AES_128_GCM, key)
            .map_err(|_| io::Error::other("bad AES-128-GCM key length"))?;
        let mut salt = [0u8; 4];
        salt.copy_from_slice(iv);
        Ok(Self {
            key: LessSafeKey::new(key),
            iv: salt,
        })
    }

    fn nonce(&self, explicit: &[u8]) -> Nonce {
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.iv);
        nonce[4..].copy_from_slice(explicit);
        Nonce::assume_unique_for_key(nonce)
    }

    /// The whole record for `plain`. The explicit nonce is the record's own
    /// epoch and sequence number, which never repeat under one key.
    fn seal(&self, epoch: u16, seq: u64, ctype: u8, plain: &[u8]) -> io::Result<Vec<u8>> {
        let epoch_seq = (u64::from(epoch) << 48 | seq).to_be_bytes();
        let len = EXPLICIT_NONCE + plain.len() + TAG;
        let mut out = Vec::with_capacity(RECORD_HEADER + len);
        out.push(ctype);
        out.extend_from_slice(&VERSION);
        out.extend_from_slice(&epoch_seq);
        out.extend_from_slice(&(len as u16).to_be_bytes());
        out.extend_from_slice(&epoch_seq);
        let start = out.len();
        out.extend_from_slice(plain);
        let tag = self
            .key
            .seal_in_place_separate_tag(
                self.nonce(&epoch_seq),
                Aad::from(aad(epoch_seq, ctype, plain.len())),
                &mut out[start..],
            )
            .map_err(|_| io::Error::other("DTLS record encryption failed"))?;
        out.extend_from_slice(tag.as_ref());
        Ok(out)
    }

    /// The plaintext of `rec`, or `None` if it does not authenticate.
    fn open(&self, rec: &Record) -> Option<Vec<u8>> {
        let body = rec.body;
        if body.len() < EXPLICIT_NONCE + TAG {
            return None;
        }
        let epoch_seq = (u64::from(rec.epoch) << 48 | rec.seq).to_be_bytes();
        let plain_len = body.len() - EXPLICIT_NONCE - TAG;
        let mut data = body[EXPLICIT_NONCE..].to_vec();
        let plain = self
            .key
            .open_in_place(
                self.nonce(&body[..EXPLICIT_NONCE]),
                Aad::from(aad(epoch_seq, rec.ctype, plain_len)),
                &mut data,
            )
            .ok()?;
        let len = plain.len();
        data.truncate(len);
        Some(data)
    }
}

/// Additional data: epoch and sequence number, type, version, plaintext length.
fn aad(epoch_seq: [u8; 8], ctype: u8, plain_len: usize) -> [u8; 13] {
    let mut aad = [0u8; 13];
    aad[..8].copy_from_slice(&epoch_seq);
    aad[8] = ctype;
    aad[9..11].copy_from_slice(&VERSION);
    aad[11..].copy_from_slice(&(plain_len as u16).to_be_bytes());
    aad
}

/// The TLS 1.2 PRF with HMAC-SHA256 (RFC 5246 §5).
fn prf(secret: &[u8], label: &[u8], seed: &[u8], out: &mut [u8]) {
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let mut ctx = hmac::Context::with_key(&key);
    ctx.update(label);
    ctx.update(seed);
    let mut a = ctx.sign();
    for chunk in out.chunks_mut(32) {
        let mut ctx = hmac::Context::with_key(&key);
        ctx.update(a.as_ref());
        ctx.update(label);
        ctx.update(seed);
        let block = ctx.sign();
        chunk.copy_from_slice(&block.as_ref()[..chunk.len()]);
        a = hmac::sign(&key, a.as_ref());
    }
}

/// The PSK premaster secret (RFC 4279 §2): as many zeros as the key is long,
/// then the key, each with a length.
fn premaster(psk: &[u8]) -> Vec<u8> {
    let len = (psk.len() as u16).to_be_bytes();
    let mut out = Vec::with_capacity(4 + 2 * psk.len());
    out.extend_from_slice(&len);
    out.resize(2 + psk.len(), 0);
    out.extend_from_slice(&len);
    out.extend_from_slice(psk);
    out
}

/// Whether two byte strings match, in time that does not depend on where
/// they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A ClientHello body: our one suite, null compression, no extensions.
fn client_hello(random: &[u8; 32], cookie: &[u8]) -> Vec<u8> {
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
fn server_hello_random(body: &[u8]) -> io::Result<[u8; 32]> {
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
struct Message {
    msg_type: u8,
    seq: u16,
    /// The record epoch it came in: a Finished must be encrypted.
    epoch: u16,
    body: Vec<u8>,
}

impl Message {
    /// The message as the transcript hashes it: one unfragmented whole.
    fn hash_into(&self, transcript: &mut digest::Context) {
        transcript.update(&handshake_header(self.msg_type, self.seq, self.body.len()));
        transcript.update(&self.body);
    }
}

/// A handshake header for an unfragmented message.
fn handshake_header(msg_type: u8, seq: u16, len: usize) -> [u8; HANDSHAKE_HEADER] {
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
struct Reassembler {
    /// The next message_seq to hand out; `None` takes whichever is first
    /// complete, as a server may number its reply to a cookie either way.
    next: Option<u16>,
    partials: BTreeMap<u16, Partial>,
}

impl Reassembler {
    /// Take in every fragment of one record's body.
    fn feed(&mut self, epoch: u16, mut body: &[u8]) {
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
    fn pop(&mut self) -> Option<Message> {
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
struct Handshake<'a> {
    socket: &'a UdpSocket,
    deadline: Instant,
    client_random: [u8; 32],
    /// The next record sequence number, per epoch.
    write_seq: [u64; 2],
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
    fn new(socket: &'a UdpSocket, deadline: Instant) -> io::Result<Self> {
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
    fn run(&mut self, identity: &[u8], psk: &[u8]) -> io::Result<(Cipher, Cipher)> {
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

#[cfg(test)]
mod tests {
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
}
