//! Just enough HTTP/1.1 to talk to a Hue bridge or a Nanoleaf controller on
//! the LAN: bounded in size and time. One-off calls get a connection each; a
//! [`Client`] keeps one open for a stream that sends a request every frame.
//!
//! A request goes in the clear, or over TLS to one certificate only
//! ([`Trust::Pinned`]). A Hue bridge's certificate is not signed by a
//! public authority, and Lueur ships no Signify root, so the certificate is
//! pinned the first time it is seen (trust on first use): from then on only
//! the holder of that certificate's key, which must sign the handshake, is
//! taken for the bridge.
//!
//! A general-purpose client would pull async, redirects and the web's
//! authorities into the core crate for devices that need none of it. Only
//! bare IP addresses are accepted, so a request can never be steered to a
//! name or a URL.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Bodies larger than this are refused: a bulb's answer is a few kilobytes.
const MAX_RESPONSE: usize = 1 << 20;

/// A whole response must arrive within this many read timeouts, so a device
/// that trickles a byte at a time cannot hold a call open.
const RESPONSE_TIMEOUTS: u32 = 2;

/// A response: status code and body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// What a host must prove before a request goes out to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Nothing: plain HTTP, read by anyone on the path.
    Clear,
    /// TLS, to the holder of the certificate with this SHA-256.
    Pinned([u8; 32]),
}

/// The error a TLS connection fails with when the host shows another
/// certificate than the pinned one. Nothing was sent to it.
#[derive(Debug)]
pub struct CertMismatch;

impl std::fmt::Display for CertMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the host showed another certificate")
    }
}

impl std::error::Error for CertMismatch {}

/// Whether `e` is a [`CertMismatch`]: someone else, or the device with a new
/// certificate.
pub fn is_cert_mismatch(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<CertMismatch>())
}

/// The SHA-256 of a DER certificate.
pub fn fingerprint(der: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, der);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

/// Send one request to `ip:port` on a fresh connection and read the whole
/// response.
pub fn request(
    method: &str,
    ip: &str,
    port: u16,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> io::Result<Response> {
    request_with(Trust::Clear, method, ip, port, path, body, timeout)
}

/// [`request`], with what the host must prove first.
pub fn request_with(
    trust: Trust,
    method: &str,
    ip: &str,
    port: u16,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> io::Result<Response> {
    let addr = address(ip, port)?;
    check_path(path)?;
    let mut conn = connect(addr, trust, timeout)?;
    exchange(&mut conn, addr, method, path, body, &[], false).map(|(response, _)| response)
}

/// A request to the holder of the `pin` certificate, with `headers` of our
/// own: a key a newer API takes there rather than in the path. Over TLS
/// only, since they carry it.
#[allow(clippy::too_many_arguments)]
pub fn request_headed(
    pin: [u8; 32],
    method: &str,
    ip: &str,
    port: u16,
    path: &str,
    body: Option<&str>,
    headers: &[(&str, &str)],
    timeout: Duration,
) -> io::Result<Response> {
    let addr = address(ip, port)?;
    check_path(path)?;
    check_headers(headers)?;
    let mut conn = connect(addr, Trust::Pinned(pin), timeout)?;
    exchange(&mut conn, addr, method, path, body, headers, false).map(|(response, _)| response)
}

/// Header names and values that cannot break out of their line.
fn check_headers(headers: &[(&str, &str)]) -> io::Result<()> {
    let clean = |s: &str| !s.is_empty() && !s.bytes().any(|b| b.is_ascii_control());
    if headers
        .iter()
        .all(|(name, value)| clean(name) && clean(value) && !name.contains([':', ' ']))
    {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "bad header"))
    }
}

/// GET `path` from `ip:port` over TLS, whatever certificate the host shows,
/// and return that certificate (DER) with the response.
///
/// This is how a certificate is learnt before it is pinned, so the request
/// must carry no credential: whoever answers reads it.
pub fn learn(
    ip: &str,
    port: u16,
    path: &str,
    timeout: Duration,
) -> io::Result<(Response, Vec<u8>)> {
    let addr = address(ip, port)?;
    check_path(path)?;
    let mut conn = connect_tls(addr, None, timeout)?;
    let cert = match &conn {
        Conn::Tls(tls) => tls
            .conn
            .peer_certificates()
            .and_then(|chain| chain.first())
            .map(|c| c.as_ref().to_vec()),
        Conn::Clear(_) => None,
    }
    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no certificate"))?;
    let (response, _) = exchange(&mut conn, addr, "GET", path, None, &[], false)?;
    Ok((response, cert))
}

