//! Philips Hue through the bridge's local REST API.
//!
//! Pairing is the bridge's own ritual: press its round button, then ask for a
//! username within 30 seconds. After that each lamp is a REST resource.
//!
//! A bridge is only ever spoken to over TLS, to the certificate it showed
//! the first time Lueur saw it (at pairing, or for a pairing older than
//! that, at its paired address): trust on first use, since nothing vouches
//! for a bridge's certificate. The username is then never sent in the
//! clear, and a host that claims the bridge's id without that
//! certificate's key gets nothing. Only the first, round bridge, which has
//! no HTTPS, is paired in the clear; its username goes only to the address
//! it was paired at. See [`Approach`] and [`pairing_cert`].
//!
//! A bridge accepts about ten light commands a second in total, so the lamps
//! of one bridge share that budget. That is enough for a calm ambilight. A
//! bridge paired with its streaming key streams to its lamps instead (see
//! [`super::entertainment`]), and falls back to this when it cannot.

use super::common::{Driver, LastSent, Mailbox, Throttle};
use super::entertainment::{self, Access, Seat};
use super::http::{self, Response, Trust};
use super::wire::Wire;
use super::{
    Approach, DeviceKind, DiscoveredDevice, HueBridge, LightStream, REST_BRIGHT, REST_KELVIN, mdns,
};
use crate::color::{BulbOutput, Rgb};
use crate::show::{OwnLoop, ShowKind};
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

pub const PORT: u16 = 80;
/// Where a bridge serves the same API over HTTPS.
pub const TLS_PORT: u16 = 443;
/// Lamp ids are `hue:<bridge>:<light>`, an unpaired bridge's `hue:<bridge>`.
pub const ID_PREFIX: &str = "hue:";
const TIMEOUT: Duration = Duration::from_secs(2);

/// The bridge's whole budget, shared by its lamps.
pub(crate) const BRIDGE_INTERVAL: Duration = Duration::from_millis(100);
/// How long a command over REST takes to show: the bridge, then Zigbee.
pub(crate) const LATENCY: Duration = Duration::from_millis(120);

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
        // Only on the first frame: the bridge handles `on` slowly. A colour
        // loop left playing when Lueur quit would cover every frame.
        body["on"] = json!(true);
        body["effect"] = json!("none");
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

/// A certificate pin as [`HueBridge::cert`] stores it.
pub fn pin_hex(pin: &[u8; 32]) -> String {
    pin.iter().map(|b| format!("{b:02x}")).collect()
}

/// The pin [`pin_hex`] wrote, if `hex` is one.
pub fn parse_pin(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks(2).enumerate() {
        let pair = std::str::from_utf8(pair).ok()?;
        out[i] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// How to reach a bridge whose pinned certificate is `cert`: over TLS to
/// that certificate, or in the clear when none is pinned. A pin that does
/// not parse reaches nothing rather than falling back to the clear.
fn link(cert: &str) -> io::Result<(Trust, u16)> {
    if cert.is_empty() {
        return Ok((Trust::Clear, PORT));
    }
    parse_pin(cert)
        .map(|pin| (Trust::Pinned(pin), TLS_PORT))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad Hue certificate pin"))
}

fn call(
    ip: &str,
    cert: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> io::Result<Response> {
    let (trust, port) = link(cert)?;
    http::request_with(trust, method, ip, port, path, body, TIMEOUT)
}

fn state_path(username: &str, light: &str) -> io::Result<String> {
    Ok(format!(
        "/api/{}/lights/{}/state",
        safe(username)?,
        safe(light)?
    ))
}

fn put_state(ip: &str, username: &str, light: &str, cert: &str, body: &Value) -> io::Result<()> {
    let path = state_path(username, light)?;
    check(call(ip, cert, "PUT", &path, Some(&body.to_string()))?).map(|_| ())
}

/// Path segments come from config; keep them to what the bridge issues.
pub(crate) fn safe(segment: &str) -> io::Result<&str> {
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

pub fn set_power(ip: &str, username: &str, light: &str, cert: &str, on: bool) -> io::Result<()> {
    put_state(ip, username, light, cert, &json!({"on": on}))
}

/// The model of the first, round bridge: the only one without HTTPS.
pub const ROUND_BRIDGE: &str = "BSB001";

/// Why a pairing was refused before anything was asked of the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairRefusal {
    /// A bridge that should serve HTTPS did not: its username would have
    /// been asked for in the clear.
    NoTls,
    /// Lueur pinned another certificate for this bridge id.
    CertChanged,
}

impl PairRefusal {
    /// The error code the interface words it with.
    pub fn code(self) -> &'static str {
        match self {
            PairRefusal::NoTls => "pair_no_tls",
            PairRefusal::CertChanged => "pair_cert_changed",
        }
    }
}

impl std::fmt::Display for PairRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PairRefusal::NoTls => "the bridge did not answer over HTTPS",
            PairRefusal::CertChanged => "the bridge shows another certificate than the pinned one",
        })
    }
}

