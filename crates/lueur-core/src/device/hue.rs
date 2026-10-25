//! Philips Hue through the bridge's local REST API.
//!
//! Pairing is the bridge's own ritual: press its round button, then ask for a
//! username within 30 seconds. After that each lamp is a REST resource.
//!
//! A bridge accepts about ten light commands a second in total, so the lamps
//! of one bridge share that budget. That is enough for a calm ambilight; the
//! Entertainment API (DTLS streaming at 25 Hz) is the way past it.

use super::common::{Driver, LastSent, Mailbox, Throttle};
use super::http::{self, Response};
use super::{DeviceKind, DiscoveredDevice, HueBridge, LightStream, REST_BRIGHT, REST_KELVIN, mdns};
use crate::color::{BulbOutput, Rgb};
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

pub const PORT: u16 = 80;
/// Lamp ids are `hue:<bridge>:<light>`, an unpaired bridge's `hue:<bridge>`.
pub const ID_PREFIX: &str = "hue:";
const TIMEOUT: Duration = Duration::from_secs(2);

/// The bridge's whole budget, shared by its lamps.
const BRIDGE_INTERVAL: Duration = Duration::from_millis(100);

/// CIE xy of an sRGB colour, through the wide gamut Hue documents for its
/// colour lamps. The bridge maps it into each lamp's own gamut.
pub fn rgb_to_xy(c: Rgb) -> [f64; 2] {
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v > 0.04045 {
            ((v + 0.055) / 1.055).powf(2.4)
        } else {
            v / 12.92
        }
    };
    let (r, g, b) = (lin(c.r), lin(c.g), lin(c.b));
    let x = r * 0.649_926 + g * 0.103_455 + b * 0.197_109;
    let y = r * 0.234_327 + g * 0.743_075 + b * 0.022_598;
    let z = g * 0.053_077 + b * 1.035_763;
    let sum = x + y + z;
    if sum <= 0.0 {
        // Black has no chromaticity; the D65 white point is a safe neutral.
        return [0.3127, 0.3290];
    }
    let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
    [round(x / sum), round(y / sum)]
}

/// Hue brightness (1..=254) for a 1..=100 percentage.
fn bri(bright: u8) -> u32 {
    super::common::scale_bright(bright, 1, 254)
}

/// Colour temperature in mireds, within what Hue lamps accept.
fn mired(kelvin: u16) -> u32 {
    (1_000_000 / u32::from(kelvin.max(1))).clamp(153, 500)
}

/// The state body for one output.
pub fn state_body(output: BulbOutput, duration_ms: u32, with_on: bool) -> Value {
    let mut body = match output {
        BulbOutput::Color { rgb, bright } => json!({"xy": rgb_to_xy(rgb), "bri": bri(bright)}),
        BulbOutput::White { kelvin, bright } => json!({"ct": mired(kelvin), "bri": bri(bright)}),
    };
    // Transition time is in tenths of a second.
    body["transitiontime"] = json!(duration_ms.div_ceil(100).min(10));
    if with_on {
        // Only on the first frame: the bridge handles `on` slowly.
        body["on"] = json!(true);
    }
    body
}

fn check(resp: Response) -> io::Result<Value> {
    if !resp.ok() {
        return Err(io::Error::other(format!("bridge answered {}", resp.status)));
    }
    serde_json::from_str(&resp.body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// The first `error` in a bridge reply, as (type, description).
fn bridge_error(v: &Value) -> Option<(i64, String)> {
    let e = v.as_array()?.iter().find_map(|x| x.get("error"))?;
    Some((
        e.get("type")?.as_i64()?,
        e.get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    ))
}

fn state_path(username: &str, light: &str) -> io::Result<String> {
    Ok(format!(
        "/api/{}/lights/{}/state",
        safe(username)?,
        safe(light)?
    ))
}

fn put_state(ip: &str, username: &str, light: &str, body: &Value) -> io::Result<()> {
    let path = state_path(username, light)?;
    check(http::request(
        "PUT",
        ip,
        PORT,
        &path,
        Some(&body.to_string()),
        TIMEOUT,
    )?)
    .map(|_| ())
}

/// Path segments come from config; keep them to what the bridge issues.
fn safe(segment: &str) -> io::Result<&str> {
    if !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Ok(segment)
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "bad Hue id"))
    }
}

pub fn set_power(ip: &str, username: &str, light: &str, on: bool) -> io::Result<()> {
    put_state(ip, username, light, &json!({"on": on}))
}

