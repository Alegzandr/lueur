//! Nanoleaf panels (Shapes, Canvas, Elements, Lines…) through the local API.
//!
//! Pairing: hold the controller's power button 5–7 s until its LEDs flash,
//! then ask for a token within 30 seconds. The ambilight runs in External
//! Control mode, a UDP stream that sets every panel each frame. The layout
//! plays as a few zones, its panels grouped along its longer side, left to
//! right or bottom to top once turned the way the Nanoleaf app shows it:
//! the ambilight mirrors a slice of the picture on each, and a chase runs
//! across it.

use super::common::{LastSent, REFRESH, Throttle, kelvin_to_rgb, rgb_to_hsv, udp_to};
use super::http::{self, Response};
use super::wire::Wire;
use super::{
    Approach, DeviceKind, DiscoveredDevice, LightStream, NanoleafPairing, REST_BRIGHT, REST_KELVIN,
    group_zones, mdns, zone_output,
};
use crate::color::{BulbOutput, Rgb};
use crate::show::OwnLoop;
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::io;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT: u16 = 16021;
/// What a Nanoleaf controller's device id starts with in Lueur; its own id
/// follows it.
pub const ID_PREFIX: &str = "nanoleaf:";
const STREAM_PORT: u16 = 60222;
const TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(40);
/// How long a command takes to show: a datagram to the controller, and on to the panels.
pub(crate) const LATENCY: Duration = Duration::from_millis(40);

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

fn get(ip: &str, port: u16, token: &str, path: &str) -> io::Result<Response> {
    let path = format!("/api/v1/{}/{path}", safe(token)?);
    check(http::request("GET", ip, port, &path, None, TIMEOUT)?)
}

/// The effect that turns on External Control v2, which the stream sends to.
fn ext_control() -> Value {
    json!({"write": {"command": "display", "animType": "extControl", "extControlVersion": "v2"}})
}

/// Whether an `effects/select` reply names External Control.
pub fn in_ext_control(select: &str) -> bool {
    select.contains("*ExtControl*")
}

pub fn set_power(ip: &str, port: u16, token: &str, on: bool) -> io::Result<()> {
    put(ip, port, token, "state", &json!({"on": {"value": on}}))
}

/// The state body that shows `rgb` on the whole layout at full brightness.
pub fn identify_body(rgb: Rgb) -> Value {
    let (h, sat, _) = rgb_to_hsv(rgb);
    json!({
        "on": {"value": true},
        "hue": {"value": (h.round() as u32) % 360},
        "sat": {"value": (sat * 100.0).round() as u32},
        "brightness": {"value": 100},
    })
}

/// Show `rgb` on the whole layout, so the user can tell which one it is.
/// Over the REST API, which answers, rather than one stream datagram that
/// may come before the controller listens and be lost.
pub fn identify(ip: &str, port: u16, token: &str, rgb: Rgb) -> io::Result<()> {
    put(ip, port, token, "state", &identify_body(rgb))
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
            faster: false,
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

/// Layout pieces that carry no LEDs, by `shapeType`: the Rhythm module, the
/// Shapes controller, the Lines connector, the controller cap and the power
/// connector. They are left out of the zones, which would otherwise have
/// parts that never light.
const UNLIT_SHAPES: [u64; 5] = [1, 12, 16, 19, 20];

/// The lit panels of a `panelLayout/layout` reply, as (id, x, y), y going
/// up. Every piece if none says it is lit, so a layout of shapes unknown
/// here still plays.
fn lit_panels(layout: &Value) -> Vec<(u16, f64, f64)> {
    let pieces: Vec<(u16, f64, f64, bool)> = layout
        .get("positionData")
        .and_then(Value::as_array)
        .map(|panels| {
            panels
                .iter()
                .filter_map(|p| {
                    let id = u16::try_from(p.get("panelId")?.as_u64()?).ok()?;
                    let at = |k| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    let unlit = p
                        .get("shapeType")
                        .and_then(Value::as_u64)
                        .is_some_and(|s| UNLIT_SHAPES.contains(&s));
                    Some((id, at("x"), at("y"), unlit))
                })
                .collect()
        })
        .unwrap_or_default();
    let all_unlit = pieces.iter().all(|&(.., unlit)| unlit);
    pieces
        .into_iter()
        .filter(|&(.., unlit)| all_unlit || !unlit)
        .map(|(id, x, y, _)| (id, x, y))
        .collect()
}

/// Panel ids from a `panelLayout/layout` reply, without the pieces that
/// carry no LEDs.
pub fn panel_ids(layout: &Value) -> Vec<u16> {
    lit_panels(layout).into_iter().map(|(id, ..)| id).collect()
}

/// The angle of a `panelLayout/globalOrientation` reply, in degrees: how
/// far the user turned the layout, anticlockwise, in the Nanoleaf app so
/// that it looks there the way it hangs on the wall. 0 when it says nothing.
pub fn orientation(reply: &Value) -> f64 {
    reply.get("value").and_then(Value::as_f64).unwrap_or(0.0)
}

/// How a layout plays as zones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zoning {
    /// Each panel's zone, in the order [`panel_ids`] lists them.
    pub zone_of: Vec<usize>,
    /// The layout is taller than it is wide, and its zones run from the
    /// bottom up rather than from left to right.
    pub upright: bool,
}