/// One kept-alive connection to one host.
///
/// A stream that sends a command every frame would otherwise pay a TCP
/// handshake (and a TLS one) per command, on top of the request itself.
pub struct Client {
    addr: SocketAddr,
    trust: Trust,
    timeout: Duration,
    /// Kept between connections, so a reconnection resumes the TLS session.
    tls: Option<Arc<ClientConfig>>,
    stream: Option<Conn>,
}

impl Client {
    pub fn new(ip: &str, port: u16, timeout: Duration) -> io::Result<Self> {
        Self::with_trust(ip, port, Trust::Clear, timeout)
    }

    /// A client whose host must prove `trust` on every connection.
    pub fn with_trust(ip: &str, port: u16, trust: Trust, timeout: Duration) -> io::Result<Self> {
        let tls = match trust {
            Trust::Clear => None,
            Trust::Pinned(pin) => Some(tls_config(Some(pin))?),
        };
        Ok(Client {
            addr: address(ip, port)?,
            trust,
            timeout,
            tls,
            stream: None,
        })
    }

    /// Send one request, on the open connection when there is one.
    ///
    /// For idempotent requests only: a connection the server dropped while
    /// idle shows up as a failure on use, and the request then goes again on
    /// a fresh connection.
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> io::Result<Response> {
        check_path(path)?;
        if let Some(mut stream) = self.stream.take()
            && let Ok((response, reusable)) =
                exchange(&mut stream, self.addr, method, path, body, &[], true)
        {
            if reusable {
                self.stream = Some(stream);
            }
            return Ok(response);
        }
        let mut stream = match (&self.tls, self.trust) {
            (Some(config), Trust::Pinned(_)) => handshake(self.addr, config.clone(), self.timeout)?,
            (None, Trust::Clear) => Conn::Clear(tcp(self.addr, self.timeout)?),
            // A pinned client always has its TLS config: never the clear.
            _ => return Err(io::Error::other("TLS client without its configuration")),
        };
        let (response, reusable) = exchange(&mut stream, self.addr, method, path, body, &[], true)?;
        if reusable {
            self.stream = Some(stream);
        }
        Ok(response)
    }
}

fn address(ip: &str, port: u16) -> io::Result<SocketAddr> {
    Ok(SocketAddr::V4(SocketAddrV4::new(
        super::common::ipv4(ip)?,
        port,
    )))
}

fn check_path(path: &str) -> io::Result<()> {
    if !path.starts_with('/') || path.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad path"));
    }
    Ok(())
}

/// A connection, in the clear or over TLS.
enum Conn {
    Clear(TcpStream),
    Tls(Box<rustls::StreamOwned<ClientConnection, TcpStream>>),
}

impl Conn {
    fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Clear(s) => s,
            Conn::Tls(tls) => &tls.sock,
        }
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Clear(s) => s.read(buf),
            Conn::Tls(tls) => tls.read(buf),
        }
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Conn::Clear(s) => s.write(buf),
            Conn::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Conn::Clear(s) => s.flush(),
            Conn::Tls(tls) => tls.flush(),
        }
    }
}

fn tcp(addr: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

fn connect(addr: SocketAddr, trust: Trust, timeout: Duration) -> io::Result<Conn> {
    match trust {
        Trust::Clear => Ok(Conn::Clear(tcp(addr, timeout)?)),
        Trust::Pinned(pin) => connect_tls(addr, Some(pin), timeout),
    }
}

/// A TLS connection to `addr`, its handshake done: to the holder of the
/// `pin` certificate, or with `None` to whoever answers.
fn connect_tls(addr: SocketAddr, pin: Option<[u8; 32]>, timeout: Duration) -> io::Result<Conn> {
    handshake(addr, tls_config(pin)?, timeout)
}

fn handshake(addr: SocketAddr, config: Arc<ClientConfig>, timeout: Duration) -> io::Result<Conn> {
    let name = ServerName::from(addr.ip());
    let conn = ClientConnection::new(config, name).map_err(io::Error::other)?;
    let mut tls = rustls::StreamOwned::new(conn, tcp(addr, timeout)?);
    // Done here, before any request is written, so a host with the wrong
    // certificate never sees one; the whole handshake shares one deadline.
    let deadline = Instant::now() + timeout * RESPONSE_TIMEOUTS;
    while tls.conn.is_handshaking() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "handshake too slow",
            ));
        }
        if let Err(e) = tls.conn.complete_io(&mut tls.sock) {
            return Err(if mismatch(&e) {
                io::Error::new(io::ErrorKind::PermissionDenied, CertMismatch)
            } else {
                e
            });
        }
    }
    Ok(Conn::Tls(Box::new(tls)))
}