/// Ask the bridge at `ip` for a username. Fails with
/// [`io::ErrorKind::WouldBlock`] until its link button has been pressed.
pub fn pair(ip: &str) -> io::Result<HueBridge> {
    let id = bridge_id(ip)?;
    let body = json!({"devicetype": "lueur#desktop"}).to_string();
    let v = check(http::request(
        "POST",
        ip,
        PORT,
        "/api",
        Some(&body),
        TIMEOUT,
    )?)?;
    if let Some((kind, desc)) = bridge_error(&v) {
        return Err(if kind == 101 {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "press the link button on the bridge",
            )
        } else {
            io::Error::other(desc)
        });
    }
    let username = v
        .as_array()
        .and_then(|a| a.iter().find_map(|x| x.pointer("/success/username")))
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no username in reply"))?;
    Ok(HueBridge {
        id,
        ip: ip.to_string(),
        username: username.to_string(),
    })
}

/// The bridge's id from its public config (no username needed).
fn bridge_id(ip: &str) -> io::Result<String> {
    let v = check(http::request(
        "GET",
        ip,
        PORT,
        "/api/config",
        None,
        TIMEOUT,
    )?)?;
    v.get("bridgeid")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a Hue bridge"))
}

/// Find bridges, then list the lamps of those Lueur is paired with.
pub fn discover(timeout: Duration, known: &[HueBridge]) -> io::Result<Vec<DiscoveredDevice>> {
    // Bridge ip → id from mDNS, plus every paired bridge at its last address
    // in case mDNS is filtered on this network.
    let mut bridges: HashMap<String, Option<String>> = HashMap::new();
    for inst in mdns::browse("_hue._tcp.local", timeout)? {
        let id = inst.txt.get("bridgeid").map(|s| s.to_ascii_lowercase());
        bridges.insert(inst.ip, id);
    }
    for b in known {
        bridges.entry(b.ip.clone()).or_insert(None);
    }

    let mut out = Vec::new();
    for (ip, id) in bridges {
        let id = match id {
            Some(id) => id,
            None => match bridge_id(&ip) {
                Ok(id) => id,
                Err(_) => continue,
            },
        };
        let paired = known.iter().find(|b| b.id == id);
        match paired.map(|b| lights(&ip, &id, &b.username)) {
            Some(Ok(lamps)) => out.extend(lamps),
            // Unpaired, or the bridge forgot us (factory reset): offer pairing.
            _ => out.push(DiscoveredDevice {
                id: format!("{ID_PREFIX}{id}"),
                ip,
                port: PORT,
                model: "bridge".into(),
                name: "Hue Bridge".into(),
                support: Vec::new(),
                fw_ver: None,
                kind: DeviceKind::Hue {
                    username: String::new(),
                    light: String::new(),
                },
                paired: false,
            }),
        }
    }
    Ok(out)
}

fn lights(ip: &str, bridge: &str, username: &str) -> io::Result<Vec<DiscoveredDevice>> {
    let path = format!("/api/{}/lights", safe(username)?);
    let v = check(http::request("GET", ip, PORT, &path, None, TIMEOUT)?)?;
    if bridge_error(&v).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unauthorized",
        ));
    }
    Ok(parse_lights(&v, ip, bridge, username))
}

