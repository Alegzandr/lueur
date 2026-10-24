//! Just enough HTTP/1.1 to talk to a Hue bridge or a Nanoleaf controller on
//! the LAN: one request per connection, plain text, bounded in size and time.
//!
//! A general-purpose client would pull TLS, async and redirects into the core
//! crate for devices that need none of it. Only bare IP addresses are
//! accepted, so a request can never be steered to a name or a URL.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpStream};
use std::time::Duration;

/// Bodies larger than this are refused: a bulb's answer is a few kilobytes.
const MAX_RESPONSE: usize = 1 << 20;

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

/// Send one request to `ip:port` and read the whole response.
pub fn request(
    method: &str,
    ip: &str,
    port: u16,
    path: &str,
    body: Option<&str>,
    timeout: Duration,
) -> io::Result<Response> {
    let ip = super::common::ipv4(ip)?;
    if !path.starts_with('/') || path.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad path"));
    }
    let addr = SocketAddr::V4(SocketAddrV4::new(ip, port));
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;

    let body = body.unwrap_or("");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ip}:{port}\r\nConnection: close\r\nAccept: application/json\r\n"
    );
    if !body.is_empty() || method != "GET" {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes())?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > MAX_RESPONSE {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "response too large",
                    ));
                }
                // Stop as soon as a Content-Length body is complete, rather
                // than waiting for a server that keeps the socket open.
                if complete(&raw) {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if raw.is_empty() {
                    return Err(e);
                }
                break;
            }
        }
    }
    parse_response(&raw)
}

/// Whether `raw` already holds a full response with a declared length.
fn complete(raw: &[u8]) -> bool {
    let Some(split) = find(raw, b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&raw[..split]);
    header(&head, "content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|len| raw.len() >= split + 4 + len)
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
        assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nab"));
        assert!(complete(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nab"));
        assert!(!complete(b"HTTP/1.1 200 OK\r\n"));
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
