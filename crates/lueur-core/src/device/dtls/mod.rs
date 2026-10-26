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

mod handshake;
mod record;
#[cfg(test)]
mod tests;

use handshake::Handshake;
use record::{Cipher, records};
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
