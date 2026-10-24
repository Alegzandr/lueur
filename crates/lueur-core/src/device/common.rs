//! Pieces every family shares: skipping frames the bulb already shows, pacing
//! to what a device tolerates, moving slow protocols off the frame loop, and
//! finding our own address on the LAN.

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT, Rgb};
use crate::yeelight::Effect;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Smallest brightness change worth a command, in percentage points.
///
/// Brightness is derived from a frame average, so it jitters by a point or two
/// even on still content. Without a deadband that jitter becomes a brightness
/// command every frame, racing the colour stream and visibly destabilising the
/// bulb. A 1-point error is imperceptible; a command storm is not.
pub const BRIGHT_DEADBAND: u8 = 2;

/// Below this brightness every point is sent. Down there a point is a large
/// share of the light (3% is three times 1%), so skipping one is a visible
/// step in a dark scene, not an invisible error.
pub const LOW_BRIGHT: u8 = 20;

/// Whether a brightness change is worth sending.
pub fn bright_needs_update(last: Option<u8>, next: u8) -> bool {
    match last {
        None => true,
        // Always honour the extremes: they are what "off-dark" and "full" look
        // like, and a deadband must never leave the bulb stranded short of them.
        Some(prev) => {
            next != prev
                && (next == MIN_BRIGHT
                    || next == MAX_BRIGHT
                    || prev.min(next) < LOW_BRIGHT
                    || prev.abs_diff(next) >= BRIGHT_DEADBAND)
        }
    }
}

/// What a bulb was last sent, so a still scene costs nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct LastSent(Option<BulbOutput>);

impl LastSent {
    /// Whether `next` differs enough from what the bulb shows to be sent.
    pub fn needs(&self, next: BulbOutput) -> bool {
        let Some(prev) = self.0 else {
            return true;
        };
        let same_chroma = match (prev, next) {
            (BulbOutput::Color { rgb: a, .. }, BulbOutput::Color { rgb: b, .. }) => a == b,
            (BulbOutput::White { kelvin: a, .. }, BulbOutput::White { kelvin: b, .. }) => a == b,
            _ => false,
        };
        !same_chroma || bright_needs_update(Some(prev.bright()), next.bright())
    }

    pub fn mark(&mut self, sent: BulbOutput) {
        self.0 = Some(sent);
    }
}

