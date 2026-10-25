//! Govee LAN API: JSON over UDP, once "LAN Control" is switched on for the
//! device in the Govee Home app.
//!
//! The search is a multicast to port 4001 whose answers come back to port
//! 4002, a fixed port, so only one program on the machine can search at a
//! time. Commands go to the device on port 4003.

use super::common::{LastSent, REFRESH, Throttle, udp_to};
use super::wire::Wire;
use super::{DeviceKind, DiscoveredDevice, LightStream, REST_BRIGHT, REST_KELVIN};
use crate::color::BulbOutput;
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

const SCAN_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SCAN_PORT: u16 = 4001;
const REPLY_PORT: u16 = 4002;
pub const CONTROL_PORT: u16 = 4003;

const MIN_KELVIN: u16 = 2000;
const MAX_KELVIN: u16 = 9000;

/// Govee's LAN firmware drops commands that arrive much faster than this.
const FRAME_INTERVAL: Duration = Duration::from_millis(50);

fn cmd(name: &str, data: Value) -> String {
    json!({"msg": {"cmd": name, "data": data}}).to_string()
}

/// The commands for one output: a colour (or white) and a brightness.
pub fn commands(output: BulbOutput) -> [String; 2] {
    let colour = match output {
        BulbOutput::Color { rgb, .. } => cmd(
            "colorwc",
            json!({"color": {"r": rgb.r, "g": rgb.g, "b": rgb.b}, "colorTemInKelvin": 0}),
        ),
        BulbOutput::White { kelvin, .. } => cmd(
            "colorwc",
            json!({
                "color": {"r": 0, "g": 0, "b": 0},
                "colorTemInKelvin": kelvin.clamp(MIN_KELVIN, MAX_KELVIN),
            }),
        ),
    };
    let bright = cmd(
        "brightness",
        json!({"value": output.bright().clamp(1, 100)}),
    );
    [colour, bright]
}

pub fn set_power(ip: &str, on: bool) -> io::Result<()> {
    let socket = udp_to(ip, CONTROL_PORT)?;
    let msg = cmd("turn", json!({"value": u8::from(on)}));
    for _ in 0..2 {
        socket.send(msg.as_bytes())?;
    }
    Ok(())
}

pub struct GoveeStream {
    socket: UdpSocket,
    last: LastSent,
    /// The brightness last sent, and when.
    last_bright: Option<(u8, Instant)>,
    pace: Throttle,
    wire: Wire,
}

impl GoveeStream {
    pub fn open(ip: &str, wire: Wire) -> io::Result<Self> {
        Ok(GoveeStream {
            socket: udp_to(ip, CONTROL_PORT)?,
            last: LastSent::default(),
            last_bright: None,
            pace: Throttle::new(FRAME_INTERVAL),
            wire,
        })
    }
}

impl LightStream for GoveeStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn push(&mut self, output: BulbOutput, _: Effect, _: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        for msg in frame_commands(&mut self.last_bright, output) {
            self.wire.text(&msg);
            self.socket.send(msg.as_bytes())?;
        }
        self.last.mark(output);
        Ok(())
    }

    fn rest(&mut self) {
        for msg in commands(BulbOutput::White {
            kelvin: REST_KELVIN,
            bright: REST_BRIGHT,
        }) {
            self.wire.text(&msg);
            let _ = self.socket.send(msg.as_bytes());
        }
    }
}

/// The commands that take the bulb to `output`, given the brightness it was
/// last sent and when (updated here). Brightness is its own command, skipped
/// when only the hue moved, but sent again with the colour once it has gone
/// [`REFRESH`] unsent: a bulb cut at the wall comes back at full brightness.
fn frame_commands(last_bright: &mut Option<(u8, Instant)>, output: BulbOutput) -> Vec<String> {
    let [colour, bright] = commands(output);
    let b = output.bright();
    if last_bright.is_some_and(|(prev, at)| prev == b && at.elapsed() < REFRESH) {
        return vec![colour];
    }
    *last_bright = Some((b, Instant::now()));
    vec![colour, bright]
}

