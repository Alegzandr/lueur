//! Light shows that need no screen: a rainbow, a sunset, the sea, a candle,
//! and lights that dance to the music (the music has a module of its own,
//! [`crate::stage`]).
//!
//! Everything here is pure: given the time step (and, for music, the sound
//! levels the shell measured) a [`Show`] says what every light shows. The
//! shell streams the result to the bulbs exactly as it streams the ambilight.
//!
//! Each show is written for real bulbs in a real room rather than for a
//! screen: movement is continuous (no colour ever "holds" then lurches on),
//! neighbouring lights stay close enough on the wheel that their light still
//! mixes into a colour on the walls instead of washing out to white, and the
//! hues the eye finds brightest are held back a little so a show keeps an
//! even level as it travels.

mod ambient;
mod colour;
mod palettes;
#[cfg(test)]
mod tests;

pub use ambient::Candle;
pub use colour::blend;
pub(crate) use colour::{even, output};
pub(crate) use palettes::{PARTY, SOFT};

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT};
use crate::stage::{Fixture, Levels, Stage};
use ambient::{candle_loop, ocean, ocean_loop, places, rainbow, rainbow_loop, sunset, sunset_loop};
use serde::{Deserialize, Serialize};

/// The light shows. Each is a finished recipe: beyond how bright it plays,
/// the only thing to tune is how fast the colour shows travel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShowKind {
    /// The lights play the track like a stage: punches, chases and drops.
    #[default]
    Music,
    /// Combinations of colours that go well together, laid across the room
    /// and gliding one into the next.
    Rainbow,
    /// Ambers, reds and roses drifting into one another.
    Sunset,
    /// A swell of blues and teals rolling from light to light.
    Ocean,
    /// A warm flame on every light, flickering on its own.
    Candle,
}

pub const SHOWS: [ShowKind; 5] = [
    ShowKind::Music,
    ShowKind::Rainbow,
    ShowKind::Sunset,
    ShowKind::Ocean,
    ShowKind::Candle,
];

impl ShowKind {
    pub fn id(self) -> &'static str {
        match self {
            ShowKind::Music => "music",
            ShowKind::Rainbow => "rainbow",
            ShowKind::Sunset => "sunset",
            ShowKind::Ocean => "ocean",
            ShowKind::Candle => "candle",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        SHOWS.into_iter().find(|k| k.id() == id)
    }

    /// How often the show computes a frame, in milliseconds. Music has to
    /// land on the beat; everything else moves slowly enough that 25 frames a
    /// second is smooth to the eye and well inside every family's budget.
    pub fn frame_ms(self) -> u64 {
        match self {
            ShowKind::Music => 25,
            _ => 40,
        }
    }

    /// Whether the show's pace can be chosen. Music keeps time with the
    /// track and a candle with a real flame; the others are free to drift.
    pub fn has_speed(self) -> bool {
        matches!(self, ShowKind::Rainbow | ShowKind::Sunset | ShowKind::Ocean)
    }

    /// What a bulb can play round and round by itself in place of this
    /// show, the way its own app would, for a light at `place` (0 at the
    /// left of the lights playing it, 1 at the right): the show goes on
    /// once Lueur has quit. None for the music, which needs Lueur to hear.
    pub fn own_loop(self, settings: &ShowSettings, place: f32) -> Option<OwnLoop> {
        let (bright, pace) = (settings.bright(), settings.pace());
        let steps = match self {
            ShowKind::Music => return None,
            ShowKind::Rainbow => rainbow_loop(place, bright, pace),
            ShowKind::Sunset => sunset_loop(place, bright, pace),
            ShowKind::Ocean => ocean_loop(place, bright, pace),
            ShowKind::Candle => candle_loop(place, bright),
        };
        let pace = if self.has_speed() { pace } else { 1.0 };
        Some(OwnLoop {
            kind: self,
            steps,
            pace,
        })
    }
}

/// A show as a bulb plays it by itself (see [`ShowKind::own_loop`]). A bulb
/// that takes a loop of colours plays `steps`; one that only has scenes of
/// its own picks the one closest to `kind`.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnLoop {
    pub kind: ShowKind,
    pub steps: Vec<Step>,
    /// How many times faster than the show's natural pace it plays (see
    /// [`ShowSettings::pace`]), for a scene that takes a speed.
    pub pace: f32,
}

impl OwnLoop {
    /// How long one round takes, in milliseconds.
    pub fn lap_ms(&self) -> u32 {
        self.steps.iter().map(|s| s.ms).sum()
    }

    /// The brightest the loop gets, 1..=100.
    pub fn bright(&self) -> u8 {
        self.steps
            .iter()
            .map(|s| s.output.bright())
            .max()
            .unwrap_or(MAX_BRIGHT)
    }

    /// The same loop shown on `wall` (see [`BulbOutput::on_wall`]).
    pub fn on_wall(mut self, wall: crate::color::Rgb) -> Self {
        for step in &mut self.steps {
            step.output = step.output.on_wall(wall);
        }
        self
    }
}

/// One leg of a loop a bulb plays by itself: a glide to `output` over `ms`
/// milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub output: BulbOutput,
    pub ms: u32,
}

