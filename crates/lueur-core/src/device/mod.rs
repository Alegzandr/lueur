//! Every bulb family behind one interface.
//!
//! The engine produces one [`BulbOutput`] per light per frame; how that reaches
//! a bulb differs wildly between brands (a Yeelight music-mode socket, a WiZ
//! UDP datagram, a Hue REST call…). This module hides
//! that behind three operations (stream, power, identify) keyed on a
//! [`DeviceKind`] that also carries whatever credential the family needs.
//!
//! Families whose bulbs only speak to paired clients (Hue, Nanoleaf)
//! keep those pairings in [`Pairings`], so a later search can list their bulbs
//! as ready to use.

pub mod common;
pub mod dtls;
pub mod entertainment;
pub mod govee;
pub mod http;
pub mod hue;
pub mod lease;
pub mod lifx;
pub mod mdns;
pub mod nanoleaf;
pub mod relink;
pub mod tuya;
pub mod wire;
pub mod wiz;

use crate::color::{BulbOutput, Rgb};
use crate::show::OwnLoop;
use crate::yeelight::{self, Effect, MusicConnection, YeelightControl};
use common::{Driver, Mailbox};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::time::Duration;
use wire::Wire;

/// Which protocol a bulb speaks, plus what that protocol needs to be spoken.
///
/// Serialised with a `family` tag so a binding written before families
/// existed (no `kind` at all) reads back as the Yeelight it always was.
/// Its `Debug` form masks the credential (see [`Masked`]).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "lowercase")]
pub enum DeviceKind {
    #[default]
    Yeelight,
    Wiz,
    /// `target` is the bulb's MAC as 12 hex digits; LIFX addresses frames by it.
    Lifx {
        #[serde(default)]
        target: String,
    },
    Govee,
    /// A light behind a Hue bridge. `ip` on the binding is the bridge's.
    /// Both fields are empty for a bridge Lueur has not been paired with.
    Hue {
        #[serde(default)]
        username: String,
        #[serde(default)]
        light: String,
        /// The bridge's pinned certificate (see [`HueBridge::cert`]): when
        /// set, the username only goes over TLS to that certificate.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        cert: String,
        /// The key the bridge streams with (see [`HueBridge::clientkey`]);
        /// empty for a pairing made before Lueur asked for one.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        clientkey: String,
    },
    Nanoleaf {
        #[serde(default)]
        token: String,
    },
    /// A Tuya / Smart Life bulb. The shell drives these (see
    /// [`tuya`]); `key` is the device's local key from the Smart Life
    /// sign-in, `version` the protocol it answered to (empty until known).
    Tuya {
        #[serde(default)]
        key: String,
        #[serde(default)]
        version: String,
        #[serde(default)]
        dps: tuya::TuyaDps,
    },
}

impl DeviceKind {
    pub fn family(&self) -> &'static str {
        match self {
            DeviceKind::Yeelight => "yeelight",
            DeviceKind::Wiz => "wiz",
            DeviceKind::Lifx { .. } => "lifx",
            DeviceKind::Govee => "govee",
            DeviceKind::Hue { .. } => "hue",
            DeviceKind::Nanoleaf { .. } => "nanoleaf",
            DeviceKind::Tuya { .. } => "tuya",
        }
    }

    /// Whether this kind carries what it needs to be controlled.
    pub fn is_ready(&self) -> bool {
        match self {
            DeviceKind::Hue {
                username, light, ..
            } => !username.is_empty() && !light.is_empty(),
            DeviceKind::Nanoleaf { token } => !token.is_empty(),
            DeviceKind::Tuya { key, .. } => !key.is_empty(),
            _ => true,
        }
    }
}

/// Everything needed to reach one bulb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Stable device id (family-prefixed for every family but Yeelight).
    pub id: String,
    pub ip: String,
    pub port: u16,
    #[serde(default)]
    pub kind: DeviceKind,
}

