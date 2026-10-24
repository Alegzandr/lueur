//! Every bulb family behind one interface.
//!
//! The engine produces one [`BulbOutput`] per light per frame; how that reaches
//! a bulb differs wildly between brands (a Yeelight music-mode socket, a WiZ
//! UDP datagram, a Hue REST call…). This module hides
//! that behind three operations — stream, power, identify — keyed on a
//! [`DeviceKind`] that also carries whatever credential the family needs.
//!
//! Families whose bulbs only speak to paired clients (Hue, Nanoleaf)
//! keep those pairings in [`Pairings`], so a later search can list their bulbs
//! as ready to use.

pub mod common;
pub mod govee;
pub mod http;
pub mod hue;
pub mod lifx;
pub mod mdns;
pub mod nanoleaf;
pub mod tuya;
pub mod wiz;

use crate::color::{BulbOutput, Rgb};
use crate::yeelight::{self, Effect, MusicConnection, YeelightControl};
use serde::{Deserialize, Serialize};
use std::io;
use std::time::Duration;

/// Which protocol a bulb speaks, plus what that protocol needs to be spoken.
///
/// Serialised with a `family` tag so a binding written before families
/// existed (no `kind` at all) reads back as the Yeelight it always was.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
            DeviceKind::Hue { username, light } => !username.is_empty() && !light.is_empty(),
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
    #[serde(default = "yes")]
    pub paired: bool,
}

fn yes() -> bool {
    true
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
        }
    }
}

/// A Hue bridge Lueur holds a username for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HueBridge {
    /// Bridge id, lower-case hex.
    pub id: String,
    /// Last address it was seen at.
    pub ip: String,
    pub username: String,
}

/// A Nanoleaf controller Lueur holds a token for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NanoleafPairing {
    pub id: String,
    pub ip: String,
    pub token: String,
}

/// A Tuya bulb imported from the user's Smart Life account. Only what local
/// control needs is kept; the account's tokens are discarded after import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A live, per-frame channel to one bulb.
pub trait LightStream: Send {
    /// Send one frame's output. Implementations skip what the bulb already
    /// shows and pace themselves to what the device tolerates, so calling this
    /// every frame is always fine.
    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()>;

    /// Leave the bulb on a gentle warm glow when the effect stops, so the room
    /// stays cosy rather than snapping to black (or to the last dark frame).
    fn rest(&mut self);
}

/// The colour temperature and brightness [`LightStream::rest`] settles on.
pub const REST_KELVIN: u16 = 2700;
pub const REST_BRIGHT: u8 = 40;

impl LightStream for MusicConnection {
    fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) -> io::Result<()> {
        MusicConnection::push(self, output, effect, duration_ms)
    }

    fn rest(&mut self) {
        let _ = self.set_bright(REST_BRIGHT, Effect::Smooth, 400);
        let _ = self.set_ct(REST_KELVIN, Effect::Smooth, 400);
    }
}

/// How long one-shot commands and stream handshakes may take.
pub const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);

/// Open a per-frame stream to `ep`.
///
/// `siblings` is how many lights in this effect share the same host (a Hue
/// bridge), so families with a shared command budget can split it fairly.
pub fn open_stream(ep: &Endpoint, siblings: usize) -> io::Result<Box<dyn LightStream>> {
    require_ready(ep)?;
    Ok(match &ep.kind {
        DeviceKind::Yeelight => Box::new(MusicConnection::establish(
            &ep.ip,
            ep.port,
            CONTROL_TIMEOUT,
        )?),
        DeviceKind::Wiz => Box::new(wiz::WizStream::open(&ep.ip)?),
        DeviceKind::Lifx { target } => Box::new(lifx::LifxStream::open(&ep.ip, ep.port, target)?),
        DeviceKind::Govee => Box::new(govee::GoveeStream::open(&ep.ip)?),
        DeviceKind::Hue { username, light } => Box::new(hue::HueStream::open(
            &ep.ip,
            username,
            light,
            siblings.max(1),
        )?),
        DeviceKind::Nanoleaf { token } => {
            Box::new(nanoleaf::NanoleafStream::open(&ep.ip, ep.port, token)?)
        }
        DeviceKind::Tuya { .. } => return Err(shell_only()),
    })
}

/// Turn a bulb on or off.
pub fn set_power(ep: &Endpoint, on: bool) -> io::Result<()> {
    require_ready(ep)?;
    match &ep.kind {
        DeviceKind::Yeelight => {
            YeelightControl::connect(&ep.ip, ep.port, CONTROL_TIMEOUT)?.set_power(on)
        }
        DeviceKind::Wiz => wiz::set_power(&ep.ip, on),
        DeviceKind::Lifx { target } => lifx::set_power(&ep.ip, ep.port, target, on),
        DeviceKind::Govee => govee::set_power(&ep.ip, on),
        DeviceKind::Hue { username, light } => hue::set_power(&ep.ip, username, light, on),
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
    match &ep.kind {
        DeviceKind::Yeelight => {
            let mut ctrl = YeelightControl::connect(&ep.ip, ep.port, CONTROL_TIMEOUT)?;
            ctrl.set_power(true)?;
            ctrl.set_rgb(color)
        }
        // Everyone else: power on, then one frame on a fresh stream.
        _ => {
            set_power(ep, true)?;
            open_stream(ep, 1)?.push(output, Effect::Smooth, 300)
        }
    }
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
    let searches: Vec<Search> = vec![
        Box::new(move || {
            Ok(yeelight::discover(timeout)?
                .into_iter()
                .map(DiscoveredDevice::from)
                .collect())
        }),
        Box::new(move || wiz::discover(timeout)),
        Box::new(move || lifx::discover(timeout)),
        Box::new(move || govee::discover(timeout)),
        Box::new(move || hue::discover(timeout, &pairings.hue)),
        Box::new(move || nanoleaf::discover(timeout, &pairings.nanoleaf)),
    ];

    let results: Vec<io::Result<Vec<DiscoveredDevice>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = searches.into_iter().map(|s| scope.spawn(s)).collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(io::Error::other("search panicked")))
            })
            .collect()
    });

    let mut found = Vec::new();
    let mut last_err = None;
    let mut any_ok = false;
    for r in results {
        match r {
            Ok(devices) => {
                any_ok = true;
                found.extend(devices);
            }
            Err(e) => last_err = Some(e),
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
                light: String::new()
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
            },
        };
        let err = set_power(&ep, true).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