/// Each panel's zone in a `panelLayout/layout` reply, turned by
/// `orientation` degrees (see [`orientation`]) to hang as it does on the
/// wall: the panels grouped along the layout's longer side, left to right
/// or bottom to top, into at most [`super::MAX_ZONES`] zones. Grouping a
/// column of panels by x alone would order them however the controller
/// lists them.
pub fn panel_zones(layout: &Value, orientation: f64) -> Zoning {
    // The app turns the picture anticlockwise; turning the positions back
    // clockwise as much puts them the way they hang, as openHAB draws them.
    let (sin, cos) = (-orientation).to_radians().sin_cos();
    let at: Vec<(f64, f64)> = lit_panels(layout)
        .into_iter()
        .map(|(_, x, y)| (x * cos - y * sin, x * sin + y * cos))
        .collect();
    let extent = |axis: fn(&(f64, f64)) -> f64| {
        let (lo, hi) = at
            .iter()
            .map(axis)
            .fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        hi - lo
    };
    let upright = extent(|p| p.1) > extent(|p| p.0);
    let key = |i: usize| if upright { at[i].1 } else { at[i].0 };
    let mut order: Vec<usize> = (0..at.len()).collect();
    order.sort_by(|&a, &b| key(a).total_cmp(&key(b)));
    let groups = group_zones(at.len());
    let mut zone_of = vec![0; at.len()];
    for (rank, &i) in order.iter().enumerate() {
        zone_of[i] = groups[rank];
    }
    Zoning { zone_of, upright }
}

/// An External Control v2 frame setting every panel to `rgb`.
/// `transition` is in tenths of a second.
pub fn frame(panels: &[u16], rgb: Rgb, transition: u16) -> Vec<u8> {
    zoned_frame(panels, |_| rgb, transition)
}