/// Lets at most one command through per `interval`.
///
/// A frame refused here is not lost: [`LastSent`] still holds the older value,
/// so the next frame after the interval carries the newest colour.
#[derive(Debug, Clone, Copy)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Throttle {
            interval,
            last: None,
        }
    }

    /// The least time between two commands.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// A fade at least as long as the gap to the next command, so a paced
    /// bulb keeps moving between two of them instead of stepping.
    pub fn glide(&self, effect: Effect, duration_ms: u32) -> u32 {
        match effect {
            Effect::Smooth => duration_ms.max(self.interval.as_millis() as u32),
            Effect::Sudden => 0,
        }
    }

    /// True (and the clock restarts) if a command may go now.
    pub fn ready(&mut self) -> bool {
        let now = Instant::now();
        match self.last {
            Some(t) if now.duration_since(t) < self.interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// One frame's worth of work for a [`Mailbox`] worker.
#[derive(Debug, Clone, Copy)]
pub enum Job {
    Frame(BulbOutput, Effect, u32),
    Rest,
}

/// Something that can drive a bulb, slowly: an HTTP request or an encrypted
/// TCP exchange per frame, or a TCP write that blocks when the bulb stops
/// reading.
pub trait Driver: Send + 'static {
    fn send(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()>;
    fn rest(&mut self);
}

/// Runs a [`Driver`] on its own thread, always on the newest frame.
///
/// The frame loop must never wait on a bulb: a REST call can take 30 ms, and
/// six of those in a row would halve the frame rate for every light. Here a
/// push only drops the frame into a one-slot mailbox and returns; the worker
/// sends whatever is newest when it is free, and frames it never got to are
/// simply superseded.
pub struct Mailbox {
    slot: SyncSender<Job>,
    latest: Arc<Mutex<Option<Job>>>,
    failed: Arc<Mutex<Option<io::Error>>>,
    worker: Option<JoinHandle<()>>,
}

impl Mailbox {
    pub fn spawn(name: &str, mut driver: impl Driver) -> io::Result<Self> {
        // The channel only wakes the worker; the frame itself sits in `latest`
        // so a backlog can never build up.
        let (tx, rx): (SyncSender<Job>, Receiver<Job>) = mpsc::sync_channel(1);
        let latest: Arc<Mutex<Option<Job>>> = Arc::new(Mutex::new(None));
        let failed: Arc<Mutex<Option<io::Error>>> = Arc::new(Mutex::new(None));
        let worker = {
            let latest = Arc::clone(&latest);
            let failed = Arc::clone(&failed);
            std::thread::Builder::new()
                .name(format!("lueur-{name}"))
                .spawn(move || {
                    // Wakes at least once a second; ends when the sender is dropped.
                    while let Ok(_) | Err(RecvTimeoutError::Timeout) =
                        rx.recv_timeout(Duration::from_secs(1))
                    {
                        let job = latest.lock().ok().and_then(|mut g| g.take());
                        match job {
                            Some(Job::Frame(output, effect, ms)) => {
                                if let Err(e) = driver.send(output, effect, ms) {
                                    if let Ok(mut f) = failed.lock() {
                                        *f = Some(e);
                                    }
                                    break;
                                }
                            }
                            Some(Job::Rest) => {
                                driver.rest();
                                break;
                            }
                            None => {}
                        }
                    }
                })?
        };
        Ok(Mailbox {
            slot: tx,
            latest,
            failed,
            worker: Some(worker),
        })
    }

    fn post(&self, job: Job) -> io::Result<()> {
        if let Some(e) = self.failed.lock().ok().and_then(|mut f| f.take()) {
            return Err(e);
        }
        if let Ok(mut g) = self.latest.lock() {
            // A pending rest is never overwritten by a late frame.
            if !matches!(*g, Some(Job::Rest)) {
                *g = Some(job);
            }
        }
        match self.slot.try_send(job) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "bulb worker stopped",
            )),
        }
    }

    pub fn push(&self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        self.post(Job::Frame(output, effect, duration_ms))
    }

    /// Ask the worker to settle the bulb, and wait (bounded) for it to do so.
    pub fn rest(&mut self) {
        let _ = self.post(Job::Rest);
        if let Some(w) = self.worker.take() {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !w.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if w.is_finished() {
                let _ = w.join();
            }
        }
    }
}

/// The local IPv4 address that routes toward `peer`.
///
/// "Connecting" a UDP socket sends nothing; it only makes the OS choose the
/// interface, which picks the right one on multi-homed machines.
pub fn local_ip_toward(peer: Ipv4Addr) -> io::Result<Ipv4Addr> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect(SocketAddrV4::new(peer, 9))?;
    match socket.local_addr()?.ip() {
        std::net::IpAddr::V4(ip) => Ok(ip),
        std::net::IpAddr::V6(_) => Err(io::Error::other("no IPv4 route")),
    }
}

/// The addresses a LAN-wide broadcast should go to.
///
/// The all-ones address alone is unreliable on Windows, which sends it out of
/// a single interface, so the /24 broadcast of our main LAN address goes too.
/// (Home networks are almost always /24.)
pub fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut out = vec![Ipv4Addr::BROADCAST];
    // Any private address works as the probe: nothing is sent.
    if let Ok(local) = local_ip_toward(Ipv4Addr::new(192, 168, 0, 1))
        && local.is_private()
    {
        let [a, b, c, _] = local.octets();
        out.push(Ipv4Addr::new(a, b, c, 255));
    }
    out
}

/// Every host of the /24 our main LAN address sits in, ourselves excluded.
pub fn lan_hosts() -> Vec<Ipv4Addr> {
    let Ok(local) = local_ip_toward(Ipv4Addr::new(192, 168, 0, 1)) else {
        return Vec::new();
    };
    if !local.is_private() {
        return Vec::new();
    }
    let [a, b, c, me] = local.octets();
    (1..=254u8)
        .filter(|&d| d != me)
        .map(|d| Ipv4Addr::new(a, b, c, d))
        .collect()
}