/// A bulb (or a bridge waiting to be paired) found on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    pub id: String,
    pub ip: String,
    pub port: u16,
    /// Model string as the device reports it (`color`, `H6008`, `bridge`…).
    pub model: String,
    /// The name the owner gave it in its own app, when the protocol exposes one.
    pub name: String,
    /// Yeelight's advertised methods; empty for other families.
    #[serde(default)]
    pub support: Vec<String>,
    pub fw_ver: Option<String>,
    #[serde(default)]
    pub kind: DeviceKind,
    /// False for a Hue bridge or Nanoleaf Lueur cannot drive yet:
    /// the interface offers pairing instead of adding it.
    #[serde(default = "crate::default_true")]
    pub paired: bool,
    /// Paired, but pairing it again would let it do more: a Hue bridge
    /// paired before Lueur asked it for its streaming key.
    #[serde(default)]
    pub faster: bool,
}

impl DiscoveredDevice {
    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            id: self.id.clone(),
            ip: self.ip.clone(),
            port: self.port,
            kind: self.kind.clone(),
        }
    }
}

impl From<yeelight::DiscoveredDevice> for DiscoveredDevice {
    fn from(d: yeelight::DiscoveredDevice) -> Self {
        DiscoveredDevice {
            id: d.id,
            ip: d.ip,
            port: d.port,
            model: d.model,
            name: d.name,
            support: d.support,
            fw_ver: d.fw_ver,
            kind: DeviceKind::Yeelight,
            paired: true,
            faster: false,
        }
    }
}

/// A Hue bridge Lueur holds a username for.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HueBridge {
    /// Bridge id, lower-case hex.
    pub id: String,
    /// The address it was paired at, or last proved itself at.
    pub ip: String,
    pub username: String,
    /// SHA-256 of the certificate its HTTPS server showed, lower-case hex:
    /// pinned at pairing, or at the first visit for a pairing older than
    /// that. Empty for a bridge without HTTPS, which is then spoken to in
    /// the clear, and only at `ip`.
    #[serde(default)]
    pub cert: String,
    /// The key the bridge's Entertainment API streams with, hex: as secret
    /// as the username. Empty for a pairing made before Lueur asked for
    /// one, which then drives its lamps one command at a time.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub clientkey: String,
}

/// A Nanoleaf controller Lueur holds a token for. Its API is HTTP only, so
/// the token only goes to `ip`, where it was granted, until the user says
/// the controller moved.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NanoleafPairing {
    /// The controller's own id, from its mDNS record.
    pub id: String,
    pub ip: String,
    pub token: String,
}

/// What Lueur may send to a bridge or controller that discovery heard at an
/// address, given the pairing it holds for that device's id.
///
/// Anything on the network can claim a device's id, so the id alone opens
/// nothing: the credential goes to the address it was granted at, or to one
/// the device proves itself at with its pinned certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approach {
    /// No pairing: it is only offered for pairing.
    Unpaired,
    /// At its paired address: its credential goes there.
    Home,
    /// Elsewhere, with a pinned certificate: the credential goes only over
    /// TLS to that certificate, and the pairing follows if it is shown.
    Prove,
    /// Elsewhere, with nothing to prove it is the same device: nothing is
    /// sent until the user says it is theirs.
    Ask,
    /// Elsewhere while the paired device still answers at home: someone else.
    Ignore,
}

/// The [`Approach`] for a device heard at `found_ip`. `paired` is its
/// pairing's address and whether a certificate is pinned for it;
/// `home_answers` whether the paired device answered at that address.
pub fn approach(found_ip: &str, paired: Option<(&str, bool)>, home_answers: bool) -> Approach {
    match paired {
        None => Approach::Unpaired,
        Some((home, _)) if home == found_ip => Approach::Home,
        Some(_) if home_answers => Approach::Ignore,
        Some((_, true)) => Approach::Prove,
        Some((_, false)) => Approach::Ask,
    }
}

/// A Tuya bulb imported from the user's Smart Life account. Only what local
/// control needs is kept; the account's tokens are discarded after import.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuyaDevice {
    /// Tuya device id, as the cloud reports it (no family prefix).
    pub id: String,
    pub name: String,
    pub key: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub dps: tuya::TuyaDps,
}

