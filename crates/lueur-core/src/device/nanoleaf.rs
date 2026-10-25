//! Nanoleaf panels (Shapes, Canvas, Elements, Lines…) through the local API.
//!
//! Pairing: hold the controller's power button 5–7 s until its LEDs flash,
//! then ask for a token within 30 seconds. The ambilight runs in External
//! Control mode, a UDP stream that sets every panel each frame; the whole
//! layout mirrors one colour, since the room plan treats it as one light.

use super::common::{LastSent, Throttle, kelvin_to_rgb, udp_to};
use super::http::{self, Response};
use super::wire::Wire;
use super::{
    Approach, DeviceKind, DiscoveredDevice, LightStream, NanoleafPairing, REST_BRIGHT, REST_KELVIN,
    mdns,
};
use crate::color::{BulbOutput, Rgb};
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::io;
use std::net::UdpSocket;
use std::time::Duration;

pub const PORT: u16 = 16021;
/// What a Nanoleaf controller's device id starts with in Lueur; its own id
/// follows it.
pub const ID_PREFIX: &str = "nanoleaf:";
const STREAM_PORT: u16 = 60222;
const TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(40);

fn check(resp: Response) -> io::Result<Response> {
    if resp.ok() {
        Ok(resp)
    } else {
        Err(io::Error::other(format!(
            "controller answered {}",
            resp.status
        )))
    }
}

/// Tokens are what the controller issued: alphanumerics only.
fn safe(token: &str) -> io::Result<&str> {
    if !token.is_empty() && token.bytes().all(|b| b.is_ascii_alphanumeric()) {
        Ok(token)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bad Nanoleaf token",
        ))
    }
}

fn put(ip: &str, port: u16, token: &str, path: &str, body: &Value) -> io::Result<()> {
    let path = format!("/api/v1/{}/{path}", safe(token)?);
    check(http::request(
        "PUT",
        ip,
        port,
        &path,
        Some(&body.to_string()),
        TIMEOUT,
    )?)
    .map(|_| ())
}

pub fn set_power(ip: &str, port: u16, token: &str, on: bool) -> io::Result<()> {
    put(ip, port, token, "state", &json!({"on": {"value": on}}))
}

/// Ask for a token. Fails with [`io::ErrorKind::WouldBlock`] until the
/// controller is in pairing mode.
pub fn pair(ip: &str, port: u16, id: &str) -> io::Result<NanoleafPairing> {
    let resp = http::request("POST", ip, port, "/api/v1/new", Some(""), TIMEOUT)?;
    if resp.status == 403 || resp.status == 401 {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "hold the power button until the lights flash",
        ));
    }
    let v: Value = serde_json::from_str(&check(resp)?.body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let token = v
        .get("auth_token")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no token in reply"))?;
    safe(token)?;
    Ok(NanoleafPairing {
        id: id.to_string(),
        ip: ip.to_string(),
        token: token.to_string(),
    })
}

