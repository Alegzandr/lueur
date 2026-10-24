//! Tuya / Smart Life bulbs: which data points (DPs) to write for a colour.
//!
//! Tuya bulbs are driven by setting numbered DPs. The transport (and the
//! per-device local key it needs) lives in the shell, which uses the
//! `rustuya` crate; this module only turns a [`BulbOutput`] into DP values,
//! which is pure and testable here.
//!
//! Two layouts exist. Current bulbs use DPs 20–28 with 0–1000 ranges, and most
//! of them have a realtime `control_data` DP (28) that takes a colour without
//! touching the saved scene, which is what the app's own music mode uses.
//! Older bulbs use DPs 1–5 with 0–255 ranges.

use super::common::{rgb_to_hsv, scale_bright};
use crate::color::BulbOutput;
use crate::yeelight::Effect;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The local TCP port every Tuya device listens on.
pub const PORT: u16 = 6668;

/// The white range Tuya maps its 0..max temperature onto.
const WARMEST_KELVIN: u16 = 2700;
const COOLEST_KELVIN: u16 = 6500;

/// Where a bulb keeps each setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TuyaDps {
    pub switch: u16,
    pub mode: u16,
    pub bright: u16,
    pub temp: u16,
    pub colour: u16,
    /// The realtime colour DP, when the bulb has one.
    #[serde(default)]
    pub control: Option<u16>,
    /// DPs 1–5 with 0–255 ranges and the older colour string.
    #[serde(default)]
    pub legacy: bool,
}

impl Default for TuyaDps {
    fn default() -> Self {
        TuyaDps {
            switch: 20,
            mode: 21,
            bright: 22,
            temp: 23,
            colour: 24,
            control: Some(28),
            legacy: false,
        }
    }
}

impl TuyaDps {
    /// Build the layout from the device's DP id → status code map, as the
    /// Smart Life cloud reports it. `None` for a device that is not a colour
    /// bulb (a plug, a sensor, a white-only lamp).
    pub fn from_codes(codes: &BTreeMap<u16, String>) -> Option<Self> {
        let find = |names: &[&str]| {
            names.iter().find_map(|n| {
                codes
                    .iter()
                    .find(|(_, c)| c.as_str() == *n)
                    .map(|(&dp, _)| dp)
            })
        };
        let legacy = find(&["colour_data_v2"]).is_none();
        Some(TuyaDps {
            switch: find(&["switch_led"])?,
            mode: find(&["work_mode"])?,
            bright: find(&["bright_value_v2", "bright_value"])?,
            temp: find(&["temp_value_v2", "temp_value"]).unwrap_or(0),
            colour: find(&["colour_data_v2", "colour_data"])?,
            control: if legacy {
                None
            } else {
                find(&["control_data"])
            },
            legacy,
        })
    }

    fn bright_range(&self) -> (u32, u32) {
        if self.legacy { (25, 255) } else { (10, 1000) }
    }

    fn temp_max(&self) -> u32 {
        if self.legacy { 255 } else { 1000 }
    }
}

fn hex4(v: u32) -> String {
    format!("{:04x}", v.min(0xffff))
}

/// Tuya temperature (0 = warmest) for a colour temperature.
fn temp_value(kelvin: u16, max: u32) -> u32 {
    let k = kelvin.clamp(WARMEST_KELVIN, COOLEST_KELVIN) - WARMEST_KELVIN;
    u32::from(k) * max / u32::from(COOLEST_KELVIN - WARMEST_KELVIN)
}

/// The colour string for the colour DP.
fn colour_string(dps: &TuyaDps, rgb: crate::color::Rgb, bright: u8) -> String {
    let (h, s, _) = rgb_to_hsv(rgb);
    let h = (h.round() as u32) % 360;
    if dps.legacy {
        // rrggbb + 0hhh + ss + vv, saturation and value on 0..255.
        let s = (s * 255.0).round() as u32;
        let v = scale_bright(bright, 1, 255);
        format!(
            "{:02x}{:02x}{:02x}{}{s:02x}{v:02x}",
            rgb.r,
            rgb.g,
            rgb.b,
            hex4(h)
        )
    } else {
        let s = (s * 1000.0).round() as u32;
        let v = scale_bright(bright, 10, 1000);
        format!("{}{}{}", hex4(h), hex4(s), hex4(v))
    }
}

fn put(map: &mut Map<String, Value>, dp: u16, v: Value) {
    if dp != 0 {
        map.insert(dp.to_string(), v);
    }
}

/// Whether two outputs need the bulb in different work modes.
pub fn same_mode(a: BulbOutput, b: BulbOutput) -> bool {
    matches!(
        (a, b),
        (BulbOutput::Color { .. }, BulbOutput::Color { .. })
            | (BulbOutput::White { .. }, BulbOutput::White { .. })
    )
}