/// An External Control v2 frame setting panel `i` of `panels` to
/// `colour(i)`.
pub fn zoned_frame(panels: &[u16], colour: impl Fn(usize) -> Rgb, transition: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + panels.len() * 8);
    out.extend_from_slice(&(panels.len() as u16).to_be_bytes());
    for (i, &id) in panels.iter().enumerate() {
        let rgb = colour(i);
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

/// The effect that leaves the panels fading through `own`'s colours by
/// themselves, every panel together, each glide as long as the loop's.
pub fn own_effect(own: &OwnLoop) -> Value {
    let palette: Vec<Value> = own
        .steps
        .iter()
        .map(|s| {
            let (rgb, bright) = match s.output {
                BulbOutput::Color { rgb, bright } => (rgb, bright),
                BulbOutput::White { kelvin, bright } => (kelvin_to_rgb(kelvin), bright),
            };
            let (h, sat, _) = rgb_to_hsv(rgb);
            json!({
                "hue": (h.round() as u32) % 360,
                "saturation": (sat * 100.0).round() as u32,
                "brightness": bright.min(100),
            })
        })
        .collect();
    // Tenths of a second, the same for every glide.
    let glide = (own.lap_ms() / own.steps.len().max(1) as u32)
        .div_ceil(100)
        .max(1);
    json!({"write": {
        "command": "display",
        "animType": "fade",
        "colorType": "HSB",
        "palette": palette,
        "transTime": {"minValue": glide, "maxValue": glide},
        "delayTime": {"minValue": 0, "maxValue": 0},
        "loop": true,
    }})
}

/// How often the keeper looks at what the controller plays.
const KEEP_EVERY: Duration = Duration::from_secs(5);
/// How many of its looks in a row may go unanswered before the controller
/// is taken for gone.
const KEEP_MISSES: u32 = 3;

/// What the stream and its keeper share.
#[derive(Default)]
struct Keeping {
    /// Set when the stream ends: the keeper then leaves the controller be.
    stop: AtomicBool,
    /// Set when the controller stopped answering the keeper.
    gone: AtomicBool,
    /// Held while the keeper speaks to the controller, so a rest never
    /// crosses a late return to External Control.
    speaking: Mutex<()>,
}

/// The keeper: puts the controller back in External Control when it left
/// it (its own app chose an effect, a touch on the panels, a restart after
/// a power cut), since the stream's datagrams are lost on it until then.
fn keep(ip: &str, port: u16, token: &str, keeping: &Keeping) {
    let mut misses = 0;
    loop {
        let wake = Instant::now() + KEEP_EVERY;
        while Instant::now() < wake {
            if keeping.stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let _speaking = keeping.speaking.lock().unwrap_or_else(|p| p.into_inner());
        if keeping.stop.load(Ordering::SeqCst) {
            return;
        }
        match get(ip, port, token, "effects/select") {
            Ok(select) => {
                misses = 0;
                if !in_ext_control(&select.body) {
                    log::debug!(family = "nanoleaf"; "back to External Control");
                    let _ = put(ip, port, token, "effects", &ext_control());
                }
            }
            Err(e) => {
                misses += 1;
                if misses >= KEEP_MISSES {
                    log::debug!(family = "nanoleaf"; "the controller stopped answering: {e}");
                    keeping.gone.store(true, Ordering::SeqCst);
                    return;
                }
            }
        }
    }
}

pub struct NanoleafStream {
    ip: String,
    port: u16,
    token: String,
    panels: Vec<u16>,
    /// Each panel's zone, and how many zones there are.
    zone_of: Vec<usize>,
    zones: usize,
    /// The zones run from the bottom up (see [`Zoning::upright`]).
    upright: bool,
    socket: UdpSocket,
    last: LastSent,
    /// The colours last sent zone by zone, and when.
    last_zones: Option<(Vec<Rgb>, Instant)>,
    pace: Throttle,
    keeping: Arc<Keeping>,
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
        // A layout nobody turned in the app, or a controller that does not
        // say, plays as the controller lists it.
        let turned = get(ip, port, token, "panelLayout/globalOrientation")
            .ok()
            .and_then(|r| serde_json::from_str::<Value>(&r.body).ok())
            .map_or(0.0, |v| orientation(&v));
        let Zoning { zone_of, upright } = panel_zones(&layout, turned);
        let zones = zone_of.iter().max().map_or(1, |z| z + 1);
        // Full brightness: the stream folds the ambilight's own into RGB.
        put(
            ip,
            port,
            token,
            "state",
            &json!({"on": {"value": true}, "brightness": {"value": 100}}),
        )?;
        put(ip, port, token, "effects", &ext_control())?;
        let keeping = Arc::new(Keeping::default());
        {
            let (ip, token, keeping) = (ip.to_string(), token.to_string(), Arc::clone(&keeping));
            std::thread::Builder::new()
                .name("lueur-nanoleaf-keep".into())
                .spawn(move || keep(&ip, port, &token, &keeping))?;
        }
        Ok(NanoleafStream {
            ip: ip.to_string(),
            port,
            token: token.to_string(),
            panels,
            zone_of,
            zones,
            upright,
            socket: udp_to(ip, STREAM_PORT)?,
            last: LastSent::default(),
            last_zones: None,
            pace: Throttle::new(FRAME_INTERVAL),
            keeping,
            wire,
        })
    }

    /// Over UDP a controller that went away is only heard of by its keeper.
    fn still_there(&self) -> io::Result<()> {
        if self.keeping.gone.load(Ordering::SeqCst) {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the controller stopped answering",
            ))
        } else {
            Ok(())
        }
    }

    /// Stop the keeper, waiting for it to finish what it was saying.
    fn quiet_keeper(&self) {
        self.keeping.stop.store(true, Ordering::SeqCst);
        drop(self.keeping.speaking.lock());
    }
}

