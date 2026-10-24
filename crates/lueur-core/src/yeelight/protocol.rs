//! The Yeelight LAN protocol: building command lines and parsing the SSDP
//! discovery response. This module is pure (no I/O) so it can be exhaustively
//! unit-tested; the actual sockets live in [`super::connection`] and
//! [`super::discovery`].
//!
//! Reference: Yeelight "Inter-Operation Specification" (third-party LAN control).
//! Commands are newline-terminated JSON objects: `{"id":N,"method":M,"params":P}`
//! followed by `\r\n`.

use crate::color::Rgb;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// The transition Yeelight applies between colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Jump immediately. Lowest latency, but can look "steppy".
    Sudden,
    /// Smoothly transition over `duration` ms (min 30 by spec).
    Smooth,
}

impl Effect {
    fn as_str(self) -> &'static str {
        match self {
            Effect::Sudden => "sudden",
            Effect::Smooth => "smooth",
        }
    }
}

/// Yeelight rejects smooth durations below 30 ms.
pub const MIN_SMOOTH_DURATION_MS: u32 = 30;

#[derive(Serialize)]
struct Command<'a> {
    id: u32,
    method: &'a str,
    params: serde_json::Value,
}

fn encode(id: u32, method: &str, params: serde_json::Value) -> String {
    let cmd = Command { id, method, params };
    // Unwrap is safe: the structure is always serialisable.
    let mut s = serde_json::to_string(&cmd).expect("yeelight command serialises");
    s.push_str("\r\n");
    s
}

/// `set_rgb`: set the main light colour. `duration` is only used for
/// [`Effect::Smooth`] and is floored to [`MIN_SMOOTH_DURATION_MS`].
pub fn set_rgb(id: u32, color: Rgb, effect: Effect, duration_ms: u32) -> String {
    let duration = normalise_duration(effect, duration_ms);
    encode(
        id,
        "set_rgb",
        serde_json::json!([color.to_u32(), effect.as_str(), duration]),
    )
}

/// `set_ct_abx`: set a white colour temperature in Kelvin (1700..=6500).
///
/// Unlike [`set_rgb`], this drives the bulb's calibrated white point, so a grey
/// scene renders as an actual neutral white rather than an uncalibrated RGB mix.
pub fn set_ct_abx(id: u32, kelvin: u16, effect: Effect, duration_ms: u32) -> String {
    let k = kelvin.clamp(crate::color::MIN_KELVIN, crate::color::MAX_KELVIN);
    let duration = normalise_duration(effect, duration_ms);
    encode(
        id,
        "set_ct_abx",
        serde_json::json!([k, effect.as_str(), duration]),
    )
}

/// `set_bright`: set brightness (1..=100).
pub fn set_bright(id: u32, brightness: u8, effect: Effect, duration_ms: u32) -> String {
    let b = brightness.clamp(1, 100);
    let duration = normalise_duration(effect, duration_ms);
    encode(
        id,
        "set_bright",
        serde_json::json!([b, effect.as_str(), duration]),
    )
}

/// `set_power`: turn the bulb on or off.
pub fn set_power(id: u32, on: bool, effect: Effect, duration_ms: u32) -> String {
    let duration = normalise_duration(effect, duration_ms);
    encode(
        id,
        "set_power",
        serde_json::json!([if on { "on" } else { "off" }, effect.as_str(), duration]),
    )
}

/// `set_music`: enable (action=1) or disable (action=0) music mode.
///
/// When enabled the bulb opens a TCP connection back to `host:port`, over which
/// we can push commands with **no rate limit** — this is the key to low-latency
/// ambilight. When disabling, host/port are ignored by the bulb.
pub fn set_music(id: u32, enable: bool, host: &str, port: u16) -> String {
    if enable {
        encode(id, "set_music", serde_json::json!([1, host, port]))
    } else {
        encode(id, "set_music", serde_json::json!([0]))
    }
}

fn normalise_duration(effect: Effect, duration_ms: u32) -> u32 {
    match effect {
        Effect::Sudden => 0,
        Effect::Smooth => duration_ms.max(MIN_SMOOTH_DURATION_MS),
    }
}

/// A device found via SSDP discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    /// Unique device id (hex string, e.g. `0x0000000012345678`).
    pub id: String,
    /// IP the bulb is reachable at.
    pub ip: String,
    /// Control port (usually 55443).
    pub port: u16,
    /// Model string, e.g. `color`, `colorc`.
    pub model: String,
    /// User-assigned name (may be empty / URL-encoded by the bulb).
    pub name: String,
    /// Methods the bulb reports supporting (from the `support` header).
    pub support: Vec<String>,
    /// Firmware version, if reported.
    pub fw_ver: Option<String>,
}