/// Credentials Lueur was granted, by family.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pairings {
    #[serde(default)]
    pub hue: Vec<HueBridge>,
    #[serde(default)]
    pub nanoleaf: Vec<NanoleafPairing>,
    #[serde(default)]
    pub tuya: Vec<TuyaDevice>,
}

/// A credential as `Debug` shows it: whether there is one, never what it
/// is, so a credential cannot reach a log through `{:?}`.
pub struct Masked<'a>(pub &'a str);

impl fmt::Debug for Masked<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() {
            "\"\""
        } else {
            wire::MASK
        })
    }
}

impl fmt::Debug for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceKind::Yeelight => f.write_str("Yeelight"),
            DeviceKind::Wiz => f.write_str("Wiz"),
            DeviceKind::Lifx { target } => f.debug_struct("Lifx").field("target", target).finish(),
            DeviceKind::Govee => f.write_str("Govee"),
            DeviceKind::Hue {
                username,
                light,
                cert,
                clientkey,
            } => f
                .debug_struct("Hue")
                .field("username", &Masked(username))
                .field("light", light)
                .field("cert", cert)
                .field("clientkey", &Masked(clientkey))
                .finish(),
            DeviceKind::Nanoleaf { token } => f
                .debug_struct("Nanoleaf")
                .field("token", &Masked(token))
                .finish(),
            DeviceKind::Tuya { key, version, dps } => f
                .debug_struct("Tuya")
                .field("key", &Masked(key))
                .field("version", version)
                .field("dps", dps)
                .finish(),
        }
    }
}

impl fmt::Debug for HueBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HueBridge")
            .field("id", &self.id)
            .field("ip", &self.ip)
            .field("username", &Masked(&self.username))
            .field("cert", &self.cert)
            .field("clientkey", &Masked(&self.clientkey))
            .finish()
    }
}

impl fmt::Debug for NanoleafPairing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NanoleafPairing")
            .field("id", &self.id)
            .field("ip", &self.ip)
            .field("token", &Masked(&self.token))
            .finish()
    }
}

impl fmt::Debug for TuyaDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TuyaDevice")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key", &Masked(&self.key))
            .field("product", &self.product)
            .field("dps", &self.dps)
            .finish()
    }
}

/// A live, per-frame channel to one bulb.
pub trait LightStream: Send {
    /// Send one frame's output. Implementations skip what the bulb already
    /// shows and pace themselves to what the device tolerates, so calling this
    /// every frame is always fine.
    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()>;

    /// The shortest gap it leaves between two commands: how fast the bulb
    /// can follow. Nothing for a stream that takes every frame.
    fn interval(&self) -> Duration {
        Duration::ZERO
    }

    /// How long a command takes to show: the trip to the bulb and its LEDs'
    /// own rise. The music sends its beats that much ahead.
    fn latency(&self) -> Duration {
        DEFAULT_LATENCY
    }

    /// How many parts of itself the light colours on its own, left to right
    /// (a strip's segments, a layout's panels): 1 for a bulb, never more
    /// than [`MAX_ZONES`].
    fn zones(&self) -> usize {
        1
    }

    /// Send one frame with an output for each zone, left to right. However
    /// many come, they are spread over the zones the light has; a light of
    /// one zone shows the brightest.
    fn push_zones(
        &mut self,
        outputs: &[BulbOutput],
        effect: Effect,
        duration_ms: u32,
    ) -> io::Result<()> {
        match outputs.iter().max_by_key(|o| o.bright()) {
            Some(&output) => self.push(output, effect, duration_ms),
            None => Ok(()),
        }
    }

    /// Leave the bulb on a gentle warm glow when the effect stops, so the room
    /// stays cosy rather than snapping to black (or to the last dark frame).
    fn rest(&mut self);

    /// Leave the bulb playing `own` by itself, the way its own app would,
    /// when Lueur quits during a show: the show goes on without Lueur. A
    /// bulb that cannot settles on the warm glow instead.
    fn rest_looping(&mut self, own: &OwnLoop) {
        let _ = own;
        self.rest();
    }
}

