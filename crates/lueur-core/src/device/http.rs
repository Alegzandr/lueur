//! Just enough HTTP/1.1 to talk to a Hue bridge or a Nanoleaf controller on
//! the LAN: plain text, bounded in size and time. One-off calls get a
//! connection each; a [`Client`] keeps one open for a stream that sends a
//! request every frame.
//!
//! A general-purpose client would pull TLS, async and redirects into the core
//! crate for devices that need none of it. Only bare IP addresses are
//! accepted, so a request can never be steered to a name or a URL.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream};
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
    let addr = address(ip, port)?;
    check_path(path)?;
    let mut stream = connect(addr, timeout)?;
    exchange(&mut stream, addr, method, path, body, false).map(|(response, _)| response)
}

/// One kept-alive connection to one host.
///
/// A stream that sends a command every frame would otherwise pay a TCP
/// handshake per command, on top of the request itself.
pub struct Client {
    addr: SocketAddr,
    timeout: Duration,
    stream: Option<TcpStream>,
}

impl Client {
    pub fn new(ip: &str, port: u16, timeout: Duration) -> io::Result<Self> {
        Ok(Client {
            addr: address(ip, port)?,
            timeout,
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
                exchange(&mut stream, self.addr, method, path, body, true)
        {
            if reusable {
                self.stream = Some(stream);
            }
            return Ok(response);
        }
        let mut stream = connect(self.addr, self.timeout)?;
        let (response, reusable) = exchange(&mut stream, self.addr, method, path, body, true)?;
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

fn connect(addr: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Write one request on `stream` and read its response, plus whether the
/// connection can carry another: only if we asked to keep it, the server
/// framed its answer exactly and did not ask to close.
fn exchange(
    stream: &mut TcpStream,
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    keep_alive: bool,
) -> io::Result<(Response, bool)> {
    let body = body.unwrap_or("");
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: {connection}\r\nAccept: application/json\r\n"
    );
    if !body.is_empty() || method != "GET" {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes())?;

    let timeout = stream.read_timeout()?.unwrap_or(Duration::from_secs(2));
    let deadline = Instant::now() + timeout * RESPONSE_TIMEOUTS;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    let framed = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            stream.set_read_timeout(Some(timeout))?;
            return Err(io::Error::new(io::ErrorKind::TimedOut, "response too slow"));
        }
        stream.set_read_timeout(Some(left.min(timeout)))?;
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
    stream.set_read_timeout(Some(timeout))?;
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
        let mut stream = connect(addr, Duration::from_millis(100)).unwrap();
        let started = Instant::now();
        // Whatever came by then is all there is: the call never waits longer.
        let _ = exchange(&mut stream, addr, "GET", "/", None, false);
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
}