/// Normalise the controller id from TXT (`AA:BB:…`) to bare lower-case hex.
fn norm_id(raw: &str) -> String {
    raw.chars()
        .filter(char::is_ascii_hexdigit)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Find controllers, with the token of those Lueur is paired with.
///
/// The API is HTTP only and anything can claim a controller's id, so the
/// token is only ever handed out for the address it was granted at. A
/// paired controller heard elsewhere is listed unpaired: whether it moved
/// is the user's call (or a new pairing's), never discovery's.
pub fn discover(timeout: Duration, known: &[NanoleafPairing]) -> io::Result<Vec<DiscoveredDevice>> {
    let heard: Vec<(String, mdns::Instance)> = mdns::browse("_nanoleafapi._tcp.local", timeout)?
        .into_iter()
        .filter_map(|inst| {
            let raw_id = inst.txt.get("id").map(String::as_str).unwrap_or(&inst.ip);
            let id = norm_id(raw_id);
            (!id.is_empty()).then_some((id, inst))
        })
        .collect();
    let mut out = Vec::new();
    for (id, inst) in &heard {
        let paired = known.iter().find(|k| k.id == *id);
        let home_answers = || {
            paired.is_some_and(|k| {
                heard.iter().any(|(other, at)| other == id && at.ip == k.ip) || answers(&k.ip)
            })
        };
        let token = match paired {
            Some(k) => match super::approach(&inst.ip, Some((&k.ip, false)), home_answers()) {
                Approach::Home => k.token.clone(),
                Approach::Ignore => continue,
                _ => String::new(),
            },
            None => String::new(),
        };
        let inst = inst.clone();
        out.push(DiscoveredDevice {
            id: format!("{ID_PREFIX}{id}"),
            ip: inst.ip,
            port: if inst.port == 0 { PORT } else { inst.port },
            model: inst.txt.get("md").cloned().unwrap_or_default(),
            name: inst.name,
            support: Vec::new(),
            fw_ver: inst.txt.get("srcvers").cloned(),
            paired: !token.is_empty(),
            kind: DeviceKind::Nanoleaf { token },
        });
    }
    Ok(out)
}

/// Whether a controller's API still listens at `ip`.
fn answers(ip: &str) -> bool {
    super::common::ipv4(ip).is_ok_and(|ip| {
        let addr = std::net::SocketAddr::from((ip, PORT));
        std::net::TcpStream::connect_timeout(&addr, TIMEOUT).is_ok()
    })
}

/// Panel ids from a `panelLayout/layout` reply. Controllers and connectors
/// carry no LEDs, but listing them is harmless: the controller ignores them.
pub fn panel_ids(layout: &Value) -> Vec<u16> {
    layout
        .get("positionData")
        .and_then(Value::as_array)
        .map(|panels| {
            panels
                .iter()
                .filter_map(|p| p.get("panelId")?.as_u64())
                .filter_map(|id| u16::try_from(id).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// An External Control v2 frame setting every panel to `rgb`.
/// `transition` is in tenths of a second.
pub fn frame(panels: &[u16], rgb: Rgb, transition: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + panels.len() * 8);
    out.extend_from_slice(&(panels.len() as u16).to_be_bytes());
    for &id in panels {
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&[rgb.r, rgb.g, rgb.b, 0]);
        out.extend_from_slice(&transition.to_be_bytes());
    }
    out
}

/// The colour a panel should show for an output: panels take plain RGB, so
/// brightness is folded in and white is approximated.
pub fn panel_rgb(output: BulbOutput) -> Rgb {
    let (rgb, bright) = match output {
        BulbOutput::Color { rgb, bright } => (rgb, bright),
        BulbOutput::White { kelvin, bright } => (kelvin_to_rgb(kelvin), bright),
    };
    let k = f32::from(bright.min(100)) / 100.0;
    let s = |v: u8| (f32::from(v) * k).round() as u8;
    Rgb::new(s(rgb.r), s(rgb.g), s(rgb.b))
}

pub struct NanoleafStream {
    ip: String,
    port: u16,
    token: String,
    panels: Vec<u16>,
    socket: UdpSocket,
    last: LastSent,
    pace: Throttle,
    wire: Wire,
}

impl NanoleafStream {
    pub fn open(ip: &str, port: u16, token: &str, wire: Wire) -> io::Result<Self> {
        let path = format!("/api/v1/{}/panelLayout/layout", safe(token)?);
        let layout = check(http::request("GET", ip, port, &path, None, TIMEOUT)?)?;
        let layout: Value = serde_json::from_str(&layout.body)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let panels = panel_ids(&layout);
        if panels.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "no panels"));
        }
        // Full brightness: the stream folds the ambilight's own into RGB.
        put(
            ip,
            port,
            token,
            "state",
            &json!({"on": {"value": true}, "brightness": {"value": 100}}),
        )?;
        put(
            ip,
            port,
            token,
            "effects",
            &json!({"write": {"command": "display", "animType": "extControl", "extControlVersion": "v2"}}),
        )?;
        Ok(NanoleafStream {
            ip: ip.to_string(),
            port,
            token: token.to_string(),
            panels,
            socket: udp_to(ip, STREAM_PORT)?,
            last: LastSent::default(),
            pace: Throttle::new(FRAME_INTERVAL),
            wire,
        })
    }
}

impl LightStream for NanoleafStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        let transition = if effect == Effect::Smooth {
            duration_ms.div_ceil(100).min(10) as u16
        } else {
            0
        };
        let frame = frame(&self.panels, panel_rgb(output), transition);
        self.wire.bytes(&frame);
        self.socket.send(&frame)?;
        self.last.mark(output);
        Ok(())
    }

    fn rest(&mut self) {
        // Setting a temperature leaves External Control on its own.
        let body = json!({"ct": {"value": REST_KELVIN}, "brightness": {"value": REST_BRIGHT, "duration": 1}});
        if Wire::enabled() {
            self.wire.request("PUT", "state", Some(&body.to_string()));
        }
        let _ = put(&self.ip, self.port, &self.token, "state", &body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_the_v2_layout() {
        let f = frame(&[0x1234, 7], Rgb::new(1, 2, 3), 1);
        assert_eq!(&f[..2], &[0, 2], "panel count");
        assert_eq!(&f[2..10], &[0x12, 0x34, 1, 2, 3, 0, 0, 1]);
        assert_eq!(&f[10..12], &[0, 7]);
        assert_eq!(f.len(), 18);
    }

    #[test]
    fn layout_yields_panel_ids() {
        let v: Value = serde_json::from_str(
            r#"{"numPanels":2,"sideLength":150,"positionData":[{"panelId":107,"x":0,"y":0,"o":0,"shapeType":7},{"panelId":5432,"x":1,"y":0,"o":0,"shapeType":7}]}"#,
        )
        .unwrap();
        assert_eq!(panel_ids(&v), vec![107, 5432]);
        assert!(panel_ids(&json!({})).is_empty());
    }

    #[test]
    fn brightness_is_folded_into_rgb() {
        let c = panel_rgb(BulbOutput::Color {
            rgb: Rgb::new(255, 100, 0),
            bright: 50,
        });
        assert_eq!(c, Rgb::new(128, 50, 0));
        let w = panel_rgb(BulbOutput::White {
            kelvin: 2700,
            bright: 100,
        });
        assert_eq!(w.r, 255);
        assert!(w.b < w.r);
    }

    #[test]
    fn ids_and_tokens_are_normalised_and_checked() {
        assert_eq!(norm_id("AB:CD:01:23:45:67"), "abcd01234567");
        assert!(safe("aBc123").is_ok());
        assert!(safe("x/../y").is_err());
        assert!(safe("").is_err());
    }
}