/// The most zones a light plays as (see [`LightStream::zones`]): a long
/// strip's dozens of segments are grouped into this many, enough for a
/// chase to run along it without drowning the rest of the room.
pub const MAX_ZONES: usize = 8;

/// What a command takes to show when nothing better is known.
pub const DEFAULT_LATENCY: Duration = Duration::from_millis(50);

/// Output `j` of `n` zones, from `outputs` spread over them.
pub fn zone_output(outputs: &[BulbOutput], j: usize, n: usize) -> Option<BulbOutput> {
    let len = outputs.len();
    (len > 0).then(|| outputs[(j * len / n.max(1)).min(len - 1)])
}

/// Split `n` items laid left to right into at most [`MAX_ZONES`] runs of
/// neighbours, as even as can be: the zone each item belongs to.
pub fn group_zones(n: usize) -> Vec<usize> {
    let zones = n.clamp(1, MAX_ZONES);
    (0..n).map(|i| i * zones / n.max(1)).collect()
}

/// The colour temperature and brightness [`LightStream::rest`] settles on.
pub const REST_KELVIN: u16 = 2700;
pub const REST_BRIGHT: u8 = 40;

impl Driver for MusicConnection {
    fn send(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        MusicConnection::push(self, output, effect, duration_ms)
    }

    fn rest(&mut self) {
        let _ = self.set_bright(REST_BRIGHT, Effect::Smooth, 400);
        let _ = self.set_ct(REST_KELVIN, Effect::Smooth, 400);
    }

    fn rest_looping(&mut self, own: &OwnLoop) {
        let _ = self.play_loop(&own.steps);
    }
}

/// A Yeelight's music channel, written from its own thread.
///
/// The write is a blocking TCP send: a bulb that stops reading (a Wi-Fi
/// stall, unplugged without a reset) would otherwise hold the frame loop for
/// the write timeout, every frame, and freeze every other light with it.
struct YeelightStream(Mailbox);

impl LightStream for YeelightStream {
    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        self.0.push(output, effect, duration_ms)
    }

    fn latency(&self) -> Duration {
        yeelight::LATENCY
    }

    fn rest(&mut self) {
        self.0.rest();
    }

    fn rest_looping(&mut self, own: &OwnLoop) {
        self.0.rest_looping(own);
    }
}

/// How long one-shot commands and stream handshakes may take.
pub const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);

/// Open a per-frame stream to `ep`.
///
/// `siblings` is how many lights in this effect share the same host (a Hue
/// bridge), so families with a shared command budget can split it fairly.
/// When `polite`, a bulb that says something else streams to it is left
/// alone, with [`lease::taken`]: only a Yeelight says, through its one
/// music channel.
pub fn open_stream(
    ep: &Endpoint,
    siblings: usize,
    polite: bool,
) -> io::Result<Box<dyn LightStream>> {
    require_ready(ep)?;
    log::debug!(family = ep.kind.family(), id = ep.id.as_str(), ip = ep.ip.as_str(); "opening a stream");
    let wire = Wire::of(ep);
    Ok(match &ep.kind {
        DeviceKind::Yeelight => Box::new(YeelightStream(Mailbox::spawn(
            "yeelight",
            MusicConnection::establish(&ep.ip, ep.port, CONTROL_TIMEOUT, polite)?.traced(wire),
        )?)),
        DeviceKind::Wiz => Box::new(wiz::WizStream::open(&ep.ip, wire)?),
        DeviceKind::Lifx { target } => {
            Box::new(lifx::LifxStream::open(&ep.ip, ep.port, target, wire)?)
        }
        DeviceKind::Govee => Box::new(govee::GoveeStream::open(&ep.ip, wire)?),
        DeviceKind::Hue {
            username,
            light,
            cert,
            clientkey,
        } => {
            let access = entertainment::Access {
                ip: ep.ip.clone(),
                username: username.clone(),
                clientkey: clientkey.clone(),
                cert: cert.clone(),
            };
            Box::new(hue::HueStream::open(&access, light, siblings.max(1), wire)?)
        }
        DeviceKind::Nanoleaf { token } => Box::new(nanoleaf::NanoleafStream::open(
            &ep.ip, ep.port, token, wire,
        )?),
        DeviceKind::Tuya { .. } => return Err(shell_only()),
    })
}