impl Drop for NanoleafStream {
    fn drop(&mut self) {
        // Not waited for: a stale stream must drop at once.
        self.keeping.stop.store(true, Ordering::SeqCst);
    }
}

impl LightStream for NanoleafStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn latency(&self) -> Duration {
        LATENCY
    }

    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        self.still_there()?;
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
        self.last_zones = None;
        Ok(())
    }

    fn zones(&self) -> usize {
        self.zones
    }

    fn upright(&self) -> bool {
        self.upright
    }

    fn push_zones(
        &mut self,
        outputs: &[BulbOutput],
        effect: Effect,
        duration_ms: u32,
    ) -> io::Result<()> {
        self.still_there()?;
        if outputs.len() <= 1 || self.zones <= 1 {
            return match outputs.first() {
                Some(&o) => self.push(o, effect, duration_ms),
                None => Ok(()),
            };
        }
        let colours: Vec<Rgb> = (0..self.zones)
            .filter_map(|j| zone_output(outputs, j, self.zones).map(panel_rgb))
            .collect();
        let fresh = self
            .last_zones
            .as_ref()
            .is_some_and(|(c, at)| *c == colours && at.elapsed() < REFRESH);
        if fresh || !self.pace.ready() {
            return Ok(());
        }
        let transition = if effect == Effect::Smooth {
            duration_ms.div_ceil(100).min(10) as u16
        } else {
            0
        };
        let zone_of = &self.zone_of;
        let last = colours.len() - 1;
        let frame = zoned_frame(
            &self.panels,
            |i| colours[zone_of.get(i).copied().unwrap_or(0).min(last)],
            transition,
        );
        self.wire.bytes(&frame);
        self.socket.send(&frame)?;
        // The next whole-layout frame must go, whatever it shows.
        self.last = LastSent::default();
        self.last_zones = Some((colours, Instant::now()));
        Ok(())
    }

    fn rest(&mut self) {
        self.quiet_keeper();
        // Setting a temperature leaves External Control on its own.
        let body = json!({"ct": {"value": REST_KELVIN}, "brightness": {"value": REST_BRIGHT, "duration": 1}});
        if Wire::enabled() {
            self.wire.request("PUT", "state", Some(&body.to_string()));
        }
        let _ = put(&self.ip, self.port, &self.token, "state", &body);
    }

    fn rest_looping(&mut self, own: &OwnLoop) {
        self.quiet_keeper();
        let body = own_effect(own);
        if Wire::enabled() {
            self.wire.request("PUT", "effects", Some(&body.to_string()));
        }
        if put(&self.ip, self.port, &self.token, "effects", &body).is_err() {
            self.rest();
        }
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
    fn pieces_without_leds_are_left_out() {
        // A Shapes layout: two hexagons, the controller and a cap.
        let v = json!({"positionData": [
            {"panelId": 10, "x": 0, "shapeType": 7},
            {"panelId": 0, "x": 50, "shapeType": 12},
            {"panelId": 11, "x": 100, "shapeType": 7},
            {"panelId": 12, "x": 150, "shapeType": 19},
        ]});
        assert_eq!(panel_ids(&v), vec![10, 11]);
        assert_eq!(panel_zones(&v, 0.0).zone_of, vec![0, 1]);
        // Nothing said to be lit: everything is kept rather than nothing.
        let odd = json!({"positionData": [{"panelId": 3, "x": 0, "shapeType": 12}]});
        assert_eq!(panel_ids(&odd), vec![3]);
    }

    #[test]
    fn identifying_sets_the_colour_over_rest() {
        let b = identify_body(Rgb::new(0, 0, 255));
        assert_eq!(b["on"]["value"], true);
        assert_eq!(b["hue"]["value"], 240);
        assert_eq!(b["sat"]["value"], 100);
        assert_eq!(b["brightness"]["value"], 100);
    }

    #[test]
    fn external_control_is_recognised() {
        assert!(in_ext_control("\"*ExtControl*\""));
        assert!(!in_ext_control("\"Northern Lights\""));
        assert!(!in_ext_control("\"*Solid*\""));
    }

    #[test]
    fn panels_are_grouped_left_to_right() {
        let v: Value = serde_json::from_str(
            r#"{"positionData":[{"panelId":1,"x":300},{"panelId":2,"x":0},{"panelId":3,"x":150}]}"#,
        )
        .unwrap();
        assert_eq!(panel_zones(&v, 0.0).zone_of, vec![2, 0, 1]);
        let panels: Vec<Value> = (0..20)
            .map(|i| json!({"panelId": i, "x": i * 10}))
            .collect();
        let zones = panel_zones(&json!({ "positionData": panels }), 0.0).zone_of;
        assert_eq!(zones.iter().max(), Some(&(super::super::MAX_ZONES - 1)));
        assert!(zones.windows(2).all(|w| w[0] <= w[1]));
        let f = zoned_frame(&[7, 8], |i| Rgb::new(i as u8, 0, 0), 0);
        assert_eq!((f[4], f[12]), (0, 1));
    }

    #[test]
    fn a_column_of_panels_plays_from_the_bottom_up() {
        // Lines up a wall: the x wanders a little, the y climbs. Listed out
        // of order, as a controller may.
        let v = json!({"positionData": [
            {"panelId": 1, "x": 10, "y": 200},
            {"panelId": 2, "x": 0, "y": 0},
            {"panelId": 3, "x": 20, "y": 100},
        ]});
        let z = panel_zones(&v, 0.0);
        assert!(z.upright);
        assert_eq!(z.zone_of, vec![2, 0, 1]);
        // A row is not upright, whatever its y.
        let row = json!({"positionData": [
            {"panelId": 1, "x": 0, "y": 10},
            {"panelId": 2, "x": 300, "y": 0},
        ]});
        assert!(!panel_zones(&row, 0.0).upright);
    }

    #[test]
    fn the_layout_is_turned_the_way_it_hangs() {
        let v = json!({"positionData": [
            {"panelId": 1, "x": 0, "y": 0},
            {"panelId": 2, "x": 100, "y": 0},
            {"panelId": 3, "x": 200, "y": 0},
        ]});
        // Turned upside down in the app: the right end hangs on the left.
        let z = panel_zones(&v, 180.0);
        assert!(!z.upright);
        assert_eq!(z.zone_of, vec![2, 1, 0]);
        // Turned a quarter anticlockwise in the app, the row stands, its
        // first panel at the top (as openHAB draws it).
        let z = panel_zones(&v, 90.0);
        assert!(z.upright);
        assert_eq!(z.zone_of, vec![2, 1, 0]);
        let z = panel_zones(&v, 270.0);
        assert_eq!((z.upright, z.zone_of), (true, vec![0, 1, 2]));
        assert_eq!(
            orientation(&json!({"value": 88, "max": 360, "min": 0})),
            88.0
        );
        assert_eq!(orientation(&json!({})), 0.0);
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
