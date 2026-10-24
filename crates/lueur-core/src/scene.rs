//! Lights held steady by hand: what one light shows, the built-in moods, and
//! the scenes the user saves.
//!
//! The ambilight decides colours frame by frame; everything here is the other
//! way of using the bulbs — pick a white or a colour and leave it there.

use crate::color::{BulbOutput, MAX_BRIGHT, MAX_KELVIN, MIN_BRIGHT, MIN_KELVIN, Rgb, full_chroma};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// A snapshot of the lights the user saved under a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    /// Keyed by room light id. Lights added since the scene was saved are
    /// left as they are when it is applied.
    #[serde(default)]
    pub lights: BTreeMap<String, LightState>,
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
    }
}
