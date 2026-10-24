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
use crate::color::{BulbOutput, Rgb};
use crate::device::common::{bright_needs_update, ipv4, local_ip_toward};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
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
        let addr = SocketAddr::V4(SocketAddrV4::new(ipv4(ip)?, port));
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
    sent: Sent,
}

/// What the bulb was last told on each channel, so [`MusicConnection::push`]
/// only sends what moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sent {
    bright: Option<u8>,
    /// The colour, while the bulb is in colour mode.
    rgb: Option<Rgb>,
    /// The temperature, while the bulb is in white mode.
    kelvin: Option<u16>,
}

impl Sent {
    /// The commands needed to take the bulb from this state to `output`, in
    /// the order they must be sent, and the state they leave it in.
    fn plan(self, output: BulbOutput) -> (Option<u8>, Option<BulbOutput>, Sent) {
        let mut next = self;
        let bright = bright_needs_update(self.bright, output.bright()).then(|| output.bright());
        if let Some(b) = bright {
            next.bright = Some(b);
        }
        let mode = match output {
            BulbOutput::Color { rgb, .. } if self.rgb != Some(rgb) => {
                next.rgb = Some(rgb);
                next.kelvin = None;
                Some(output)
            }
            // Re-sending the same temperature every frame would be pure noise;
            // the bulb holds it until we change modes.
            BulbOutput::White { kelvin, .. } if self.kelvin != Some(kelvin) => {
                next.kelvin = Some(kelvin);
                next.rgb = None;
                Some(output)
            }
            _ => None,
        };
        (bright, mode, next)
    }
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

        let bulb = ipv4(ip)?;
        let local_ip = local_ip_toward(bulb)?.to_string();

        // Tell the bulb to connect back to us.
        let mut control = YeelightControl::connect(ip, port, handshake_timeout)?;
        control.enable_music(&local_ip, local_port)?;
        control.drain();

        // Accept the bulb's inbound connection within the timeout.
        listener.set_nonblocking(true)?;
        let deadline = std::time::Instant::now() + handshake_timeout;
        let stream = loop {
            match listener.accept() {
                // Only the bulb itself may take the music channel; anyone
                // else on the LAN who connects first is turned away.
                Ok((s, peer)) if peer.ip() == IpAddr::V4(bulb) => break s,
                Ok(_) => continue,
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
            sent: Sent::default(),
        })
    }

    /// Set brightness on the music channel.
    pub fn set_bright(
        &mut self,
        brightness: u8,
        effect: Effect,
        duration_ms: u32,
    ) -> io::Result<()> {
        let line = protocol::set_bright(next_id(), brightness, effect, duration_ms);
        self.sent = Sent::default();
        self.stream.write_all(line.as_bytes())
    }

    /// Set a white colour temperature (Kelvin) on the music channel.
    pub fn set_ct(&mut self, kelvin: u16, effect: Effect, duration_ms: u32) -> io::Result<()> {
        let line = protocol::set_ct_abx(next_id(), kelvin, effect, duration_ms);
        self.sent = Sent::default();
        self.stream.write_all(line.as_bytes())
    }

    /// Push one [`BulbOutput`] — the correct pairing of chromaticity and
    /// brightness for this bulb.
    ///
    /// Only what changed since the last push goes out, and it goes out as one
    /// write: a steady scene costs nothing, a moving one a single packet. Every
    /// command queued here is one the bulb has to work through before it gets
    /// to the next frame, so silence is the lowest-latency command there is.
    pub fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        let (bright, mode, next) = self.sent.plan(output);
        let mut batch = String::new();
        if let Some(b) = bright {
            batch.push_str(&protocol::set_bright(next_id(), b, effect, duration_ms));
        }
        match mode {
            Some(BulbOutput::Color { rgb, .. }) => {
                batch.push_str(&protocol::set_rgb(next_id(), rgb, effect, duration_ms))
            }
            Some(BulbOutput::White { kelvin, .. }) => batch.push_str(&protocol::set_ct_abx(
                next_id(),
                kelvin,
                effect,
                duration_ms,
            )),
            None => {}
        }
        if batch.is_empty() {
            return Ok(());
        }
        self.stream.write_all(batch.as_bytes())?;
        self.sent = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);

    fn color(rgb: Rgb, bright: u8) -> BulbOutput {
        BulbOutput::Color { rgb, bright }
    }

    #[test]
    fn first_push_sends_everything() {
        let (bright, mode, _) = Sent::default().plan(color(RED, 50));
        assert_eq!(bright, Some(50));
        assert_eq!(mode, Some(color(RED, 50)));
    }

    #[test]
    fn a_steady_scene_sends_nothing() {
        let (_, _, sent) = Sent::default().plan(color(RED, 50));
        assert_eq!(sent.plan(color(RED, 50)), (None, None, sent));
    }

    #[test]
    fn a_slow_fade_still_reaches_the_bulb() {
        // The deadband is measured from what the bulb last *received*. Measured
        // from the previous frame instead, a fade of one point per frame never
        // clears it and the bulb sits at its old brightness indefinitely.
        let (_, _, mut sent) = Sent::default().plan(color(RED, 50));
        let mut delivered = Vec::new();
        for b in 51..=54 {
            let (bright, _, next) = sent.plan(color(RED, b));
            delivered.extend(bright);
            sent = next;
        }
        assert_eq!(delivered, [52, 54]);
    }

    #[test]
    fn leaving_white_resends_the_colour() {
        let (_, _, sent) = Sent::default().plan(color(RED, 50));
        let white = BulbOutput::White {
            kelvin: 4000,
            bright: 50,
        };
        let (_, mode, sent) = sent.plan(white);
        assert_eq!(mode, Some(white));
        // Same colour as before white, but the bulb is in CT mode now.
        let (_, mode, _) = sent.plan(color(RED, 50));
        assert_eq!(mode, Some(color(RED, 50)));
    }
}