/// The pace a stream to `kind` would keep (see [`LightStream::interval`]),
/// without opening one: for a light that waits for its bulb. Tuya's is the
/// shell's.
pub fn interval_of(kind: &DeviceKind, siblings: usize) -> Duration {
    match kind {
        DeviceKind::Yeelight | DeviceKind::Tuya { .. } => Duration::ZERO,
        DeviceKind::Wiz => wiz::FRAME_INTERVAL,
        DeviceKind::Lifx { .. } => lifx::FRAME_INTERVAL,
        DeviceKind::Govee => govee::FRAME_INTERVAL,
        DeviceKind::Hue { clientkey, .. } if !clientkey.is_empty() => entertainment::INTERVAL,
        DeviceKind::Hue { .. } => hue::BRIDGE_INTERVAL * siblings.max(1) as u32,
        DeviceKind::Nanoleaf { .. } => nanoleaf::FRAME_INTERVAL,
    }
}

/// How long a command to `kind` takes to show (see
/// [`LightStream::latency`]), without opening a stream: for a light that
/// waits for its bulb. Tuya's is the shell's.
pub fn latency_of(kind: &DeviceKind) -> Duration {
    match kind {
        DeviceKind::Yeelight => yeelight::LATENCY,
        DeviceKind::Wiz => wiz::LATENCY,
        DeviceKind::Lifx { .. } => lifx::LATENCY,
        DeviceKind::Govee => govee::LATENCY,
        DeviceKind::Hue { clientkey, .. } if !clientkey.is_empty() => entertainment::LATENCY,
        DeviceKind::Hue { .. } => hue::LATENCY,
        DeviceKind::Nanoleaf { .. } => nanoleaf::LATENCY,
        DeviceKind::Tuya { .. } => DEFAULT_LATENCY,
    }
}

/// Turn a bulb on or off.
pub fn set_power(ep: &Endpoint, on: bool) -> io::Result<()> {
    require_ready(ep)?;
    log::debug!(family = ep.kind.family(), id = ep.id.as_str(), ip = ep.ip.as_str(), on = on; "power");
    match &ep.kind {
        DeviceKind::Yeelight => {
            YeelightControl::connect(&ep.ip, ep.port, CONTROL_TIMEOUT)?.set_power(on)
        }
        DeviceKind::Wiz => wiz::set_power(&ep.ip, on),
        DeviceKind::Lifx { target } => lifx::set_power(&ep.ip, ep.port, target, on),
        DeviceKind::Govee => govee::set_power(&ep.ip, on),
        DeviceKind::Hue {
            username,
            light,
            cert,
            ..
        } => hue::set_power(&ep.ip, username, light, cert, on),
        DeviceKind::Nanoleaf { token } => nanoleaf::set_power(&ep.ip, ep.port, token, on),
        DeviceKind::Tuya { .. } => Err(shell_only()),
    }
}

/// Switch a bulb on at `color`, full brightness, so the user can tell which
/// one it is.
pub fn identify(ep: &Endpoint, color: Rgb) -> io::Result<()> {
    require_ready(ep)?;
    let output = BulbOutput::Color {
        rgb: crate::color::full_chroma(color),
        bright: 100,
    };
    log::debug!(family = ep.kind.family(), id = ep.id.as_str(), ip = ep.ip.as_str(); "identify");
    match &ep.kind {
        DeviceKind::Yeelight => {
            let mut ctrl = YeelightControl::connect(&ep.ip, ep.port, CONTROL_TIMEOUT)?;
            ctrl.set_power(true)?;
            ctrl.set_rgb(color)
        }
        DeviceKind::Nanoleaf { token } => {
            nanoleaf::identify(&ep.ip, ep.port, token, crate::color::full_chroma(color))
        }
        // Everyone else: power on, then one frame on a fresh stream.
        _ => {
            set_power(ep, true)?;
            open_stream(&unstreamed(ep), 1, false)?.push(output, Effect::Smooth, 300)
        }
    }
}