/// The DPs to write for one frame.
///
/// With a realtime DP, a frame is that one value and the work mode is only
/// sent when `set_mode` (the first frame, or a switch between colour and
/// white). Without one, the mode and saved colour are written each time.
pub fn frame_dps(dps: &TuyaDps, output: BulbOutput, effect: Effect, set_mode: bool) -> Value {
    let mut map = Map::new();
    let mode = match output {
        BulbOutput::Color { .. } => "colour",
        BulbOutput::White { .. } => "white",
    };
    if let Some(control) = dps.control {
        // m + hhhh ssss vvvv + bbbb kkkk; m is 1 to fade, 0 to jump.
        let m = u8::from(effect == Effect::Smooth);
        let value = match output {
            BulbOutput::Color { rgb, bright } => {
                let (h, s, _) = rgb_to_hsv(rgb);
                format!(
                    "{m}{}{}{}{}{}",
                    hex4((h.round() as u32) % 360),
                    hex4((s * 1000.0).round() as u32),
                    hex4(scale_bright(bright, 10, 1000)),
                    hex4(0),
                    hex4(0)
                )
            }
            BulbOutput::White { kelvin, bright } => format!(
                "{m}{}{}{}{}{}",
                hex4(0),
                hex4(0),
                hex4(0),
                hex4(scale_bright(bright, 10, 1000)),
                hex4(temp_value(kelvin, 1000))
            ),
        };
        if set_mode {
            put(&mut map, dps.mode, json!(mode));
        }
        put(&mut map, control, json!(value));
        return Value::Object(map);
    }

    put(&mut map, dps.mode, json!(mode));
    match output {
        BulbOutput::Color { rgb, bright } => {
            put(&mut map, dps.colour, json!(colour_string(dps, rgb, bright)));
        }
        BulbOutput::White { kelvin, bright } => {
            let (lo, hi) = dps.bright_range();
            put(&mut map, dps.bright, json!(scale_bright(bright, lo, hi)));
            put(
                &mut map,
                dps.temp,
                json!(temp_value(kelvin, dps.temp_max())),
            );
        }
    }
    Value::Object(map)
}

/// The DPs that leave the bulb on a steady white or colour, saved in its own
/// state (not the realtime channel), so it holds after Lueur disconnects.
pub fn steady_dps(dps: &TuyaDps, output: BulbOutput) -> Value {
    let plain = TuyaDps {
        control: None,
        ..*dps
    };
    let mut v = frame_dps(&plain, output, Effect::Smooth, true);
    if let Value::Object(map) = &mut v {
        put(map, dps.switch, json!(true));
    }
    v
}

/// The DP that switches the bulb on or off.
pub fn power_dps(dps: &TuyaDps, on: bool) -> Value {
    let mut map = Map::new();
    put(&mut map, dps.switch, json!(on));
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    fn codes(pairs: &[(u16, &str)]) -> BTreeMap<u16, String> {
        pairs.iter().map(|&(k, v)| (k, v.to_string())).collect()
    }

    fn current() -> TuyaDps {
        TuyaDps::from_codes(&codes(&[
            (20, "switch_led"),
            (21, "work_mode"),
            (22, "bright_value_v2"),
            (23, "temp_value_v2"),
            (24, "colour_data_v2"),
            (25, "scene_data_v2"),
            (28, "control_data"),
        ]))
        .unwrap()
    }

    #[test]
    fn current_layout_is_read_from_codes() {
        assert_eq!(current(), TuyaDps::default());
    }

    #[test]
    fn legacy_layout_is_read_from_codes() {
        let dps = TuyaDps::from_codes(&codes(&[
            (1, "switch_led"),
            (2, "work_mode"),
            (3, "bright_value"),
            (4, "temp_value"),
            (5, "colour_data"),
        ]))
        .unwrap();
        assert!(dps.legacy);
        assert_eq!((dps.switch, dps.colour, dps.control), (1, 5, None));
    }

    #[test]
    fn a_plug_is_not_a_bulb() {
        assert!(TuyaDps::from_codes(&codes(&[(1, "switch_1"), (9, "countdown_1")])).is_none());
    }

    #[test]
    fn realtime_frame_is_one_dp() {
        let v = frame_dps(
            &current(),
            BulbOutput::Color {
                rgb: Rgb::new(255, 0, 0),
                bright: 100,
            },
            Effect::Smooth,
            false,
        );
        assert_eq!(v, json!({"28": "1000003e803e800000000"}));
    }

    #[test]
    fn mode_rides_along_when_asked() {
        let v = frame_dps(
            &current(),
            BulbOutput::White {
                kelvin: 6500,
                bright: 1,
            },
            Effect::Sudden,
            true,
        );
        assert_eq!(v, json!({"21": "white", "28": "0000000000000000a03e8"}));
    }

    #[test]
    fn blue_hue_is_240() {
        let v = frame_dps(
            &current(),
            BulbOutput::Color {
                rgb: Rgb::new(0, 0, 255),
                bright: 100,
            },
            Effect::Sudden,
            false,
        );
        assert_eq!(v["28"].as_str().unwrap()[1..5], *"00f0");
    }

    #[test]
    fn steady_state_uses_the_saved_dps() {
        let v = steady_dps(
            &current(),
            BulbOutput::White {
                kelvin: 2700,
                bright: 40,
            },
        );
        assert_eq!(v["20"], true);
        assert_eq!(v["21"], "white");
        assert_eq!(v["23"], 0, "2700 K is Tuya's warmest");
        assert!(v.get("28").is_none());
    }

    #[test]
    fn legacy_colour_string_has_rgb_then_hsv() {
        let dps = TuyaDps {
            switch: 1,
            mode: 2,
            bright: 3,
            temp: 4,
            colour: 5,
            control: None,
            legacy: true,
        };
        let v = frame_dps(
            &dps,
            BulbOutput::Color {
                rgb: Rgb::new(0, 255, 0),
                bright: 100,
            },
            Effect::Smooth,
            false,
        );
        assert_eq!(v["2"], "colour");
        assert_eq!(v["5"], "00ff000078ffff");
    }

    #[test]
    fn power_is_the_switch_dp() {
        assert_eq!(power_dps(&current(), false), json!({"20": false}));
    }
}