impl std::error::Error for PairRefusal {}

/// The [`PairRefusal`] `e` carries, if it is one.
pub fn pair_refusal(e: &io::Error) -> Option<PairRefusal> {
    e.get_ref()?.downcast_ref::<PairRefusal>().copied()
}

/// The certificate a pairing goes on with: the one the bridge showed
/// (`learned`), which must be the `pinned` one if this id already has one.
/// Only the round bridge ([`ROUND_BRIDGE`]) may be paired in the clear.
pub fn pairing_cert(
    learned: Option<String>,
    pinned: Option<&str>,
    model: Option<&str>,
) -> Result<String, PairRefusal> {
    match (learned, pinned) {
        (Some(cert), Some(pin)) if cert != pin => Err(PairRefusal::CertChanged),
        (Some(cert), _) => Ok(cert),
        (None, Some(_)) => Err(PairRefusal::NoTls),
        (None, None) if model == Some(ROUND_BRIDGE) => Ok(String::new()),
        (None, None) => Err(PairRefusal::NoTls),
    }
}

/// Ask the bridge at `ip` for a username. Fails with
/// [`io::ErrorKind::WouldBlock`] until its link button has been pressed.
///
/// The bridge's certificate is pinned first, so the username is asked for,
/// and later sent, over TLS only; see [`pairing_cert`] for when that is
/// refused. `known` are the bridges already paired.
pub fn pair(ip: &str, known: &[HueBridge]) -> io::Result<HueBridge> {
    let config = public_config(ip)?;
    let id = config_id(&config)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a Hue bridge"))?;
    let model = config.get("modelid").and_then(Value::as_str);
    let pinned = known
        .iter()
        .find(|b| b.id == id && !b.cert.is_empty())
        .map(|b| b.cert.as_str());
    let cert = pairing_cert(learn_cert(ip, &id), pinned, model)
        .map_err(|r| io::Error::new(io::ErrorKind::PermissionDenied, r))?;
    // The streaming key comes with the username or never: only HTTPS
    // bridges stream, and over TLS it is as safe as the username.
    let body = if cert.is_empty() {
        json!({"devicetype": "lueur#desktop"})
    } else {
        json!({"devicetype": "lueur#desktop", "generateclientkey": true})
    }
    .to_string();
    let v = check(call(ip, &cert, "POST", "/api", Some(&body))?)?;
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
    let success = |key: &str| {
        v.as_array()
            .and_then(|a| a.iter().find_map(|x| x.pointer(&format!("/success/{key}"))))
            .and_then(Value::as_str)
    };
    let username = success("username")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no username in reply"))?;
    let clientkey = success("clientkey")
        .filter(|k| k.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_default();
    Ok(HueBridge {
        id,
        ip: ip.to_string(),
        username: username.to_string(),
        cert,
        clientkey: clientkey.to_string(),
    })
}

/// The bridge id in a `/api/config` reply.
fn config_id(v: &Value) -> Option<String> {
    v.get("bridgeid")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
}

/// The bridge's public config, over HTTP (no username needed).
fn public_config(ip: &str) -> io::Result<Value> {
    check(http::request(
        "GET",
        ip,
        PORT,
        "/api/config",
        None,
        TIMEOUT,
    )?)
}

/// The bridge's id from its public config.
fn bridge_id(ip: &str) -> io::Result<String> {
    config_id(&public_config(ip)?)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a Hue bridge"))
}

/// The pin of the certificate the host at `ip` serves HTTPS with, if what
/// answers there over TLS says it is bridge `id`. `None` for a bridge
/// without HTTPS and anything else; nothing secret is sent to find out.
///
/// This proves nothing by itself: anyone can make a certificate, and the
/// bridge's id is public. It is trust on first use: the certificate is
/// taken on the word of whatever answers the first time (at pairing, or at
/// the paired address for a pairing older than pinning), and from then on
/// only that certificate is.
pub fn learn_cert(ip: &str, id: &str) -> Option<String> {
    let (resp, der) = http::learn(ip, TLS_PORT, "/api/config", TIMEOUT).ok()?;
    let reported = config_id(&check(resp).ok()?)?;
    (!id.is_empty() && reported == id).then(|| pin_hex(&http::fingerprint(&der)))
}

/// Find bridges, then list the lamps of those Lueur is paired with.
///
/// A bridge's username goes to the address it was paired at, or over TLS
/// to its pinned certificate wherever it moved (see [`Approach`]). A paired
/// bridge heard anywhere else is listed unpaired, with the certificate it
/// showed if any: nothing was sent to it, and it is the user's call (or a
/// new pairing) whether it is theirs.
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
    // Every address that answered, with the id it claims.
    let heard: Vec<(String, String)> = bridges
        .into_iter()
        .filter_map(|(ip, id)| {
            let id = match id {
                Some(id) => id,
                None => bridge_id(&ip).ok()?,
            };
            Some((ip, id))
        })
        .collect();

    let mut out = Vec::new();
    for (ip, id) in &heard {
        let paired = known.iter().find(|b| b.id == *id);
        let home_answers =
            paired.is_some_and(|b| heard.iter().any(|(at, claim)| *at == b.ip && claim == id));
        let approach = super::approach(
            ip,
            paired.map(|b| (b.ip.as_str(), !b.cert.is_empty())),
            home_answers,
        );
        match (approach, paired) {
            (Approach::Ignore, _) => {}
            (Approach::Home | Approach::Prove, Some(b)) => out.extend(visit(ip, id, b, approach)),
            (Approach::Ask, _) => out.push(bridge(ip, id, learn_cert(ip, id).unwrap_or_default())),
            _ => out.push(bridge(ip, id, String::new())),
        }
    }
    Ok(out)
}