/// Parse a bare IPv4 address a bulb can have: private or link-local.
///
/// Every connection to a bulb goes through here, so an address taken from a
/// config file or a discovery reply can never send traffic to the internet,
/// to this machine's own services, or to a multicast group. (A subnet's
/// broadcast address passes: on a /16 the same `.255` is a host, and the OS
/// refuses broadcast sends on these sockets anyway.)
pub fn ipv4(ip: &str) -> io::Result<Ipv4Addr> {
    let addr: Ipv4Addr = ip
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "bad address"))?;
    if !(addr.is_private() || addr.is_link_local()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a LAN address",
        ));
    }
    Ok(addr)
}

/// A UDP socket "connected" to one bulb, so sends are a single syscall and
/// stray datagrams from elsewhere are ignored.
pub fn udp_to(ip: &str, port: u16) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect(SocketAddrV4::new(ipv4(ip)?, port))?;
    socket.set_write_timeout(Some(Duration::from_millis(500)))?;
    Ok(socket)
}

/// Linear interpolation of a 1..=100 brightness onto a device range.
pub fn scale_bright(bright: u8, min: u32, max: u32) -> u32 {
    let b = u32::from(bright.clamp(MIN_BRIGHT, MAX_BRIGHT));
    min + (max.saturating_sub(min)) * (b - 1) / 99
}

/// Approximate RGB of a black body at `kelvin`, for devices with no white
/// channel of their own (Tanner Helland's fit).
pub fn kelvin_to_rgb(kelvin: u16) -> Rgb {
    let t = f32::from(kelvin.clamp(1000, 40000)) / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698_73 * (t - 60.0).powf(-0.133_204_76)
    };
    let g = if t <= 66.0 {
        99.470_8 * t.ln() - 161.119_57
    } else {
        288.122_16 * (t - 60.0).powf(-0.075_514_846)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    Rgb::new(c(r), c(g), c(b))
}

