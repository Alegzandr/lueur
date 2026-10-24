//! Lights held steady by hand: what one light shows, and the built-in moods.
//!
//! The ambilight decides colours frame by frame; everything here is the other
//! way of using the bulbs — pick a white or a colour and leave it there.

use crate::color::{BulbOutput, MAX_BRIGHT, MAX_KELVIN, MIN_BRIGHT, MIN_KELVIN, Rgb, full_chroma};
use serde::{Deserialize, Serialize};

/// The hue of a steady light: a colour temperature, or a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Tint {
    White { kelvin: u16 },
    Color { r: u8, g: u8, b: u8 },
}

/// What one light shows when the screen is not driving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LightState {
    pub on: bool,
    /// Brightness percentage, 1..=100.
    pub bright: u8,
    pub tint: Tint,
}

impl Default for LightState {
    /// A warm, comfortable white: what most people expect a lamp to be.
    fn default() -> Self {
        LightState {
            on: true,
            bright: 80,
            tint: Tint::White { kelvin: 2700 },
        }
    }
}

impl LightState {
    /// The same state with every value inside what bulbs accept.
    pub fn clamped(self) -> Self {
        LightState {
            on: self.on,
            bright: self.bright.clamp(MIN_BRIGHT, MAX_BRIGHT),
            tint: match self.tint {
                Tint::White { kelvin } => Tint::White {
                    kelvin: kelvin.clamp(MIN_KELVIN, MAX_KELVIN),
                },
                // Black has no hue to show; it means "as dim as it goes".
                Tint::Color { r: 0, g: 0, b: 0 } => Tint::Color {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                c => c,
            },
        }
    }

    /// What to push to the bulb while it is on.
    pub fn output(self) -> BulbOutput {
        let s = self.clamped();
        match s.tint {
            Tint::White { kelvin } => BulbOutput::White {
                kelvin,
                bright: s.bright,
            },
            Tint::Color { r, g, b } => BulbOutput::Color {
                rgb: full_chroma(Rgb::new(r, g, b)),
                bright: s.bright,
            },
        }
    }
}

/// A built-in lighting mood, applied to every light alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mood {
    pub id: &'static str,
    pub kelvin: u16,
    pub bright: u8,
}

impl Mood {
    pub fn state(self) -> LightState {
        LightState {
            on: true,
            bright: self.bright,
            tint: Tint::White {
                kelvin: self.kelvin,
            },
        }
    }
}

/// From the brightest to the dimmest. The interface and the tray name them by
/// `id`, so these ids are part of the contract with both.
pub const MOODS: [Mood; 4] = [
    Mood {
        id: "daylight",
        kelvin: 5500,
        bright: 100,
    },
    Mood {
        id: "reading",
        kelvin: 4000,
        bright: 100,
    },
    Mood {
        id: "relax",
        kelvin: 2700,
        bright: 60,
    },
    Mood {
        id: "night",
        kelvin: 2200,
        bright: 8,
    },
];

pub fn mood(id: &str) -> Option<Mood> {
    MOODS.iter().copied().find(|m| m.id == id)
}

/// Id of the mood that follows the time of day rather than holding one white.
/// Not in [`MOODS`]: what it shows is [`natural_at`] the current time.
pub const NATURAL: &str = "natural";

/// The natural light's course through a day, as (minute of the day, kelvin,
/// brightness): a warm, low glow at night, a cool full white through the
/// working day, and a slow slide back to warm across the evening.
const NATURAL_DAY: [(u16, u16, u8); 8] = [
    (0, 2200, 25),
    (6 * 60, 2200, 25),
    (8 * 60, 4000, 80),
    (10 * 60, 5000, 100),
    (16 * 60, 5000, 100),
    (19 * 60, 3000, 80),
    (21 * 60 + 30, 2400, 50),
    (24 * 60, 2200, 25),
];

/// What the natural light shows `minute` minutes after local midnight. The
/// white moves in steps of 50 K so a bulb is only re-sent a visible change.
pub fn natural_at(minute: u16) -> LightState {
    let minute = minute % (24 * 60);
    let i = NATURAL_DAY
        .windows(2)
        .position(|w| minute < w[1].0)
        .unwrap_or(NATURAL_DAY.len() - 2);
    let (m0, k0, b0) = NATURAL_DAY[i];
    let (m1, k1, b1) = NATURAL_DAY[i + 1];
    let t = f32::from(minute - m0) / f32::from(m1 - m0);
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let kelvin = (lerp(f32::from(k0), f32::from(k1)) / 50.0).round() as u16 * 50;
    LightState {
        on: true,
        bright: lerp(f32::from(b0), f32::from(b1)).round() as u8,
        tint: Tint::White { kelvin },
    }
    .clamped()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_serialise_with_a_mode_tag() {
        let s = LightState {
            on: true,
            bright: 40,
            tint: Tint::Color {
                r: 255,
                g: 0,
                b: 10,
            },
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"on":true,"bright":40,"tint":{"mode":"color","r":255,"g":0,"b":10}}"#
        );
        assert_eq!(serde_json::from_str::<LightState>(&json).unwrap(), s);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let s = LightState {
            on: true,
            bright: 0,
            tint: Tint::White { kelvin: 20000 },
        };
        assert_eq!(
            s.output(),
            BulbOutput::White {
                kelvin: MAX_KELVIN,
                bright: MIN_BRIGHT
            }
        );
    }

    #[test]
    fn a_dim_colour_is_sent_at_full_chroma() {
        let s = LightState {
            on: true,
            bright: 50,
            tint: Tint::Color { r: 60, g: 30, b: 0 },
        };
        match s.output() {
            BulbOutput::Color { rgb, bright } => {
                assert_eq!(rgb.r, 255);
                assert_eq!(bright, 50);
            }
            other => panic!("expected a colour, got {other:?}"),
        }
    }

    #[test]
    fn moods_are_found_by_id() {
        assert_eq!(mood("night").map(|m| m.kelvin), Some(2200));
        assert!(mood("disco").is_none());
        for m in MOODS {
            assert!((MIN_KELVIN..=MAX_KELVIN).contains(&m.kelvin), "{}", m.id);
        }
        assert!(mood(NATURAL).is_none());
    }

    fn white(s: LightState) -> (u16, u8) {
        match s.tint {
            Tint::White { kelvin } => (kelvin, s.bright),
            other => panic!("expected a white, got {other:?}"),
        }
    }

    #[test]
    fn natural_light_is_cool_by_day_and_warm_at_night() {
        assert_eq!(white(natural_at(12 * 60)), (5000, 100));
        assert_eq!(white(natural_at(2 * 60)), (2200, 25));
        assert_eq!(white(natural_at(0)), white(natural_at(24 * 60 - 1)));
        // Halfway through the evening slide.
        assert_eq!(white(natural_at(17 * 60 + 30)), (4000, 90));
    }

    #[test]
    fn natural_light_never_jumps() {
        for m in 1..24 * 60 {
            let (k0, b0) = white(natural_at(m - 1));
            let (k1, b1) = white(natural_at(m));
            assert!(k0.abs_diff(k1) <= 50, "kelvin jumps at minute {m}");
            assert!(b0.abs_diff(b1) <= 1, "brightness jumps at minute {m}");
        }
    }
}