/// The show the window and the tray start, how bright it plays, how fast the
/// colour shows move, and whether the music may flash white.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowSettings {
    pub kind: ShowKind,
    /// 1..=100.
    pub bright: u8,
    /// 1..=100; 50 is each show's natural pace.
    pub speed: u8,
    /// The music may flash white on its drops and the snares just after,
    /// never in a gentle track and never more than three times a second: the
    /// show's guard ([`crate::safety::FlashGuard::show`])
    /// holds them to that even with the protection off.
    pub flashes: bool,
    /// How late the sound reaches the ear after the computer plays it,
    /// milliseconds, for the music to wait on: Bluetooth headphones take a
    /// fifth of a second or so, a TV's speakers can too. Below zero the
    /// lights go earlier still.
    pub delay_ms: i16,
}

impl Default for ShowSettings {
    fn default() -> Self {
        ShowSettings {
            kind: ShowKind::default(),
            bright: 80,
            speed: 50,
            flashes: true,
            delay_ms: 0,
        }
    }
}

/// The range of [`ShowSettings::delay_ms`].
pub const DELAY_MS: std::ops::RangeInclusive<i16> = -150..=500;

impl ShowSettings {
    pub(crate) fn bright(&self) -> f32 {
        f32::from(self.bright.clamp(MIN_BRIGHT, MAX_BRIGHT)) / 100.0
    }

    /// [`ShowSettings::delay_ms`] in seconds, within [`DELAY_MS`].
    pub fn delay(&self) -> f32 {
        f32::from(self.delay_ms.clamp(*DELAY_MS.start(), *DELAY_MS.end())) / 1000.0
    }

    /// How many times faster than its natural pace a show plays: a quarter
    /// at the slowest, four times at the fastest. Even steps along the
    /// slider feel even, since the eye judges pace by ratio.
    pub fn pace(&self) -> f32 {
        let s = f32::from(self.speed.clamp(1, 100));
        4f32.powf((s - 50.0) / 50.0)
    }
}

/// The shows' clocks go back to zero after this many seconds: an `f32` that
/// kept counting would round each small step away after a few days. A whole
/// number of loops through the combinations and of ocean swells, so those
/// two never jump.
pub(super) const CLOCK_WRAP: f32 = 3600.0;

/// One running show, whatever its kind.
#[derive(Debug, Clone)]
pub struct Show {
    kind: ShowKind,
    lights: usize,
    t: f32,
    /// Where each light stands across the room, for the rainbow.
    places: Vec<f32>,
    candle: Candle,
    stage: Stage,
    /// What [`Show::sharp`] answers for the shows that only ever glide.
    smooth: Vec<bool>,
}

impl Show {
    /// A show on these lights, in the order its frames list them.
    pub fn new(kind: ShowKind, fixtures: &[Fixture]) -> Self {
        let lights = fixtures.len();
        Show {
            kind,
            lights,
            t: 0.0,
            places: places(fixtures),
            candle: Candle::new(lights),
            stage: Stage::new(fixtures),
            smooth: vec![false; lights],
        }
    }

    /// Advance by `dt` seconds. `levels` is what the music show listens to;
    /// the others ignore it.
    ///
    /// The colour shows run on their own clock, ticking at the chosen pace,
    /// so turning the speed up or down mid-show changes how fast the colours
    /// move from where they are rather than making them leap.
    pub fn frame(&mut self, settings: &ShowSettings, dt: f32, levels: Levels) -> Vec<BulbOutput> {
        if self.kind.has_speed() {
            self.t = (self.t + dt * settings.pace()) % CLOCK_WRAP;
        }
        let (t, n, bright) = (self.t, self.lights, settings.bright());
        match self.kind {
            ShowKind::Music => self.stage.frame(settings, levels, dt),
            ShowKind::Candle => self.candle.frame(settings, dt),
            ShowKind::Rainbow => self.places.iter().map(|&p| rainbow(t, p, bright)).collect(),
            ShowKind::Sunset => (0..n).map(|i| sunset(t, i, n, bright)).collect(),
            ShowKind::Ocean => (0..n).map(|i| ocean(t, i, n, bright)).collect(),
        }
    }

    /// Which lights the last frame should reach sharp rather than faded in:
    /// the ones a beat just struck.
    pub fn sharp(&self) -> &[bool] {
        if self.kind == ShowKind::Music {
            self.stage.sharp()
        } else {
            &self.smooth
        }
    }

    #[doc(hidden)]
    pub fn debug_section(&self) -> &'static str {
        self.stage.debug_section()
    }

    #[doc(hidden)]
    pub fn debug_look(&self) -> String {
        self.stage.debug_look()
    }

    /// How early the music's beats should go out (see [`Stage::lead`]).
    pub fn lead(&self) -> f32 {
        self.stage.lead()
    }

    /// Seconds until light `i` is due something sharp (see
    /// [`Stage::due`]); never, for the shows that only glide.
    pub fn due(&self, i: usize) -> Option<f32> {
        (self.kind == ShowKind::Music)
            .then(|| self.stage.due(i))
            .flatten()
    }

    /// Seconds until anything sharp is due on any light.
    pub fn next_due(&self) -> Option<f32> {
        (self.kind == ShowKind::Music)
            .then(|| self.stage.next_due())
            .flatten()
    }
}