/// The lamps of paired bridge `b`, heard at `ip`. When they cannot be had
/// with its username, the bridge itself, unpaired.
fn visit(ip: &str, id: &str, b: &HueBridge, approach: Approach) -> Vec<DiscoveredDevice> {
    // A pairing older than pinning learns its certificate at home, where its
    // username has always gone.
    let cert = if b.cert.is_empty() && approach == Approach::Home {
        learn_cert(ip, id).unwrap_or_default()
    } else {
        b.cert.clone()
    };
    match lights(ip, id, &b.username, &cert) {
        // Paired before Lueur asked for the streaming key: its lamps work,
        // and the bridge is offered for pairing again, to stream.
        Ok(mut lamps) if b.clientkey.is_empty() && !cert.is_empty() => {
            lamps.push(DiscoveredDevice {
                faster: true,
                ..bridge(ip, id, cert)
            });
            lamps
        }
        Ok(lamps) => lamps,
        // Another certificate, or none to prove a move: nothing was sent.
        Err(e) if http::is_cert_mismatch(&e) || approach == Approach::Prove => {
            vec![bridge(ip, id, learn_cert(ip, id).unwrap_or_default())]
        }
        // At home but no lamps: the bridge forgot us (factory reset), so
        // offer pairing.
        Err(_) => vec![bridge(ip, id, cert)],
    }
}

/// A bridge Lueur cannot drive (yet), as discovery lists it.
fn bridge(ip: &str, id: &str, cert: String) -> DiscoveredDevice {
    DiscoveredDevice {
        id: format!("{ID_PREFIX}{id}"),
        ip: ip.to_string(),
        port: PORT,
        model: "bridge".into(),
        name: "Hue Bridge".into(),
        support: Vec::new(),
        fw_ver: None,
        kind: DeviceKind::Hue {
            username: String::new(),
            light: String::new(),
            cert,
            clientkey: String::new(),
        },
        paired: false,
        faster: false,
    }
}

