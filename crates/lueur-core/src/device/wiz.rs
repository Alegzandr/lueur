//! WiZ (Philips WiZ, and the many brands on its platform): JSON over UDP 38899.
//!
//! No pairing and no key: any client on the LAN may set a bulb. A bulb takes
//! a colour and a dimming level in one `setPilot`, which suits the per-frame
//! stream well.

use super::common::{LastSent, Throttle, broadcast_targets, scale_bright, udp_to};
use super::wire::Wire;
use super::{DeviceKind, DiscoveredDevice, LightStream, REST_BRIGHT, REST_KELVIN};
use crate::color::BulbOutput;
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

pub const PORT: u16 = 38899;

/// WiZ refuses dimming below 10%.
const MIN_DIMMING: u32 = 10;
/// The white range WiZ RGB bulbs accept.
const MIN_KELVIN: u16 = 2200;
const MAX_KELVIN: u16 = 6500;

/// About what a WiZ bulb follows smoothly; faster just queues up.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(40);
/// How long a command takes to show: a datagram, and the bulb's own short fade.
pub(crate) const LATENCY: Duration = Duration::from_millis(50);

/// The `setPilot` command for one output.
pub fn pilot(output: BulbOutput) -> Value {
    match output {
        BulbOutput::Color { rgb, bright } => json!({
            "method": "setPilot",
            "params": {
                "r": rgb.r, "g": rgb.g, "b": rgb.b,
                "dimming": scale_bright(bright, MIN_DIMMING, 100),
            }
        }),
        BulbOutput::White { kelvin, bright } => json!({
            "method": "setPilot",
            "params": {
                "temp": kelvin.clamp(MIN_KELVIN, MAX_KELVIN),
                "dimming": scale_bright(bright, MIN_DIMMING, 100),
            }
        }),
    }
}

/// Turn a WiZ bulb on or off.
pub fn set_power(ip: &str, on: bool) -> io::Result<()> {
    let socket = udp_to(ip, PORT)?;
    let msg = json!({"method": "setState", "params": {"state": on}}).to_string();
    // UDP is lossy and a power command is rare: say it twice.
    for _ in 0..2 {
        socket.send(msg.as_bytes())?;
    }
    Ok(())
}

pub struct WizStream {
    socket: UdpSocket,
    last: LastSent,
    pace: Throttle,
    wire: Wire,
}

impl WizStream {
    pub fn open(ip: &str, wire: Wire) -> io::Result<Self> {
        Ok(WizStream {
            socket: udp_to(ip, PORT)?,
            last: LastSent::default(),
            pace: Throttle::new(FRAME_INTERVAL),
            wire,
        })
    }
}

impl LightStream for WizStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn latency(&self) -> Duration {
        LATENCY
    }

    fn push(&mut self, output: BulbOutput, _: Effect, _: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        let msg = pilot(output).to_string();
        self.wire.text(&msg);
        self.socket.send(msg.as_bytes())?;
        self.last.mark(output);
        Ok(())
    }

    fn rest(&mut self) {
        let msg = pilot(BulbOutput::White {
            kelvin: REST_KELVIN,
            bright: REST_BRIGHT,
        })
        .to_string();
        self.wire.text(&msg);
        for _ in 0..2 {
            let _ = self.socket.send(msg.as_bytes());
        }
    }
}

/// Broadcast a system-config query and collect the bulbs that answer.
pub fn discover(timeout: Duration) -> io::Result<Vec<DiscoveredDevice>> {
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(200)))?;
    let query = json!({"method": "getSystemConfig", "params": {}}).to_string();
    for target in broadcast_targets() {
        for _ in 0..2 {
            let _ = socket.send_to(query.as_bytes(), SocketAddrV4::new(target, PORT));
        }
    }

    let mut found: HashMap<String, DiscoveredDevice> = HashMap::new();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, SocketAddr::V4(from))) => {
                if let Some(d) = parse_reply(&buf[..n], *from.ip()) {
                    found.insert(d.id.clone(), d);
                }
            }
            Ok(_) => {}
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
            }
            Err(e) => return Err(e),
        }
    }
    Ok(found.into_values().collect())
}

/// A `getSystemConfig` reply from `from`, if it is one.
pub fn parse_reply(raw: &[u8], from: Ipv4Addr) -> Option<DiscoveredDevice> {
    let v: Value = serde_json::from_slice(raw).ok()?;
    let result = v.get("result")?;
    let mac = result.get("mac")?.as_str()?.to_ascii_lowercase();
    if mac.len() != 12 || !mac.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let text = |k: &str| result.get(k).and_then(Value::as_str).map(str::to_string);
    Some(DiscoveredDevice {
        id: format!("wiz:{mac}"),
        ip: from.to_string(),
        port: PORT,
        model: text("moduleName").unwrap_or_default(),
        // WiZ keeps room and bulb names in its cloud, not on the bulb.
        name: String::new(),
        support: Vec::new(),
        fw_ver: text("fwVersion"),
        kind: DeviceKind::Wiz,
        paired: true,
        faster: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    #[test]
    fn colour_pilot_carries_rgb_and_dimming() {
        let v = pilot(BulbOutput::Color {
            rgb: Rgb::new(255, 10, 0),
            bright: 100,
        });
        assert_eq!(v["method"], "setPilot");
        assert_eq!(v["params"]["r"], 255);
        assert_eq!(v["params"]["g"], 10);
        assert_eq!(v["params"]["dimming"], 100);
    }

    #[test]
    fn dimming_never_drops_below_wiz_floor() {
        let v = pilot(BulbOutput::Color {
            rgb: Rgb::new(255, 0, 0),
            bright: 1,
        });
        assert_eq!(v["params"]["dimming"], 10);
    }

    #[test]
    fn white_pilot_clamps_temperature() {
        let v = pilot(BulbOutput::White {
            kelvin: 1700,
            bright: 50,
        });
        assert_eq!(v["params"]["temp"], 2200);
        assert!(v["params"].get("r").is_none());
    }

    #[test]
    fn parses_a_system_config_reply() {
        let raw = br#"{"method":"getSystemConfig","env":"pro","result":{"mac":"A8BB5006033D","homeId":1,"moduleName":"ESP01_SHRGB1C_31","fwVersion":"1.22.0"}}"#;
        let d = parse_reply(raw, Ipv4Addr::new(192, 168, 1, 44)).unwrap();
        assert_eq!(d.id, "wiz:a8bb5006033d");
        assert_eq!(d.ip, "192.168.1.44");
        assert_eq!(d.model, "ESP01_SHRGB1C_31");
        assert_eq!(d.kind, DeviceKind::Wiz);
    }

    #[test]
    fn ignores_our_own_query_and_noise() {
        let q = json!({"method": "getSystemConfig", "params": {}}).to_string();
        assert!(parse_reply(q.as_bytes(), Ipv4Addr::LOCALHOST).is_none());
        assert!(parse_reply(b"garbage", Ipv4Addr::LOCALHOST).is_none());
        assert!(parse_reply(br#"{"result":{"mac":"zz"}}"#, Ipv4Addr::LOCALHOST).is_none());
    }
}