/// Hue (0..360), saturation and value (0..1) of a colour.
pub fn rgb_to_hsv(c: Rgb) -> (f32, f32, f32) {
    let r = f32::from(c.r) / 255.0;
    let g = f32::from(c.g) / 255.0;
    let b = f32::from(c.b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb::new(255, 0, 0);

    #[test]
    fn only_lan_addresses_are_bulbs() {
        for ok in ["192.168.1.5", "10.0.0.2", "172.16.4.1", "169.254.3.3"] {
            assert!(ipv4(ok).is_ok(), "{ok}");
        }
        for bad in [
            "8.8.8.8",
            "127.0.0.1",
            "0.0.0.0",
            "224.0.0.251",
            "255.255.255.255",
            "::1",
            "fe80::1",
            "lamp.local",
        ] {
            assert!(ipv4(bad).is_err(), "{bad}");
        }
    }

    fn color(rgb: Rgb, bright: u8) -> BulbOutput {
        BulbOutput::Color { rgb, bright }
    }

    #[test]
    fn first_frame_is_always_sent() {
        assert!(LastSent::default().needs(color(RED, 50)));
    }

    #[test]
    fn a_still_scene_is_skipped() {
        let mut last = LastSent::default();
        last.mark(color(RED, 50));
        assert!(!last.needs(color(RED, 50)));
        assert!(!last.needs(color(RED, 51)), "1-point jitter is absorbed");
        assert!(last.needs(color(RED, 53)));
        assert!(last.needs(color(Rgb::new(0, 0, 255), 50)));
    }

    #[test]
    fn switching_to_white_is_sent() {
        let mut last = LastSent::default();
        last.mark(color(RED, 50));
        assert!(last.needs(BulbOutput::White {
            kelvin: 4000,
            bright: 50
        }));
    }

    #[test]
    fn first_brightness_always_sends() {
        assert!(bright_needs_update(None, 50));
    }

    #[test]
    fn single_point_jitter_is_absorbed() {
        // The command storm this prevents is what destabilises the bulb.
        assert!(!bright_needs_update(Some(50), 50));
        assert!(!bright_needs_update(Some(50), 51));
        assert!(!bright_needs_update(Some(50), 49));
    }

    #[test]
    fn dark_scenes_move_point_by_point() {
        assert!(bright_needs_update(Some(3), 4));
        assert!(bright_needs_update(Some(20), 19));
        assert!(!bright_needs_update(Some(20), 21));
    }

    #[test]
    fn real_movement_gets_through() {
        assert!(bright_needs_update(Some(50), 52));
        assert!(bright_needs_update(Some(50), 48));
        assert!(bright_needs_update(Some(50), 90));
    }

    #[test]
    fn extremes_are_never_absorbed() {
        // Drifting to 2 and stopping would leave the bulb visibly glowing when
        // the scene is black; same story at the top of the range.
        assert!(bright_needs_update(Some(2), MIN_BRIGHT));
        assert!(bright_needs_update(Some(99), MAX_BRIGHT));
    }

    #[test]
    fn throttle_spaces_commands() {
        let mut t = Throttle::new(Duration::from_secs(60));
        assert!(t.ready());
        assert!(!t.ready());
        let mut open = Throttle::new(Duration::ZERO);
        assert!(open.ready());
        assert!(open.ready());
    }

    #[test]
    fn a_paced_fade_spans_the_gap() {
        let t = Throttle::new(Duration::from_millis(300));
        assert_eq!(t.glide(Effect::Smooth, 40), 300);
        assert_eq!(t.glide(Effect::Smooth, 800), 800);
        assert_eq!(t.glide(Effect::Sudden, 800), 0);
    }

    #[test]
    fn brightness_scales_onto_device_ranges() {
        assert_eq!(scale_bright(1, 10, 1000), 10);
        assert_eq!(scale_bright(100, 10, 1000), 1000);
        assert_eq!(scale_bright(0, 1, 254), 1, "0 is clamped to the floor");
        let mid = scale_bright(50, 0, 1000);
        assert!((490..=500).contains(&mid), "{mid}");
    }

    #[test]
    fn warm_white_is_warm() {
        let warm = kelvin_to_rgb(2700);
        assert_eq!(warm.r, 255);
        assert!(warm.b < warm.g && warm.g < warm.r, "{warm:?}");
        let daylight = kelvin_to_rgb(6500);
        assert!(daylight.b > 240, "{daylight:?}");
    }

    #[test]
    fn hsv_of_primaries() {
        assert_eq!(rgb_to_hsv(RED), (0.0, 1.0, 1.0));
        let (h, s, v) = rgb_to_hsv(Rgb::new(0, 0, 255));
        assert_eq!((h.round(), s, v), (240.0, 1.0, 1.0));
        let (h, _, _) = rgb_to_hsv(Rgb::new(255, 0, 128));
        assert!(h > 300.0 && h < 360.0, "{h}");
        assert_eq!(rgb_to_hsv(Rgb::BLACK), (0.0, 0.0, 0.0));
    }

    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl Driver for Recorder {
        fn send(&mut self, output: BulbOutput, _: Effect, _: u32) -> io::Result<()> {
            std::thread::sleep(Duration::from_millis(20));
            self.0.lock().unwrap().push(format!("{}", output.bright()));
            Ok(())
        }
        fn rest(&mut self) {
            self.0.lock().unwrap().push("rest".into());
        }
    }

    #[test]
    fn mailbox_never_blocks_and_keeps_the_newest_frame() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut mb = Mailbox::spawn("test", Recorder(Arc::clone(&log))).unwrap();
        let start = Instant::now();
        for b in 1..=50u8 {
            mb.push(color(RED, b), Effect::Sudden, 0).unwrap();
        }
        // 50 frames at 20 ms each would take a second if push waited.
        assert!(start.elapsed() < Duration::from_millis(200));
        std::thread::sleep(Duration::from_millis(100));
        mb.rest();
        let log = log.lock().unwrap();
        assert_eq!(log.last().map(String::as_str), Some("rest"));
        assert!(log.len() < 50, "superseded frames are dropped: {log:?}");
        assert!(
            log.iter().any(|s| s == "50"),
            "the newest frame lands: {log:?}"
        );
    }
}
