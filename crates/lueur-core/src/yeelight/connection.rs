//! TCP control of a Yeelight bulb, including low-latency **music mode**.
//!
//! Two connection styles:
//!
//! * The *control* connection ([`YeelightControl`]) is a normal TCP link to the
//!   bulb's control port. Yeelight rate-limits this to ~60 commands/minute, so
//!   it is only used for one-shot commands (power, discovery test, enabling
//!   music mode).
//!
//! * *Music mode* ([`MusicConnection`]) flips the direction: we open a listener,
//!   tell the bulb (over the control connection) to connect back to us, and then
//!   stream `set_rgb` commands over the socket the bulb opens. There is **no
//!   rate limit** on this channel, which is what makes fluid ambilight possible.

use super::protocol::{self, Effect};
use crate::color::Rgb;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

static COMMAND_ID: AtomicU32 = AtomicU32::new(1);

fn next_id() -> u32 {
    COMMAND_ID.fetch_add(1, Ordering::Relaxed)
}

/// A short-lived control connection for one-shot commands.
pub struct YeelightControl {
    stream: TcpStream,
}

impl YeelightControl {
    /// Connect to the bulb's control port with a connect timeout.
    pub fn connect(ip: &str, port: u16, timeout: Duration) -> io::Result<Self> {
        let addr: SocketAddr = format!("{ip}:{port}")
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad address"))?;
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_nodelay(true)?; // ambilight cares about latency, not throughput
        Ok(Self { stream })
    }

    fn send_line(&mut self, line: &str) -> io::Result<()> {
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()
    }

    /// Turn the bulb on or off (smooth by default).
    pub fn set_power(&mut self, on: bool) -> io::Result<()> {
        self.send_line(&protocol::set_power(next_id(), on, Effect::Smooth, 300))
    }

    /// Set a colour once (used for the "test" button in the UI).
    pub fn set_rgb(&mut self, color: Rgb) -> io::Result<()> {
        self.send_line(&protocol::set_rgb(next_id(), color, Effect::Smooth, 300))
    }

    /// Ask the bulb to open a music-mode connection back to `host:port`.
    pub fn enable_music(&mut self, host: &str, port: u16) -> io::Result<()> {
        self.send_line(&protocol::set_music(next_id(), true, host, port))
    }

    /// Disable music mode (bulb returns to normal rate-limited control).
    pub fn disable_music(&mut self) -> io::Result<()> {
        self.send_line(&protocol::set_music(next_id(), false, "", 0))
    }

    /// Read and discard any pending response bytes (best-effort).
    pub fn drain(&mut self) {
        let mut buf = [0u8; 1024];
        let _ = self.stream.read(&mut buf);
    }
}

/// A live music-mode channel to one bulb. Writing a colour is a single small,
/// buffered, unthrottled TCP send.
pub struct MusicConnection {
    stream: TcpStream,
    ip: String,
    port: u16,
}

impl MusicConnection {
    /// Establish music mode with a bulb.
    ///
    /// Steps:
    /// 1. Bind a listener on an ephemeral local port.
    /// 2. Discover the local IP the bulb can reach us on.
    /// 3. Open a control connection and send `set_music(1, our_ip, our_port)`.
    /// 4. Accept the connection the bulb opens back to us.
    ///
    /// `handshake_timeout` bounds the accept step.
    pub fn establish(ip: &str, port: u16, handshake_timeout: Duration) -> io::Result<Self> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        let local_port = listener.local_addr()?.port();

        let local_ip = local_ip_toward(ip, port)?;

        // Tell the bulb to connect back to us.
        let mut control = YeelightControl::connect(ip, port, handshake_timeout)?;
        control.enable_music(&local_ip, local_port)?;
        control.drain();

        // Accept the bulb's inbound connection within the timeout.
        listener.set_nonblocking(true)?;
        let deadline = std::time::Instant::now() + handshake_timeout;
        let stream = loop {
            match listener.accept() {
                Ok((s, _peer)) => break s,
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "bulb did not open music connection in time",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(e),
            }
        };

        stream.set_nonblocking(false)?;
        stream.set_nodelay(true)?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;

        Ok(Self {
            stream,
            ip: ip.to_string(),
            port,
        })
    }

    pub fn ip(&self) -> &str {
        &self.ip
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Push a colour with the given effect. On the music channel this is not rate
    /// limited, so it can be called every frame.
    pub fn set_rgb(&mut self, color: Rgb, effect: Effect, duration_ms: u32) -> io::Result<()> {
        let line = protocol::set_rgb(next_id(), color, effect, duration_ms);
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()
    }

    /// Set brightness on the music channel.
    pub fn set_bright(&mut self, brightness: u8, effect: Effect, duration_ms: u32) -> io::Result<()> {
        let line = protocol::set_bright(next_id(), brightness, effect, duration_ms);
        self.stream.write_all(line.as_bytes())?;
        self.stream.flush()
    }
}

/// Determine the local IPv4 address that routes toward `peer_ip`.
///
/// Trick: "connect" a UDP socket to the peer (no packets are actually sent for
/// UDP) and read back the local address the OS selected. This picks the right
/// interface on multi-homed machines without any external dependency.
fn local_ip_toward(peer_ip: &str, peer_port: u16) -> io::Result<String> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect(format!("{peer_ip}:{peer_port}"))?;
    Ok(socket.local_addr()?.ip().to_string())
}