/// Parse a raw SSDP `NOTIFY`/`HTTP/1.1 200 OK` response from a Yeelight bulb.
///
/// Returns `None` if the payload doesn't look like a Yeelight advertisement
/// (missing id or a usable `Location`).
pub fn parse_discovery(raw: &str) -> Option<DiscoveredDevice> {
    let mut id = None;
    let mut location = None;
    let mut model = String::new();
    let mut name = String::new();
    let mut support = Vec::new();
    let mut fw_ver = None;

    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "id" => id = Some(value.to_string()),
            "location" => location = Some(value.to_string()),
            "model" => model = value.to_string(),
            "name" => name = value.to_string(),
            "fw_ver" => fw_ver = Some(value.to_string()),
            "support" => {
                support = value.split_whitespace().map(|s| s.to_string()).collect();
            }
            _ => {}
        }
    }

    let id = id?;
    // Location looks like `yeelight://192.168.1.50:55443`.
    let loc = location?;
    let addr = loc.strip_prefix("yeelight://").unwrap_or(&loc);
    let (ip, port) = match addr.rsplit_once(':') {
        Some((ip, port)) => (ip.to_string(), port.parse().ok()?),
        None => (addr.to_string(), DEFAULT_CONTROL_PORT),
    };

    if ip.is_empty() {
        return None;
    }

    Some(DiscoveredDevice {
        id,
        ip,
        port,
        model,
        name,
        support,
        fw_ver,
    })
}

/// The multicast group and port Yeelight uses for SSDP discovery.
pub const SSDP_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
pub const SSDP_PORT: u16 = 1982;
pub const DEFAULT_CONTROL_PORT: u16 = 55443;

/// Build the SSDP `M-SEARCH` datagram that asks Yeelight bulbs to announce.
pub fn ssdp_search_message() -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}:{SSDP_PORT}\r\nMAN: \"ssdp:discover\"\r\nST: wifi_bulb\r\n\r\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_rgb_sudden_has_zero_duration() {
        let s = set_rgb(1, Rgb::new(255, 0, 0), Effect::Sudden, 500);
        assert_eq!(
            s,
            "{\"id\":1,\"method\":\"set_rgb\",\"params\":[16711680,\"sudden\",0]}\r\n"
        );
    }

    #[test]
    fn set_rgb_smooth_floors_duration() {
        let s = set_rgb(7, Rgb::new(0, 255, 0), Effect::Smooth, 10);
        assert_eq!(
            s,
            "{\"id\":7,\"method\":\"set_rgb\",\"params\":[65280,\"smooth\",30]}\r\n"
        );
    }

    #[test]
    fn set_rgb_ends_with_crlf() {
        assert!(set_rgb(1, Rgb::BLACK, Effect::Sudden, 0).ends_with("\r\n"));
    }

    #[test]
    fn set_ct_abx_clamps_kelvin() {
        assert!(set_ct_abx(1, 900, Effect::Sudden, 0).contains("[1700,\"sudden\",0]"));
        assert!(set_ct_abx(1, 9000, Effect::Sudden, 0).contains("[6500,\"sudden\",0]"));
        assert_eq!(
            set_ct_abx(4, 4000, Effect::Smooth, 80),
            "{\"id\":4,\"method\":\"set_ct_abx\",\"params\":[4000,\"smooth\",80]}\r\n"
        );
    }

    #[test]
    fn set_bright_clamps_range() {
        assert!(set_bright(1, 0, Effect::Sudden, 0).contains("[1,\"sudden\",0]"));
        assert!(set_bright(1, 200, Effect::Sudden, 0).contains("[100,\"sudden\",0]"));
    }

    #[test]
    fn set_music_enable_and_disable() {
        assert_eq!(
            set_music(1, true, "192.168.1.5", 40000),
            "{\"id\":1,\"method\":\"set_music\",\"params\":[1,\"192.168.1.5\",40000]}\r\n"
        );
        assert_eq!(
            set_music(2, false, "x", 0),
            "{\"id\":2,\"method\":\"set_music\",\"params\":[0]}\r\n"
        );
    }

    #[test]
    fn set_power_on_off() {
        assert!(set_power(1, true, Effect::Smooth, 300).contains("\"on\",\"smooth\",300"));
        assert!(set_power(1, false, Effect::Sudden, 0).contains("\"off\",\"sudden\",0"));
    }

    #[test]
    fn parse_discovery_extracts_fields() {
        let raw = "HTTP/1.1 200 OK\r\n\
            Cache-Control: max-age=3600\r\n\
            Location: yeelight://192.168.1.239:55443\r\n\
            Server: POSIX UPnP/1.0 YGLC/1\r\n\
            id: 0x0000000002dfd2a4\r\n\
            model: color\r\n\
            fw_ver: 65\r\n\
            support: get_prop set_default set_power toggle set_bright set_rgb set_music\r\n\
            power: off\r\n\
            name: bedroom\r\n";
        let d = parse_discovery(raw).expect("should parse");
        assert_eq!(d.id, "0x0000000002dfd2a4");
        assert_eq!(d.ip, "192.168.1.239");
        assert_eq!(d.port, 55443);
        assert_eq!(d.model, "color");
        assert_eq!(d.name, "bedroom");
        assert_eq!(d.fw_ver.as_deref(), Some("65"));
        assert!(d.support.contains(&"set_music".to_string()));
        assert!(d.support.contains(&"set_rgb".to_string()));
    }

    #[test]
    fn parse_discovery_rejects_non_yeelight() {
        assert!(parse_discovery("HTTP/1.1 200 OK\r\nServer: something\r\n").is_none());
    }

    #[test]
    fn parse_discovery_requires_location() {
        assert!(parse_discovery("id: 0x1\r\nmodel: color\r\n").is_none());
    }

    #[test]
    fn ssdp_message_targets_wifi_bulb() {
        let m = ssdp_search_message();
        assert!(m.contains("ST: wifi_bulb"));
        assert!(m.contains("MAN: \"ssdp:discover\""));
        assert!(m.contains("239.255.255.250:1982"));
    }
}
