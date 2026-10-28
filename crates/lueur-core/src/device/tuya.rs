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
use crate::show::OwnLoop;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// The local TCP port every Tuya device listens on.
pub const PORT: u16 = 6668;

/// What a Tuya bulb's device id starts with in Lueur; the cloud's own id
/// follows it.
pub const ID_PREFIX: &str = "tuya:";

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
    /// The scene DP, which plays a loop of colours by itself.
    #[serde(default)]
    pub scene: Option<u16>,
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
            scene: Some(25),
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
            scene: if legacy {
                None
            } else {
                find(&["scene_data_v2"])
            },
        })
    }

    /// Where the bulb takes a scene. A layout saved before Lueur knew of
    /// scenes says nothing, but every current bulb with its colour at 24
    /// follows Tuya's standard layout, which keeps the scene at 25.
    fn scene_dp(&self) -> Option<u16> {
        self.scene
            .or((!self.legacy && self.colour == 24).then_some(25))
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
///
/// Realtime frames always jump. The bulb's own fade lasts far longer than the
/// gap between two frames (seconds, not the tenth of a second asked for) and
/// starts over with each frame, so a stream of fading frames leaves the bulb
/// crawling behind every change. The frames themselves already step through
/// the in-between colours, which is what makes the motion look smooth.
pub fn frame_dps(dps: &TuyaDps, output: BulbOutput, set_mode: bool) -> Value {
    let mut map = Map::new();
    let mode = match output {
        BulbOutput::Color { .. } => "colour",
        BulbOutput::White { .. } => "white",
    };
    if let Some(control) = dps.control {
        // m + hhhh ssss vvvv + bbbb kkkk; m is 1 to fade, 0 to jump.
        let m = 0;
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
    let mut v = frame_dps(&plain, output, true);
    if let Value::Object(map) = &mut v {
        put(map, dps.switch, json!(true));
    }
    v
}

/// The most colours a scene holds.
const SCENE_UNITS: usize = 8;
/// The number the scene goes by; the app lists its colourful scenes here.
const SCENE_NUMBER: u8 = 5;

/// The DPs that leave the bulb playing `own` by itself as one of its own
/// scenes, every colour gliding into the next. None for a bulb that takes
/// no scene.
///
/// A scene's pace is not a duration but a speed, 1 to 100: the app offers
/// 40 to 100. A loop's long glides play at 40, its slowest; the quick
/// ones of a flame at 100.
pub fn scene_dps(dps: &TuyaDps, own: &OwnLoop) -> Option<Value> {
    let dp = dps.scene_dp()?;
    let every = own.steps.len().div_ceil(SCENE_UNITS).max(1);
    let units: Vec<_> = own.steps.iter().step_by(every).collect();
    let glide = own.lap_ms() / units.len().max(1) as u32;
    let speed = (40_000 / glide.max(1)).clamp(40, 100);
    let mut value = format!("{SCENE_NUMBER:02x}");
    for step in units {
        // Switch and glide speed, then 02 to glide rather than jump.
        value.push_str(&format!("{speed:02x}{speed:02x}02"));
        let (h, s, v, b, t) = match step.output {
            BulbOutput::Color { rgb, bright } => {
                let (h, s, _) = rgb_to_hsv(rgb);
                (
                    (h.round() as u32) % 360,
                    (s * 1000.0).round() as u32,
                    scale_bright(bright, 10, 1000),
                    0,
                    0,
                )
            }
            BulbOutput::White { kelvin, bright } => (
                0,
                0,
                0,
                scale_bright(bright, 10, 1000),
                temp_value(kelvin, 1000),
            ),
        };
        for x in [h, s, v, b, t] {
            value.push_str(&hex4(x));
        }
    }
    let mut map = Map::new();
    put(&mut map, dps.switch, json!(true));
    put(&mut map, dps.mode, json!("scene"));
    put(&mut map, dp, json!(value));
    Some(Value::Object(map))
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
            false,
        );
        // The leading 0 is a jump: the bulb's own fade would lag the stream.
        assert_eq!(v, json!({"28": "0000003e803e800000000"}));
    }

    #[test]
    fn mode_rides_along_when_asked() {
        let v = frame_dps(
            &current(),
            BulbOutput::White {
                kelvin: 6500,
                bright: 1,
            },
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
            scene: None,
        };
        let v = frame_dps(
            &dps,
            BulbOutput::Color {
                rgb: Rgb::new(0, 255, 0),
                bright: 100,
            },
            false,
        );
        assert_eq!(v["2"], "colour");
        assert_eq!(v["5"], "00ff000078ffff");
    }

    #[test]
    fn a_scene_glides_through_at_most_eight_colours() {
        use crate::show::{ShowKind, ShowSettings};
        let s = ShowSettings {
            kind: ShowKind::Rainbow,
            ..ShowSettings::default()
        };
        let own = ShowKind::Rainbow.own_loop(&s, 0.0).unwrap();
        let v = scene_dps(&current(), &own).unwrap();
        assert_eq!(v["20"], true);
        assert_eq!(v["21"], "scene");
        let scene = v["25"].as_str().unwrap();
        // The number, then 26 hex digits a colour; the first one red.
        assert_eq!(scene.len() % 26, 2);
        assert!((scene.len() - 2) / 26 <= SCENE_UNITS);
        assert_eq!(&scene[..2], "05");
        assert_eq!(&scene[2..8], "282802");
        assert_eq!(&scene[8..16], "000003e8");
    }

    #[test]
    fn a_layout_saved_without_its_scene_still_has_one() {
        let saved = TuyaDps {
            scene: None,
            ..TuyaDps::default()
        };
        assert_eq!(saved.scene_dp(), Some(25));
        let legacy = TuyaDps {
            legacy: true,
            colour: 5,
            scene: None,
            ..TuyaDps::default()
        };
        assert_eq!(legacy.scene_dp(), None);
    }

    #[test]
    fn power_is_the_switch_dp() {
        assert_eq!(power_dps(&current(), false), json!({"20": false}));
    }
}