/// The dimmable lamps in a `/lights` listing.
pub fn parse_lights(v: &Value, ip: &str, bridge: &str, username: &str) -> Vec<DiscoveredDevice> {
    let Some(map) = v.as_object() else {
        return Vec::new();
    };
    let mut out: Vec<DiscoveredDevice> = map
        .iter()
        .filter(|(_, l)| l.pointer("/state/bri").is_some())
        .filter(|(num, _)| safe(num).is_ok())
        .map(|(num, l)| {
            let text = |k: &str| {
                l.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            DiscoveredDevice {
                id: format!("{ID_PREFIX}{bridge}:{num}"),
                ip: ip.to_string(),
                port: PORT,
                model: text("type"),
                name: text("name"),
                support: Vec::new(),
                fw_ver: l
                    .get("swversion")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                kind: DeviceKind::Hue {
                    username: username.to_string(),
                    light: num.clone(),
                },
                paired: true,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

struct HueDriver {
    /// Kept open between frames: a fresh connection per command costs a
    /// handshake every time.
    client: http::Client,
    path: String,
    turned_on: bool,
}

impl HueDriver {
    fn put(&mut self, body: &Value) -> io::Result<()> {
        check(
            self.client
                .request("PUT", &self.path, Some(&body.to_string()))?,
        )
        .map(|_| ())
    }
}

impl Driver for HueDriver {
    fn send(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        let ms = if effect == Effect::Smooth {
            duration_ms
        } else {
            0
        };
        self.put(&state_body(output, ms, !self.turned_on))?;
        self.turned_on = true;
        Ok(())
    }

    fn rest(&mut self) {
        let rest = BulbOutput::White {
            kelvin: REST_KELVIN,
            bright: REST_BRIGHT,
        };
        let _ = self.put(&state_body(rest, 400, true));
    }
}

pub struct HueStream {
    mailbox: Mailbox,
    last: LastSent,
    pace: Throttle,
}

impl HueStream {
    /// `siblings` lamps of the same bridge share its command budget.
    pub fn open(ip: &str, username: &str, light: &str, siblings: usize) -> io::Result<Self> {
        let driver = HueDriver {
            client: http::Client::new(ip, PORT, TIMEOUT)?,
            path: state_path(username, light)?,
            turned_on: false,
        };
        Ok(HueStream {
            mailbox: Mailbox::spawn("hue", driver)?,
            last: LastSent::default(),
            pace: Throttle::new(BRIDGE_INTERVAL * siblings as u32),
        })
    }
}

impl LightStream for HueStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        let ms = self.pace.glide(effect, duration_ms);
        self.mailbox.push(output, effect, ms)?;
        self.last.mark(output);
        Ok(())
    }

    fn rest(&mut self) {
        self.mailbox.rest();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primaries_land_in_the_right_corners() {
        let [rx, ry] = rgb_to_xy(Rgb::new(255, 0, 0));
        assert!(rx > 0.6 && ry < 0.35, "red {rx},{ry}");
        let [gx, gy] = rgb_to_xy(Rgb::new(0, 255, 0));
        assert!(gy > 0.7 && gx < 0.2, "green {gx},{gy}");
        let [bx, by] = rgb_to_xy(Rgb::new(0, 0, 255));
        assert!(bx < 0.2 && by < 0.1, "blue {bx},{by}");
        assert_eq!(rgb_to_xy(Rgb::BLACK), [0.3127, 0.3290]);
    }

    #[test]
    fn colour_body_has_xy_and_scaled_brightness() {
        let b = state_body(
            BulbOutput::Color {
                rgb: Rgb::new(255, 0, 0),
                bright: 100,
            },
            80,
            false,
        );
        assert_eq!(b["bri"], 254);
        assert_eq!(b["transitiontime"], 1);
        assert!(b.get("on").is_none());
        assert!(b["xy"].is_array());
    }

    #[test]
    fn white_body_uses_mireds_and_turns_on_when_asked() {
        let b = state_body(
            BulbOutput::White {
                kelvin: 2700,
                bright: 1,
            },
            0,
            true,
        );
        assert_eq!(b["ct"], 370);
        assert_eq!(b["bri"], 1);
        assert_eq!(b["transitiontime"], 0);
        assert_eq!(b["on"], true);
    }

    #[test]
    fn link_button_error_is_recognised() {
        let v: Value = serde_json::from_str(
            r#"[{"error":{"type":101,"address":"","description":"link button not pressed"}}]"#,
        )
        .unwrap();
        assert_eq!(bridge_error(&v).unwrap().0, 101);
        let ok: Value = serde_json::from_str(r#"[{"success":{"username":"abc"}}]"#).unwrap();
        assert!(bridge_error(&ok).is_none());
    }

    #[test]
    fn lists_only_dimmable_lamps() {
        let v: Value = serde_json::from_str(
            r#"{
                "1": {"name": "Desk", "type": "Extended color light", "state": {"on": true, "bri": 100, "xy": [0.3, 0.3]}},
                "2": {"name": "Plug", "type": "On/Off plug-in unit", "state": {"on": false}},
                "3": {"name": "Hall", "type": "Color temperature light", "state": {"on": true, "bri": 10, "ct": 300}}
            }"#,
        )
        .unwrap();
        let lamps = parse_lights(&v, "192.168.1.30", "001788fffe1a2b3c", "user");
        assert_eq!(lamps.len(), 2);
        assert_eq!(lamps[0].id, "hue:001788fffe1a2b3c:1");
        assert_eq!(lamps[0].name, "Desk");
        assert_eq!(
            lamps[1].kind,
            DeviceKind::Hue {
                username: "user".into(),
                light: "3".into()
            }
        );
    }

    #[test]
    fn path_segments_are_restricted() {
        assert!(safe("abcDEF-123").is_ok());
        assert!(safe("../config").is_err());
        assert!(safe("").is_err());
        assert!(safe("a/b").is_err());
    }
}