/// `ep` driven one command at a time: a Hue lamp over REST rather than in
/// its bridge's stream. For a flash to identify it or a colour set by hand,
/// which are no reason to set a stream up, and which a stream shared by the
/// bridge's lamps would set up anew, interrupting the others, each time
/// another lamp joined.
pub fn unstreamed(ep: &Endpoint) -> Endpoint {
    let mut ep = ep.clone();
    if let DeviceKind::Hue { clientkey, .. } = &mut ep.kind {
        clientkey.clear();
    }
    ep
}

/// Tuya's transport lives in the shell, which handles these kinds itself.
fn shell_only() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "Tuya devices are driven by the shell",
    )
}

fn require_ready(ep: &Endpoint) -> io::Result<()> {
    if ep.kind.is_ready() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "this device has not been paired yet",
        ))
    }
}

/// Search every family at once and merge what answers.
///
/// Each family gets its own thread and the same `timeout`, so the whole search
/// takes about as long as the slowest family. A family that cannot search (a
/// port already taken, say) is skipped rather than failing the others; only
/// when every family fails is the error returned.
pub fn discover(timeout: Duration, pairings: &Pairings) -> io::Result<Vec<DiscoveredDevice>> {
    type Search<'a> = Box<dyn FnOnce() -> io::Result<Vec<DiscoveredDevice>> + Send + 'a>;
    let searches: Vec<(&str, Search)> = vec![
        (
            "yeelight",
            Box::new(move || {
                Ok(yeelight::discover(timeout)?
                    .into_iter()
                    .map(DiscoveredDevice::from)
                    .collect())
            }),
        ),
        ("wiz", Box::new(move || wiz::discover(timeout))),
        ("lifx", Box::new(move || lifx::discover(timeout))),
        ("govee", Box::new(move || govee::discover(timeout))),
        (
            "hue",
            Box::new(move || hue::discover(timeout, &pairings.hue)),
        ),
        (
            "nanoleaf",
            Box::new(move || nanoleaf::discover(timeout, &pairings.nanoleaf)),
        ),
    ];

    let results: Vec<(&str, io::Result<Vec<DiscoveredDevice>>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = searches
            .into_iter()
            .map(|(family, s)| (family, scope.spawn(s)))
            .collect();
        handles
            .into_iter()
            .map(|(family, h)| {
                let r = h
                    .join()
                    .unwrap_or_else(|_| Err(io::Error::other("search panicked")));
                (family, r)
            })
            .collect()
    });

    let mut found = Vec::new();
    let mut last_err = None;
    let mut any_ok = false;
    for (family, r) in results {
        match r {
            Ok(devices) => {
                log::debug!(family = family; "search found {}", devices.len());
                any_ok = true;
                found.extend(devices);
            }
            Err(e) => {
                log::warn!(family = family; "search failed: {e}");
                last_err = Some(e);
            }
        }
    }
    if !any_ok && let Some(e) = last_err {
        return Err(e);
    }
    // One entry per device, even if two searches heard it.
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found.dedup_by(|a, b| a.id == b.id);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_without_kind_is_a_yeelight() {
        let ep: Endpoint =
            serde_json::from_str(r#"{"id":"0x1","ip":"192.168.1.5","port":55443}"#).unwrap();
        assert_eq!(ep.kind, DeviceKind::Yeelight);
    }

    #[test]
    fn kinds_round_trip_with_a_family_tag() {
        let kind = DeviceKind::Hue {
            username: "u".into(),
            light: "3".into(),
            cert: String::new(),
            clientkey: String::new(),
        };
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, r#"{"family":"hue","username":"u","light":"3"}"#);
        assert_eq!(serde_json::from_str::<DeviceKind>(&json).unwrap(), kind);
        assert_eq!(
            serde_json::from_str::<DeviceKind>(r#"{"family":"wiz"}"#).unwrap(),
            DeviceKind::Wiz
        );
    }

    #[test]
    fn unpaired_kinds_are_not_ready() {
        assert!(
            !DeviceKind::Hue {
                username: String::new(),
                light: String::new(),
                cert: String::new(),
                clientkey: String::new(),
            }
            .is_ready()
        );
        assert!(
            !DeviceKind::Nanoleaf {
                token: String::new()
            }
            .is_ready()
        );
        assert!(DeviceKind::Wiz.is_ready());
        assert!(DeviceKind::Yeelight.is_ready());
    }

    #[test]
    fn unpaired_endpoints_are_refused_before_any_traffic() {
        let ep = Endpoint {
            id: "hue:x".into(),
            // TEST-NET: never routable, and never reached.
            ip: "192.0.2.1".into(),
            port: 80,
            kind: DeviceKind::Hue {
                username: String::new(),
                light: String::new(),
                cert: String::new(),
                clientkey: String::new(),
            },
        };
        let err = set_power(&ep, true).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_credential_only_goes_where_it_was_granted_or_proved() {
        let home = "192.168.1.2";
        let away = "192.168.1.66";
        assert_eq!(approach(away, None, false), Approach::Unpaired);
        // At home, pinned or not.
        assert_eq!(approach(home, Some((home, false)), true), Approach::Home);
        assert_eq!(approach(home, Some((home, true)), false), Approach::Home);
        // Elsewhere: only a pinned certificate can vouch for it.
        assert_eq!(approach(away, Some((home, true)), false), Approach::Prove);
        assert_eq!(approach(away, Some((home, false)), false), Approach::Ask);
        // Elsewhere while the real one answers at home: someone else.
        assert_eq!(approach(away, Some((home, true)), true), Approach::Ignore);
        assert_eq!(approach(away, Some((home, false)), true), Approach::Ignore);
    }

    #[test]
    fn debug_forms_never_show_a_credential() {
        let kinds = [
            DeviceKind::Hue {
                username: "hue-secret".into(),
                light: "1".into(),
                cert: String::new(),
                clientkey: "hue-secret-key".into(),
            },
            DeviceKind::Nanoleaf {
                token: "nano-secret".into(),
            },
            DeviceKind::Tuya {
                key: "tuya-secret".into(),
                version: "3.3".into(),
                dps: tuya::TuyaDps::default(),
            },
        ];
        for kind in kinds {
            let ep = Endpoint {
                id: "x".into(),
                ip: "192.168.1.2".into(),
                port: 80,
                kind,
            };
            let shown = format!("{ep:?}");
            assert!(
                !shown.contains("secret") && shown.contains(wire::MASK),
                "{shown}"
            );
        }
        let pairings = Pairings {
            hue: vec![HueBridge {
                id: "b".into(),
                ip: "192.168.1.2".into(),
                username: "hue-secret".into(),
                cert: String::new(),
                clientkey: "hue-secret-key".into(),
            }],
            nanoleaf: vec![NanoleafPairing {
                id: "n".into(),
                ip: "192.168.1.3".into(),
                token: "nano-secret".into(),
            }],
            tuya: vec![TuyaDevice {
                id: "t".into(),
                name: "Lamp".into(),
                key: "tuya-secret".into(),
                product: String::new(),
                dps: tuya::TuyaDps::default(),
            }],
        };
        let shown = format!("{pairings:?}");
        assert!(!shown.contains("secret"), "{shown}");
        // No credential reads as none, not as a masked one.
        let bare = DeviceKind::Nanoleaf {
            token: String::new(),
        };
        assert_eq!(format!("{bare:?}"), r#"Nanoleaf { token: "" }"#);
    }

    #[test]
    fn older_pairings_read_back_without_a_certificate() {
        let b: HueBridge =
            serde_json::from_str(r#"{"id":"b1","ip":"192.168.1.2","username":"u"}"#).unwrap();
        assert!(b.cert.is_empty());
        let k: DeviceKind =
            serde_json::from_str(r#"{"family":"hue","username":"u","light":"1"}"#).unwrap();
        assert!(matches!(k, DeviceKind::Hue { cert, .. } if cert.is_empty()));
    }
}