fn lights(ip: &str, bridge: &str, username: &str, cert: &str) -> io::Result<Vec<DiscoveredDevice>> {
    let path = format!("/api/{}/lights", safe(username)?);
    let v = check(call(ip, cert, "GET", &path, None)?)?;
    if bridge_error(&v).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unauthorized",
        ));
    }
    Ok(parse_lights(&v, ip, bridge, username, cert))
}

/// The dimmable lamps in a `/lights` listing, reached through the bridge at
/// `ip` with `username` and its pinned `cert`.
pub fn parse_lights(
    v: &Value,
    ip: &str,
    bridge: &str,
    username: &str,
    cert: &str,
) -> Vec<DiscoveredDevice> {
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
                // The streaming key is the pairing's, filled in with the
                // username wherever the light is bound.
                kind: DeviceKind::Hue {
                    username: username.to_string(),
                    light: num.clone(),
                    cert: cert.to_string(),
                    clientkey: String::new(),
                },
                paired: true,
                faster: false,
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
    /// Masks the username the path carries.
    wire: Wire,
}

impl HueDriver {
    /// The bridge's reply, which lists what it refused.
    fn put(&mut self, body: &Value) -> io::Result<Value> {
        let body = body.to_string();
        self.wire.request("PUT", &self.path, Some(&body));
        check(self.client.request("PUT", &self.path, Some(&body))?)
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

    /// A Hue lamp's own loop goes round the colour wheel and nothing else:
    /// it plays the rainbow, and the other shows settle. So does a lamp
    /// without colours, which refuses the loop.
    fn rest_looping(&mut self, own: &OwnLoop) {
        if own.kind == ShowKind::Rainbow {
            let body = json!({"on": true, "effect": "colorloop", "bri": bri(own.bright())});
            if self.put(&body).is_ok_and(|v| bridge_error(&v).is_none()) {
                return;
            }
        }
        self.rest();
    }
}

pub struct HueStream {
    /// Its place in the bridge's stream, when the bridge streams to it.
    seat: Option<Seat>,
    /// The REST API: for every frame when it does not, and to settle it.
    mailbox: Mailbox,
    last: LastSent,
    pace: Throttle,
}

impl HueStream {
    /// `siblings` lamps of the same bridge share its command budget, unless
    /// the bridge streams to them (with the `clientkey` it gave at pairing).
    pub fn open(access: &Access, light: &str, siblings: usize, wire: Wire) -> io::Result<Self> {
        let (trust, port) = link(&access.cert)?;
        let driver = HueDriver {
            client: http::Client::with_trust(&access.ip, port, trust, TIMEOUT)?,
            path: state_path(&access.username, light)?,
            turned_on: false,
            wire: wire.hiding(&access.username),
        };
        let seat = if access.can_stream() {
            match Seat::take(access, light) {
                Ok(seat) => Some(seat),
                Err(e) => {
                    log::debug!(light = light; "not streamed to: {e}");
                    None
                }
            }
        } else {
            None
        };
        // Over REST the first command turns the lamp on; a stream may not.
        if seat.is_some() {
            let on = json!({"on": true, "effect": "none"});
            let _ = put_state(&access.ip, &access.username, light, &access.cert, &on);
        }
        Ok(HueStream {
            seat,
            mailbox: Mailbox::spawn("hue", driver)?,
            last: LastSent::default(),
            pace: Throttle::new(BRIDGE_INTERVAL * siblings as u32),
        })
    }
}

impl LightStream for HueStream {
    fn interval(&self) -> Duration {
        if self.seat.is_some() {
            entertainment::INTERVAL
        } else {
            self.pace.interval()
        }
    }

    fn latency(&self) -> Duration {
        if self.seat.is_some() {
            entertainment::LATENCY
        } else {
            LATENCY
        }
    }

    fn zones(&self) -> usize {
        self.seat.as_ref().map_or(1, Seat::zones)
    }

    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        if let Some(seat) = &self.seat {
            return seat.show(&[output]);
        }
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        let ms = self.pace.glide(effect, duration_ms);
        self.mailbox.push(output, effect, ms)?;
        self.last.mark(output);
        Ok(())
    }