/// Whether a handshake failed on our own certificate check.
fn mismatch(e: &io::Error) -> bool {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|tls| {
            matches!(
                tls,
                rustls::Error::InvalidCertificate(
                    rustls::CertificateError::ApplicationVerificationFailure
                )
            )
        })
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn tls_config(pin: Option<[u8; 32]>) -> io::Result<Arc<ClientConfig>> {
    let provider = provider();
    let verifier = Arc::new(Pinned {
        pin,
        algorithms: provider.signature_verification_algorithms,
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

/// Accepts the one certificate `pin` names, or with `None` any (to learn
/// it). Either way the handshake must be signed by that certificate's key.
#[derive(Debug)]
struct Pinned {
    pin: Option<[u8; 32]>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // Validity dates are not checked: the certificate is known by its
        // fingerprint, and a bridge's clock is not to be relied on.
        match self.pin {
            Some(pin) if fingerprint(end_entity.as_ref()) != pin => {
                Err(rustls::Error::InvalidCertificate(
                    rustls::CertificateError::ApplicationVerificationFailure,
                ))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// Write one request on `stream` and read its response, plus whether the
/// connection can carry another: only if we asked to keep it, the server
/// framed its answer exactly and did not ask to close.
fn exchange(
    stream: &mut Conn,
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    headers: &[(&str, &str)],
    keep_alive: bool,
) -> io::Result<(Response, bool)> {
    let body = body.unwrap_or("");
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: {connection}\r\nAccept: application/json\r\n"
    );
    for (name, value) in headers {
        req.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() || method != "GET" {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes())?;

    let timeout = stream
        .tcp()
        .read_timeout()?
        .unwrap_or(Duration::from_secs(2));
    let deadline = Instant::now() + timeout * RESPONSE_TIMEOUTS;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let framed = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            stream.tcp().set_read_timeout(Some(timeout))?;
            return Err(io::Error::new(io::ErrorKind::TimedOut, "response too slow"));
        }
        stream.tcp().set_read_timeout(Some(left.min(timeout)))?;
        match stream.read(&mut buf) {
            Ok(0) => break None,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_RESPONSE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "response too large",
                    ));
                }
                // Stop as soon as the body is complete, rather than waiting
                // for a server that keeps the socket open.
                if let Some(len) = framed_len(&raw) {
                    break Some(len);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if raw.is_empty() {
                    return Err(e);
                }
                break None;
            }
        }
    };
    stream.tcp().set_read_timeout(Some(timeout))?;
    let response = parse_response(&raw)?;
    let reusable = keep_alive && framed == Some(raw.len()) && !closing(&raw);
    Ok((response, reusable))
}

/// The length of the response `raw` starts with, once all of it is there.
/// `None` while it is incomplete, and for one only the server closing ends.
fn framed_len(raw: &[u8]) -> Option<usize> {
    let split = find(raw, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let start = split + 4;
    let status = head.lines().next()?.split_whitespace().nth(1)?;
    if status == "204" || status == "304" {
        return Some(start);
    }
    if header(&head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        return chunked_len(&raw[start..]).map(|len| start + len);
    }
    let len = header(&head, "content-length")?.parse::<usize>().ok()?;
    let end = start.checked_add(len)?;
    (raw.len() >= end).then_some(end)
}

/// The length of a complete chunked body, its final empty chunk included.
/// Trailers are not supported. The sizes come from the device, so a chunk
/// larger than any response we accept ends the search instead of wrapping.
fn chunked_len(data: &[u8]) -> Option<usize> {
    let mut at = 0;
    loop {
        let line_end = at + find(&data[at..], b"\r\n")?;
        let size_text = std::str::from_utf8(&data[at..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        if size > MAX_RESPONSE {
            return None;
        }
        at = line_end + 2 + size + 2;
        if data.len() < at {
            return None;
        }
        if size == 0 {
            return Some(at);
        }
    }
}

/// Whether the server asked to close the connection after this response.
fn closing(raw: &[u8]) -> bool {
    let Some(split) = find(raw, b"\r\n\r\n") else {
        return true;
    };
    let head = String::from_utf8_lossy(&raw[..split]);
    header(&head, "connection").is_some_and(|v| v.eq_ignore_ascii_case("close"))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Split a raw response into status and (de-chunked) body.
pub fn parse_response(raw: &[u8]) -> io::Result<Response> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "malformed HTTP response");
    let split = find(raw, b"\r\n\r\n").ok_or_else(bad)?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| bad())?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(bad)?;
    let mut body = raw[split + 4..].to_vec();
    if header(head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        body = dechunk(&body).ok_or_else(bad)?;
    } else if let Some(len) = header(head, "content-length").and_then(|v| v.parse::<usize>().ok()) {
        body.truncate(len);
    }
    Ok(Response {
        status,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn dechunk(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = find(data, b"\r\n")?;
        let size_text = std::str::from_utf8(&data[..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        if data.len() < size {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_sized_response() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n[]extra").unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, "[]");
        assert!(r.ok());
    }

    #[test]
    fn parses_a_chunked_response() {
        let raw = b"HTTP/1.1 403 Forbidden\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 403);
        assert_eq!(r.body, "{\"a\":1}");
        assert!(!r.ok());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_response(b"nonsense").is_err());
        assert!(parse_response(b"HTTP/1.1 abc\r\n\r\n").is_err());
    }

    #[test]
    fn knows_when_a_body_is_complete() {
        let short = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nab";
        assert_eq!(framed_len(short), None);
        let full = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nab";
        assert_eq!(framed_len(full), Some(full.len()));
        assert_eq!(framed_len(b"HTTP/1.1 200 OK\r\n"), None);
        let empty = b"HTTP/1.1 204 No Content\r\n\r\n";
        assert_eq!(framed_len(empty), Some(empty.len()));
        // Unframed: only the server closing ends it.
        assert_eq!(framed_len(b"HTTP/1.1 200 OK\r\n\r\nab"), None);
    }

    #[test]
    fn a_chunked_body_ends_after_its_last_chunk() {
        let mut raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        raw.extend_from_slice(b"2\r\n[]\r\n0\r\n");
        assert_eq!(framed_len(&raw), None, "the closing CRLF is still to come");
        raw.extend_from_slice(b"\r\n");
        assert_eq!(framed_len(&raw), Some(raw.len()));
    }

    #[test]
    fn an_absurd_size_never_completes() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nFFFFFFFFFFFFFFEC\r\n";
        assert_eq!(framed_len(raw), None);
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 18446744073709551615\r\n\r\n";
        assert_eq!(framed_len(raw), None);
    }

    #[test]
    fn a_trickling_server_times_out() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            let _ = peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n");
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(30));
                if peer.write_all(b"x").is_err() {
                    break;
                }
            }
        });
        let mut stream = Conn::Clear(tcp(addr, Duration::from_millis(100)).unwrap());
        let started = Instant::now();
        // Whatever came by then is all there is: the call never waits longer.
        let _ = exchange(&mut stream, addr, "GET", "/", None, &[], false);
        assert!(started.elapsed() < Duration::from_millis(400));
        drop(stream);
        server.join().unwrap();
    }

    #[test]
    fn a_close_header_retires_the_connection() {
        assert!(closing(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n"));
        assert!(!closing(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"));
    }

    #[test]
    fn refuses_hostnames_and_odd_paths() {
        let t = Duration::from_millis(10);
        assert!(request("GET", "example.com", 80, "/", None, t).is_err());
        assert!(request("GET", "192.0.2.1", 80, "no-slash", None, t).is_err());
        assert!(request("GET", "192.0.2.1", 80, "/a b", None, t).is_err());
        assert!(request("GET", "192.0.2.1", 80, "/a\r\nX: y", None, t).is_err());
    }

    /// A bridge-like certificate: EC P-256, subject CN the bridge id, signed
    /// by a CA named `root-bridge` (test material, made for these tests).
    const TEST_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBvjCCAWOgAwIBAgIUHzOhgKHM7qDOmui6BKRKDoWD1IswCgYIKoZIzj0EAwIw
OTELMAkGA1UEBhMCTkwxFDASBgNVBAoMC1BoaWxpcHMgSHVlMRQwEgYDVQQDDAty
b290LWJyaWRnZTAgFw0yNjEwMjUxNTA0MTVaGA8yMTI2MTAwMTE1MDQxNVowPjEL
MAkGA1UEBhMCTkwxFDASBgNVBAoMC1BoaWxpcHMgSHVlMRkwFwYDVQQDDBAwMDE3
ODhmZmZlMWEyYjNjMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEWXwCTe3QdaH1
4IaF6olTAJVS/H57daDN3rffpFX0UQSiPM68cNuwoDs26chcjZhZ/RaiQZs6fjCz
eXPUDNx17qNCMEAwHQYDVR0OBBYEFNTxgscfAd2tc2gLFM9V7XZPnL/sMB8GA1Ud
IwQYMBaAFDKdPft/NJhVYWw3dq+2tBPhZBTEMAoGCCqGSM49BAMCA0kAMEYCIQC1
u6DhGPeig+hovqCv/lLUKswaHWYJtnXdd+m6QINc8QIhAMqTFWh8a3JEkRVFdmuz
RLQ1VMwgSg6P74fkAB4cLHyX
-----END CERTIFICATE-----";
    const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgwvpa08ix0QUTLnVb
ug5m8R0fmJf21i22EjzrmSj+slChRANCAARZfAJN7dB1ofXghoXqiVMAlVL8fnt1
oM3et9+kVfRRBKI8zrxw27CgOzbpyFyNmFn9FqJBmzp+MLN5c9QM3HXu
-----END PRIVATE KEY-----";

    fn test_cert() -> CertificateDer<'static> {
        use rustls::pki_types::pem::PemObject;
        CertificateDer::from_pem_slice(TEST_CERT.as_bytes()).unwrap()
    }

    /// A TLS server on loopback answering one request with `reply`. Its
    /// thread returns the request it read, or `None` if none arrived.
    fn tls_server(reply: &'static [u8]) -> (SocketAddr, std::thread::JoinHandle<Option<Vec<u8>>>) {
        use rustls::pki_types::PrivateKeyDer;
        use rustls::pki_types::pem::PemObject;
        use std::net::TcpListener;
        let key = PrivateKeyDer::from_pem_slice(TEST_KEY.as_bytes()).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![test_cert()], key)
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().ok()?;
            sock.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
            let conn = rustls::ServerConnection::new(Arc::new(config)).ok()?;
            let mut tls = rustls::StreamOwned::new(conn, sock);
            let mut seen = Vec::new();
            let mut buf = [0u8; 1024];
            while find(&seen, b"\r\n\r\n").is_none() {
                let n = tls.read(&mut buf).ok()?;
                if n == 0 {
                    return None;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            tls.write_all(reply).ok()?;
            tls.flush().ok()?;
            Some(seen)
        });
        (addr, server)
    }

    const OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";

    #[test]
    fn the_pinned_certificate_gets_the_request() {
        let (addr, server) = tls_server(OK);
        let pin = fingerprint(test_cert().as_ref());
        let mut conn = connect_tls(addr, Some(pin), Duration::from_secs(2)).unwrap();
        let (r, _) = exchange(&mut conn, addr, "GET", "/api/secret", None, &[], false).unwrap();
        assert_eq!(r.body, "{}");
        drop(conn);
        let seen = server.join().unwrap().unwrap();
        assert!(seen.starts_with(b"GET /api/secret "));
    }

    #[test]
    fn another_certificate_gets_nothing() {
        let (addr, server) = tls_server(OK);
        let err = match connect_tls(addr, Some([7u8; 32]), Duration::from_secs(2)) {
            Err(e) => e,
            Ok(_) => panic!("a foreign certificate was accepted"),
        };
        assert!(is_cert_mismatch(&err), "{err}");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert!(server.join().unwrap().is_none(), "the host read a request");
    }

    #[test]
    fn learning_reports_the_certificate_shown() {
        let (addr, server) = tls_server(OK);
        let conn = connect_tls(addr, None, Duration::from_secs(2)).unwrap();
        let Conn::Tls(tls) = &conn else {
            panic!("not TLS")
        };
        let shown = tls.conn.peer_certificates().unwrap()[0].clone();
        assert_eq!(shown, test_cert());
        drop(conn);
        let _ = server.join();
    }
}