pub fn discover(timeout: Duration) -> io::Result<Vec<DiscoveredDevice>> {
    let listener = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, REPLY_PORT))?;
    listener.set_read_timeout(Some(Duration::from_millis(200)))?;
    let sender = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    let scan = cmd("scan", json!({"account_topic": "reserve"}));
    for _ in 0..2 {
        let _ = sender.send_to(scan.as_bytes(), SocketAddrV4::new(SCAN_ADDR, SCAN_PORT));
    }

    let mut found: HashMap<String, (DiscoveredDevice, bool)> = HashMap::new();
    let deadline = Instant::now() + timeout;
    let mut buf = [0u8; 2048];
    while Instant::now() < deadline {
        match listener.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(d) = parse_scan(&buf[..n]) {
                    let own = d.ip == from.ip().to_string();
                    super::common::keep(&mut found, d.id.clone(), d, own);
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
            }
            Err(e) => return Err(e),
        }
    }
    Ok(found.into_values().map(|(d, _)| d).collect())
}

/// A scan reply, if it is one.
pub fn parse_scan(raw: &[u8]) -> Option<DiscoveredDevice> {
    let v: Value = serde_json::from_slice(raw).ok()?;
    let msg = v.get("msg")?;
    if msg.get("cmd")?.as_str()? != "scan" {
        return None;
    }
    let data = msg.get("data")?;
    let ip = data.get("ip")?.as_str()?;
    ip.parse::<Ipv4Addr>().ok()?;
    let device = data.get("device")?.as_str()?;
    let id: String = device
        .chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_ascii_lowercase();
    if id.is_empty() {
        return None;
    }
    let text = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_string);
    Some(DiscoveredDevice {
        id: format!("govee:{id}"),
        ip: ip.to_string(),
        port: CONTROL_PORT,
        model: text("sku").unwrap_or_default(),
        name: String::new(),
        support: Vec::new(),
        fw_ver: text("wifiVersionSoft"),
        kind: DeviceKind::Govee,
        paired: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    #[test]
    fn colour_and_brightness_commands() {
        let [c, b] = commands(BulbOutput::Color {
            rgb: Rgb::new(255, 0, 9),
            bright: 42,
        });
        let c: Value = serde_json::from_str(&c).unwrap();
        assert_eq!(c["msg"]["cmd"], "colorwc");
        assert_eq!(c["msg"]["data"]["color"]["b"], 9);
        assert_eq!(c["msg"]["data"]["colorTemInKelvin"], 0);
        let b: Value = serde_json::from_str(&b).unwrap();
        assert_eq!(b["msg"]["data"]["value"], 42);
    }

    #[test]
    fn brightness_is_sent_when_it_moves_and_with_each_refresh() {
        let red = |bright| BulbOutput::Color {
            rgb: Rgb::new(255, 0, 0),
            bright,
        };
        let bright_cmd = |b| commands(red(b))[1].clone();
        let mut last = None;
        assert_eq!(frame_commands(&mut last, red(40)), commands(red(40)));
        // Only the hue moved: the colour alone.
        let blue = BulbOutput::Color {
            rgb: Rgb::new(0, 0, 255),
            bright: 40,
        };
        assert_eq!(frame_commands(&mut last, blue), [commands(blue)[0].clone()]);
        assert_eq!(frame_commands(&mut last, red(60))[1], bright_cmd(60));
        // A refresh, long after the last brightness: both again, so a bulb
        // that came back at 100% returns to 60.
        last = Some((60, Instant::now() - REFRESH));
        let sent = frame_commands(&mut last, red(60));
        assert_eq!(sent, commands(red(60)));
        assert!(sent[1].contains(r#""value":60"#), "{}", sent[1]);
    }

    #[test]
    fn white_uses_colour_temperature() {
        let [c, _] = commands(BulbOutput::White {
            kelvin: 12000,
            bright: 10,
        });
        let c: Value = serde_json::from_str(&c).unwrap();
        assert_eq!(c["msg"]["data"]["colorTemInKelvin"], 9000);
    }

    #[test]
    fn parses_a_scan_reply() {
        let raw = br#"{"msg":{"cmd":"scan","data":{"ip":"192.168.1.23","device":"1F:80:C5:32:32:36:72:4E","sku":"H618E","bleVersionHard":"3.01.01","bleVersionSoft":"1.03.01","wifiVersionHard":"1.00.10","wifiVersionSoft":"1.02.03"}}}"#;
        let d = parse_scan(raw).unwrap();
        assert_eq!(d.id, "govee:1f80c5323236724e");
        assert_eq!(d.ip, "192.168.1.23");
        assert_eq!(d.model, "H618E");
        assert_eq!(d.port, CONTROL_PORT);
    }

    #[test]
    fn ignores_other_messages() {
        assert!(parse_scan(br#"{"msg":{"cmd":"devStatus","data":{}}}"#).is_none());
        assert!(
            parse_scan(br#"{"msg":{"cmd":"scan","data":{"ip":"nope","device":"aa"}}}"#).is_none()
        );
    }
}