    fn push_zones(
        &mut self,
        outputs: &[BulbOutput],
        effect: Effect,
        duration_ms: u32,
    ) -> io::Result<()> {
        match &self.seat {
            Some(seat) => seat.show(outputs),
            None => match outputs.iter().max_by_key(|o| o.bright()) {
                Some(&o) => self.push(o, effect, duration_ms),
                None => Ok(()),
            },
        }
    }

    fn rest(&mut self) {
        // The stream would cover the glow: off it first.
        if let Some(seat) = self.seat.as_mut() {
            seat.leave();
        }
        self.mailbox.rest();
    }

    fn rest_looping(&mut self, own: &OwnLoop) {
        if let Some(seat) = self.seat.as_mut() {
            seat.leave();
        }
        self.mailbox.rest_looping(own);
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
        let lamps = parse_lights(&v, "192.168.1.30", "001788fffe1a2b3c", "user", "");
        assert_eq!(lamps.len(), 2);
        assert_eq!(lamps[0].id, "hue:001788fffe1a2b3c:1");
        assert_eq!(lamps[0].name, "Desk");
        assert_eq!(
            lamps[1].kind,
            DeviceKind::Hue {
                username: "user".into(),
                light: "3".into(),
                cert: String::new(),
                clientkey: String::new(),
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

    #[test]
    fn pins_round_trip_and_never_fall_back_to_the_clear() {
        let pin = [0xabu8; 32];
        let hex = pin_hex(&pin);
        assert_eq!(hex.len(), 64);
        assert_eq!(parse_pin(&hex), Some(pin));
        assert_eq!(link(&hex).unwrap(), (Trust::Pinned(pin), TLS_PORT));
        assert_eq!(link("").unwrap(), (Trust::Clear, PORT));
        // A damaged pin reaches nothing, rather than the bridge in the clear.
        for bad in ["zz", &hex[..62], &format!("{hex}00"), &"g".repeat(64)] {
            assert!(link(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pairing_needs_https_except_on_the_round_bridge() {
        let seen = Some("ab".repeat(32));
        assert_eq!(
            pairing_cert(seen.clone(), None, Some("BSB002")),
            Ok("ab".repeat(32))
        );
        // No HTTPS: only the round bridge may be paired in the clear.
        assert_eq!(
            pairing_cert(None, None, Some(ROUND_BRIDGE)),
            Ok(String::new())
        );
        assert_eq!(
            pairing_cert(None, None, Some("BSB002")),
            Err(PairRefusal::NoTls)
        );
        assert_eq!(pairing_cert(None, None, None), Err(PairRefusal::NoTls));
    }

    #[test]
    fn pairing_again_needs_the_pinned_certificate() {
        let pin = "ab".repeat(32);
        assert_eq!(
            pairing_cert(Some(pin.clone()), Some(&pin), None),
            Ok(pin.clone())
        );
        assert_eq!(
            pairing_cert(Some("cd".repeat(32)), Some(&pin), None),
            Err(PairRefusal::CertChanged)
        );
        // A pinned bridge that stops answering over TLS is not unpinned,
        // even if it claims to be the round one.
        assert_eq!(
            pairing_cert(None, Some(&pin), Some(ROUND_BRIDGE)),
            Err(PairRefusal::NoTls)
        );
        let e = io::Error::new(io::ErrorKind::PermissionDenied, PairRefusal::NoTls);
        assert_eq!(pair_refusal(&e).map(PairRefusal::code), Some("pair_no_tls"));
        assert_eq!(pair_refusal(&io::Error::other("x")), None);
    }

    #[test]
    fn a_pinned_stream_refuses_a_bad_pin_before_any_traffic() {
        // TEST-NET: never routable, and never reached.
        let wire = Wire::new("hue", "hue:b:1");
        let access = Access {
            ip: "192.0.2.1".into(),
            username: "user".into(),
            clientkey: String::new(),
            cert: "nonsense".into(),
        };
        assert!(HueStream::open(&access, "1", 1, wire).is_err());
        assert!(set_power("192.0.2.1", "user", "1", "nonsense", true).is_err());
    }
}
