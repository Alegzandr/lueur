//! Records on the wire, their AES-128-GCM protection, and the key
//! derivation (the TLS 1.2 PRF over a PSK premaster secret).

use super::{EXPLICIT_NONCE, MAX_SEQ, RECORD_HEADER, TAG, VERSION};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hmac;
use std::io;

/// One record, as read off the wire.
pub(super) struct Record<'a> {
    pub(super) ctype: u8,
    pub(super) epoch: u16,
    pub(super) seq: u64,
    pub(super) body: &'a [u8],
}

/// The records in a datagram, up to the first one that does not parse.
pub(super) fn records(mut datagram: &[u8]) -> Vec<Record<'_>> {
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
pub(super) fn plain_record(ctype: u8, seq: u64, body: &[u8]) -> Vec<u8> {
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
pub(super) struct Cipher {
    key: LessSafeKey,
    iv: [u8; 4],
}

impl Cipher {
    pub(super) fn new(key: &[u8], iv: &[u8]) -> io::Result<Self> {
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
    pub(super) fn seal(
        &self,
        epoch: u16,
        seq: u64,
        ctype: u8,
        plain: &[u8],
    ) -> io::Result<Vec<u8>> {
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
    pub(super) fn open(&self, rec: &Record) -> Option<Vec<u8>> {
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
pub(super) fn prf(secret: &[u8], label: &[u8], seed: &[u8], out: &mut [u8]) {
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
pub(super) fn premaster(psk: &[u8]) -> Vec<u8> {
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
pub(super) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
