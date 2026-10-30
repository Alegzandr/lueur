//! Keeping the lights safe to look at, whatever plays them.
//!
//! A [`FlashGuard`] stands between every mode (the ambilight, the shows, the
//! music) and the bulbs. It lets through anything that changes slowly, and
//! anything fast that does not come back: a cut to a new scene, a light
//! switched off. What it holds back is flashing: a light swinging hard one
//! way and back, more than three times a second.
//!
//! The measure follows WCAG 2.3.1 ("Three Flashes or Below Threshold"),
//! made for a room rather than a screen:
//!
//! * A *transition* is a change of a tenth or more of the light's range
//!   against the way it was last moving, with the darker side under 0.8;
//!   a *flash* is two of them. At most six transitions in any one second, so
//!   three flashes, per light.
//! * Two signals are watched, each with its own budget: how strongly the light
//!   lights the room (its brightness, weighted by how bright its colour looks),
//!   and how much saturated red it shows, which WCAG treats on its own.
//! * A bulb fills the field of view, so no hue counts for less than
//!   [`FLOOR`] of a white at the same power: a full blue strobing in a dark
//!   room is a flash, even though a screen's blue would barely register.
//!
//! Once a light has used its budget, it never completes another transition
//! until the oldest has left the window, and it moves gently ([`EASE`] of its
//! range a second at most) until it has been calm for a second: the flashing
//! becomes a slow drift, never a jump.
//!
//! The ambilight also caps how fast a light may brighten: over a tenth of a
//! second ([`FIRST_RISE`]) from a steady picture, so a flashbang in a game
//! still strikes, and over [`RISE`] once the light has already swung lately,
//! so a picture that keeps flashing swells rather than whites out the room
//! each time. Dimming stays immediate.
//!
//! The room keeps a budget too. In a show, every light's transitions count
//! towards it. In the ambilight only alternations do, lights turning opposite
//! ways at once: a siren flashing red left and right, each side within its
//! own budget, flashes the room twice as often. A cut, or something moving
//! across the picture now and then, stays free.
//!
//! Turned off, the guard lets everything through, with one exception: a show
//! still holds its white flashes (a third signal, how much near-white light
//! a light gives) to the same budget. The music has always promised never
//! to flash white more than three times a second, and that promise does not
//! depend on the setting.

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT, Rgb, full_chroma};
use crate::show::blend;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// The photosensitivity protection, on unless the user turns it off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetySettings {
    /// No light flashes more than three times a second, nor the room, and
    /// the ambilight brightens over a tenth of a second at least.
    pub flash_guard: bool,
}

impl Default for SafetySettings {
    fn default() -> Self {
        SafetySettings { flash_guard: true }
    }
}

/// The least change, as a share of the light's range, that counts as a
/// transition.
const STEP: f32 = 0.1;
/// A swing whose darker side is at least this bright is no flash.
const DARK: f32 = 0.8;
/// Transitions allowed per light and signal in any [`WINDOW`]: three flashes.
const TRANSITIONS: usize = 6;
/// The sliding window flashes are counted over, seconds.
const WINDOW: f64 = 1.0;
/// Lights turning the same way this close together are one change of the
/// room, seconds: a beat that strikes every light at once is one flash.
const MERGE: f64 = 0.1;
/// How fast a light that used its budget may move, share of its range a
/// second: across the whole range in half a second.
pub const EASE: f32 = 2.0;
/// How fast the ambilight may brighten a light when protected and it has
/// turned [`BUSY`] times lately, share of its range a second: from dark to
/// full in 0.28 s.
pub const RISE: f32 = 1.0 / 0.28;
/// How fast it may brighten one that has been steady: from dark to full in a
/// tenth of a second, quick enough for a shot or an explosion to land.
pub const FIRST_RISE: f32 = 1.0 / 0.1;
/// Transitions of what a light is asked to show, within the last
/// [`WINDOW`], after which it brightens at [`RISE`]: the picture has flashed
/// once already. Counted on what is asked rather than what is sent, since
/// the cap itself keeps what is sent from turning.
const BUSY: usize = 2;
/// The least a colour counts for, against a white at the same power.
pub const FLOOR: f32 = 0.3;
/// What one brightness percent is worth: the finest step a bulb takes.
const QUANTUM: f32 = 0.01;

/// An sRGB channel in linear light.
fn linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A white of `kelvin` as an RGB colour at full chroma (Tanner Helland's
/// fit): what it looks like, for the measure and for gliding between a white
/// and a colour.
fn kelvin_rgb(kelvin: u16) -> Rgb {
    let t = f32::from(kelvin) / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.7 * (t - 60.0).powf(-0.133_204_76)
    };
    let g = if t <= 66.0 {
        99.470_8 * t.ln() - 161.119_57
    } else {
        288.122_16 * (t - 60.0).powf(-0.075_514_85)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    let to = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    full_chroma(Rgb::new(to(r), to(g), to(b)))
}

/// The hue an output shows, at full chroma.
fn chroma(o: BulbOutput) -> Rgb {
    match o {
        BulbOutput::Color { rgb, .. } => full_chroma(rgb),
        BulbOutput::White { kelvin, .. } => kelvin_rgb(kelvin),
    }
}

/// How strongly `o` lights the room (0..=1), and how much saturated red it
/// shows (0..=1).
///
/// The first is the brightness weighted by the colour's relative luminance,
/// never under [`FLOOR`]. The second is WCAG's red measure (red over green
/// and blue, in linear light) for colours that are mostly red, eased in
/// between a red share of 0.7 and 0.9 so that a hue drifting across the line
/// is never a jump.
pub(crate) fn signals(o: BulbOutput) -> (f32, f32) {
    let level = f32::from(o.bright().min(MAX_BRIGHT)) / f32::from(MAX_BRIGHT);
    let c = chroma(o);
    let (r, g, b) = (linear(c.r), linear(c.g), linear(c.b));
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let sum = r + g + b;
    let share = if sum > 0.0 { r / sum } else { 0.0 };
    let red = ((share - 0.7) / 0.2).clamp(0.0, 1.0) * (r - g - b).max(0.0);
    (level * (FLOOR + (1.0 - FLOOR) * y), level * red)
}

/// How much near-white light `o` gives (0..=1): its brightness, for a
/// colour pale enough to read as white, fading out between an HSV saturation
/// of 0.1 and 0.35 so that a colour paling towards white is never a jump.
fn glare(o: BulbOutput) -> f32 {
    let level = f32::from(o.bright().min(MAX_BRIGHT)) / f32::from(MAX_BRIGHT);
    level * ((0.35 - crate::color::saturation(chroma(o))) / 0.25).clamp(0.0, 1.0)
}

/// Which signals a guard holds to the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Watch {
    /// Every flash (the protection is on).
    All,
    /// White flashes only (a show with the protection off).
    Whites,
}

/// What a light is held to this frame.
#[derive(Clone, Copy)]
struct Rules<'a> {
    watch: Watch,
    /// How fast it may brighten, if capped.
    rise: Option<f32>,
    /// The room's budgets, when the room keeps one.
    room: Option<&'a Room>,
}

/// The same output at another brightness.
fn with_bright(o: BulbOutput, bright: u8) -> BulbOutput {
    let bright = bright.clamp(MIN_BRIGHT, MAX_BRIGHT);
    match o {
        BulbOutput::Color { rgb, .. } => BulbOutput::Color { rgb, bright },
        BulbOutput::White { kelvin, .. } => BulbOutput::White { kelvin, bright },
    }
}

/// `t` of the way from `a` to `b`. Colours travel round the wheel (see
/// [`blend`]); a white and a colour meet through the white's own tint.
fn mix(a: BulbOutput, b: BulbOutput, t: f32) -> BulbOutput {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    let bright = lerp(f32::from(a.bright()), f32::from(b.bright())).round() as u8;
    match (a, b) {
        (BulbOutput::White { kelvin: ka, .. }, BulbOutput::White { kelvin: kb, .. }) => {
            BulbOutput::White {
                kelvin: lerp(f32::from(ka), f32::from(kb)).round() as u16,
                bright: bright.clamp(MIN_BRIGHT, MAX_BRIGHT),
            }
        }
        _ => with_bright(
            BulbOutput::Color {
                rgb: full_chroma(blend(chroma(a), chroma(b), t)),
                bright,
            },
            bright,
        ),
    }
}

/// The turns one signal of one light has taken lately.
#[derive(Debug, Clone, Default)]
struct Swings {
    /// The furthest the signal has gone since it last turned (or where it
    /// started, before it first moved a [`STEP`]).
    ext: f32,
    /// +1 rising, -1 falling, 0 before its first transition.
    dir: i8,
    seeded: bool,
    /// When the recent transitions happened, oldest first.
    at: VecDeque<f64>,
}

impl Swings {
    /// The direction of the transition moving to `v` would complete, if any.
    fn turns(&self, v: f32) -> Option<i8> {
        if !self.seeded {
            return None;
        }
        let up = v - self.ext;
        if up >= STEP && self.dir <= 0 && self.ext < DARK {
            Some(1)
        } else if -up >= STEP && self.dir >= 0 && v < DARK {
            Some(-1)
        } else {
            None
        }
    }

    /// Whether moving to `v` at `now` stays within the budget, the room's
    /// too when it keeps one (`lamp` is which light this is).
    fn allows(&self, v: f32, now: f64, room: Option<&Crowd>, lamp: usize) -> bool {
        let Some(dir) = self.turns(v) else {
            return true;
        };
        self.recent(now) < TRANSITIONS && room.is_none_or(|room| room.allows(dir, now, lamp))
    }

    /// How many transitions it took in the last [`WINDOW`].
    fn recent(&self, now: f64) -> usize {
        self.at.iter().filter(|&&t| now - t < WINDOW).count()
    }

    /// The signal is now `v`. The transition it completed, if any.
    fn see(&mut self, v: f32, now: f64) -> Option<i8> {
        while self.at.front().is_some_and(|&t| now - t >= WINDOW) {
            self.at.pop_front();
        }
        if !self.seeded {
            self.ext = v;
            self.seeded = true;
            return None;
        }
        let turned = self.turns(v);
        match turned {
            Some(dir) => {
                self.dir = dir;
                self.ext = v;
                self.at.push_back(now);
            }
            None if (self.dir > 0 && v > self.ext) || (self.dir < 0 && v < self.ext) => {
                self.ext = v;
            }
            None => {}
        }
        turned
    }
}

/// The turns one signal has taken across the whole room.
///
/// Lights that flash one after another each stay within their own budget,
/// yet the room flashes as often as all of them together: a white rolling
/// from light to light eight times a second is eight flashes a second. So
/// the room keeps a budget of its own, the same three flashes a second, as
/// three rises and three falls; lights turning the same way within
/// [`MERGE`] of each other count once.
#[derive(Debug, Clone, Default)]
struct Crowd {
    /// When the room's recent transitions happened, and which way.
    at: VecDeque<(f64, i8)>,
    /// Only alternations count (the ambilight): a light's transition counts
    /// when another light turned the other way within [`MERGE`].
    alternations: bool,
    /// Every light's transitions within the last [`MERGE`], and whose: what
    /// tells an alternation.
    turns: VecDeque<(f64, i8, usize)>,
}

impl Crowd {
    fn alternating() -> Self {
        Crowd {
            alternations: true,
            ..Crowd::default()
        }
    }

    /// The room is turning `dir` already: another light joining it is the
    /// same change.
    fn joins(&self, dir: i8, now: f64) -> bool {
        self.at.iter().any(|&(t, d)| d == dir && now - t < MERGE)
    }

    /// Whether light `lamp` turning `dir` now counts towards the room's
    /// budget.
    fn counts(&self, dir: i8, now: f64, lamp: usize) -> bool {
        !self.alternations
            || self
                .turns
                .iter()
                .any(|&(t, d, l)| d == -dir && l != lamp && now - t < MERGE)
    }

    fn allows(&self, dir: i8, now: f64, lamp: usize) -> bool {
        !self.counts(dir, now, lamp)
            || self.joins(dir, now)
            || self
                .at
                .iter()
                .filter(|&&(t, d)| d == dir && now - t < WINDOW)
                .count()
                < TRANSITIONS / 2
    }

    fn see(&mut self, dir: Option<i8>, now: f64, lamp: usize) {
        while self.at.front().is_some_and(|&(t, _)| now - t >= WINDOW) {
            self.at.pop_front();
        }
        while self
            .turns
            .front()
            .is_some_and(|&(t, _, _)| now - t >= MERGE)
        {
            self.turns.pop_front();
        }
        let Some(dir) = dir else {
            return;
        };
        if self.counts(dir, now, lamp) && !self.joins(dir, now) {
            self.at.push_back((now, dir));
        }
        if self.alternations {
            self.turns.push_back((now, dir, lamp));
        }
    }
}

/// The room's budgets, one per signal.
#[derive(Debug, Clone, Default)]
struct Room {
    general: Crowd,
    red: Crowd,
    white: Crowd,
}

impl Room {
    /// A room where only alternations between lights count (see [`Crowd`]).
    fn alternating() -> Self {
        Room {
            general: Crowd::alternating(),
            red: Crowd::alternating(),
            white: Crowd::alternating(),
        }
    }
}

/// One light's history.
#[derive(Debug, Clone, Default)]
struct Lamp {
    /// What it was last sent.
    shown: Option<BulbOutput>,
    general: Swings,
    red: Swings,
    white: Swings,
    /// The turns of what it is asked to show, before any holding back: how
    /// steady the picture behind it is.
    asked: Swings,
    /// When it was last held back. For a [`WINDOW`] after, it moves at
    /// [`EASE`] whatever its budget, so a strobe that carries on becomes a
    /// drift rather than bursts of flashes each time the budget comes back.
    held_at: Option<f64>,
    /// The last output was not the one asked for.
    held: bool,
    /// Which light of the guard's it is, for the room's budget.
    index: usize,
}

impl Lamp {
    fn allows(&self, o: BulbOutput, now: f64, watch: Watch, room: Option<&Room>) -> bool {
        let i = self.index;
        let white = self.white.allows(glare(o), now, room.map(|r| &r.white), i);
        if watch == Watch::Whites {
            return white;
        }
        let (general, red) = signals(o);
        white
            && self
                .general
                .allows(general, now, room.map(|r| &r.general), i)
            && self.red.allows(red, now, room.map(|r| &r.red), i)
    }

    /// What to send instead of `target`, coming from `prev`, `dt` seconds on.
    fn limit(
        &mut self,
        prev: BulbOutput,
        target: BulbOutput,
        dt: f32,
        now: f64,
        rules: Rules,
    ) -> BulbOutput {
        let Rules { watch, rise, room } = rules;
        let (from, from_red) = signals(prev);
        let from_white = glare(prev);
        let mut want = target;
        if let Some(rate) = rise {
            // Brighten no faster than `rate`, keeping the hue: the colour
            // still answers at once, only its glare is eased in. A light
            // whose picture has been steady may rise faster (see
            // [`FIRST_RISE`]).
            let rate = if self.asked.recent(now) >= BUSY {
                rate
            } else {
                rate.max(FIRST_RISE)
            };
            let cap = from + rate * dt;
            let (to, _) = signals(want);
            if to > cap + QUANTUM {
                let scale = cap.max(0.0) / to;
                let bright = (f32::from(want.bright()) * scale).floor() as u8;
                want = with_bright(want, bright);
            }
        }
        let easing = self.held_at.is_some_and(|t| now - t < WINDOW);
        if !easing && self.allows(want, now, watch, room) {
            return want;
        }
        // Over budget: the largest step towards it that neither completes a
        // transition nor moves faster than `EASE`.
        let pace = EASE * dt + QUANTUM;
        let fits = |o: BulbOutput| {
            let (g, r) = signals(o);
            let steady = match watch {
                Watch::All => (g - from).abs() <= pace && (r - from_red).abs() <= pace,
                Watch::Whites => true,
            };
            steady && (glare(o) - from_white).abs() <= pace && self.allows(o, now, watch, room)
        };
        let sent = if fits(want) {
            want
        } else {
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..12 {
                let mid = 0.5 * (lo + hi);
                if fits(mix(prev, want, mid)) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            mix(prev, want, lo)
        };
        if sent != want {
            self.held_at = Some(now);
        }
        sent
    }

    fn see(&mut self, o: BulbOutput, now: f64, room: &mut Room) {
        let (general, red) = signals(o);
        let i = self.index;
        room.general.see(self.general.see(general, now), now, i);
        room.red.see(self.red.see(red, now), now, i);
        room.white.see(self.white.see(glare(o), now), now, i);
        self.shown = Some(o);
    }
}

/// Holds every light of one running mode under the flash budget.
///
/// Keep one per loop, and pass it every frame's outputs, protected or not:
/// it watches even while turned off, so turning it on mid-show protects at
/// once.
#[derive(Debug, Clone)]
pub struct FlashGuard {
    lamps: Vec<Lamp>,
    /// How fast a light may brighten, for the ambilight.
    rise: Option<f32>,
    /// A show: white flashes are held to the budget even with the
    /// protection off.
    show: bool,
    /// Whether the room keeps a budget of its own (see [`Crowd`]).
    crowd: bool,
    /// Boxed: a guard is kept by value beside every light set by hand.
    room: Box<Room>,
    /// Seconds since the guard started.
    now: f64,
}

impl FlashGuard {
    /// A guard that holds flashes to the budget while the protection is on,
    /// and lets everything through while it is off: for lights set by hand.
    pub fn new(lights: usize) -> Self {
        FlashGuard {
            lamps: vec![Lamp::default(); lights],
            rise: None,
            show: false,
            crowd: false,
            room: Box::default(),
            now: 0.0,
        }
    }

    /// A guard for a show: rises are free (the music's punches are the show),
    /// the room as a whole keeps the budget as well as each light, and white
    /// flashes stay within both even with the protection off.
    pub fn show(lights: usize) -> Self {
        FlashGuard {
            show: true,
            crowd: true,
            ..FlashGuard::new(lights)
        }
    }

    /// A guard for the ambilight: the flash budget, brightening capped (see
    /// [`FIRST_RISE`] and [`RISE`]), and a room budget for lights that
    /// alternate.
    pub fn ambilight(lights: usize) -> Self {
        FlashGuard {
            rise: Some(RISE),
            crowd: true,
            room: Box::new(Room::alternating()),
            ..FlashGuard::new(lights)
        }
    }

    /// Hold `outputs`, the frame about to be sent, `dt` seconds after the last
    /// one; `on` is the user's setting.
    pub fn apply(&mut self, outputs: &mut [BulbOutput], dt: f32, on: bool) {
        // A stall is not time the eye had to rest: count it short.
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.25)
        } else {
            0.0
        };
        self.now += f64::from(dt);
        if self.lamps.len() < outputs.len() {
            self.lamps.resize(outputs.len(), Lamp::default());
        }
        let now = self.now;
        // The ambilight's room counts only alternations (see [`Crowd`]): its
        // lights mirror their own parts of the picture, and counting their
        // ordinary, unrelated changes would hold them back.
        let FlashGuard {
            lamps,
            rise,
            show,
            crowd,
            room,
            ..
        } = self;
        for (i, (lamp, out)) in lamps.iter_mut().zip(outputs.iter_mut()).enumerate() {
            lamp.index = i;
            let target = *out;
            lamp.asked.see(signals(target).0, now);
            let crowd = crowd.then_some(&**room);
            let sent = match lamp.shown {
                Some(prev) if on => {
                    let rules = Rules {
                        watch: Watch::All,
                        rise: *rise,
                        room: crowd,
                    };
                    lamp.limit(prev, target, dt, now, rules)
                }
                Some(prev) if *show => {
                    let rules = Rules {
                        watch: Watch::Whites,
                        rise: None,
                        room: crowd,
                    };
                    lamp.limit(prev, target, dt, now, rules)
                }
                _ => {
                    lamp.held_at = None;
                    target
                }
            };
            lamp.held = sent != target;
            lamp.see(sent, now, room);
            *out = sent;
        }
    }

    /// Whether light `i`'s last output was eased rather than the one asked
    /// for: it should glide to it, never be sent sharp.
    pub fn held(&self, i: usize) -> bool {
        self.lamps.get(i).is_some_and(|l| l.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::engine::{Ambilight, EngineSettings, Frame, PixelFormat};
    use crate::geometry::{LightMapping, SampleZone};
    use crate::show::ShowSettings;
    use crate::stage::{Fixture, Levels, Stage};

    fn color(r: u8, g: u8, b: u8, bright: u8) -> BulbOutput {
        BulbOutput::Color {
            rgb: Rgb::new(r, g, b),
            bright,
        }
    }

    fn white(bright: u8) -> BulbOutput {
        BulbOutput::White {
            kelvin: 6500,
            bright,
        }
    }

    /// The most transitions of one signal in any one second, counted afresh
    /// from what was sent: a direct reading of WCAG's rule, independent of
    /// the guard's own bookkeeping.
    fn most_per_second(frames: &[(f64, f32)]) -> usize {
        let mut times = Vec::new();
        let mut ext = frames[0].1;
        let mut dir = 0i8;
        for &(t, v) in &frames[1..] {
            let rising = v - ext >= STEP && dir <= 0 && ext < DARK;
            let falling = ext - v >= STEP && dir >= 0 && v < DARK;
            if rising || falling {
                dir = if rising { 1 } else { -1 };
                ext = v;
                times.push(t);
            } else if (dir > 0 && v > ext) || (dir < 0 && v < ext) {
                ext = v;
            }
        }
        times
            .iter()
            .map(|&t| times.iter().filter(|&&u| u <= t && t - u < 1.0).count())
            .max()
            .unwrap_or(0)
    }

    /// The most flashes in any second, per light, of one signal.
    fn flashes_of(sent: &[Vec<BulbOutput>], dt: f32, signal: impl Fn(BulbOutput) -> f32) -> usize {
        let mut most = 0;
        for i in 0..sent[0].len() {
            let series: Vec<(f64, f32)> = sent
                .iter()
                .enumerate()
                .map(|(k, f)| (k as f64 * f64::from(dt), signal(f[i])))
                .collect();
            most = most.max(most_per_second(&series));
        }
        most.div_ceil(2)
    }

    /// The most flashes in any second, per light, of every signal.
    fn flashes(sent: &[Vec<BulbOutput>], dt: f32) -> usize {
        flashes_of(sent, dt, |o| signals(o).0)
            .max(flashes_of(sent, dt, |o| signals(o).1))
            .max(flashes_of(sent, dt, glare))
    }

    /// Run `frames` through `guard`, `dt` apart.
    fn guarded(
        guard: &mut FlashGuard,
        frames: &[Vec<BulbOutput>],
        dt: f32,
        on: bool,
    ) -> Vec<Vec<BulbOutput>> {
        frames
            .iter()
            .map(|f| {
                let mut f = f.clone();
                guard.apply(&mut f, dt, on);
                f
            })
            .collect()
    }

    #[test]
    fn it_is_on_unless_turned_off() {
        assert!(AppConfig::from_json("{}").unwrap().safety.flash_guard);
        let json = r#"{ "safety": {} }"#;
        assert!(AppConfig::from_json(json).unwrap().safety.flash_guard);
        let json = r#"{ "safety": { "flash_guard": false } }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert!(!cfg.safety.flash_guard);
        assert!(cfg.to_json().unwrap().contains("\"flash_guard\": false"));
    }

    #[test]
    fn the_measure_follows_the_eye() {
        let (w, _) = signals(white(100));
        assert!((w - 1.0).abs() < 0.02, "{w}");
        let (b, _) = signals(color(0, 0, 255, 100));
        assert!((FLOOR..0.4).contains(&b), "a full blue counts for {b}");
        let (y, _) = signals(color(255, 255, 0, 100));
        assert!(y > 0.9, "{y}");
        let (_, red) = signals(color(255, 0, 0, 100));
        assert!(red > 0.99, "{red}");
        let (_, amber) = signals(color(255, 200, 0, 100));
        assert_eq!(amber, 0.0);
        let (dark, _) = signals(white(1));
        assert!(dark < 0.02);
    }

    #[test]
    fn a_slow_change_passes_untouched() {
        // Four seconds from dark to full and back, through the hues: never a
        // flash, so never touched, even by the ambilight's cap on rises.
        let dt = 1.0 / 45.0;
        let frames: Vec<Vec<BulbOutput>> = (0..180)
            .map(|k| {
                let x = k as f32 / 90.0;
                let level = if x < 1.0 { x } else { 2.0 - x };
                let bright = (1.0 + 99.0 * level).round() as u8;
                vec![color(255, (k * 255 / 180) as u8, 40, bright)]
            })
            .collect();
        let mut guard = FlashGuard::ambilight(1);
        assert_eq!(guarded(&mut guard, &frames, dt, true), frames);
    }

    #[test]
    fn normal_changes_keep_their_pace() {
        // A cut to another scene, then a fade to black: the ambilight's
        // latency is unchanged, darkening is immediate.
        let dt = 1.0 / 45.0;
        let frames = vec![
            vec![color(255, 120, 0, 60)],
            vec![color(0, 80, 255, 64)],
            vec![color(0, 80, 255, 1)],
            vec![color(0, 80, 255, 8)],
        ];
        let mut guard = FlashGuard::ambilight(1);
        assert_eq!(guarded(&mut guard, &frames, dt, true), frames);
    }

    #[test]
    fn a_flashbang_rises_over_a_moment() {
        // A dark game, then the whole screen white for a second, then dark.
        let dt = 1.0 / 45.0;
        let mut frames = vec![vec![color(30, 40, 60, 5)]; 10];
        frames.extend(vec![vec![white(100)]; 45]);
        frames.extend(vec![vec![color(30, 40, 60, 5)]; 5]);
        let mut guard = FlashGuard::ambilight(1);
        let sent = guarded(&mut guard, &frames, dt, true);
        let full = sent
            .iter()
            .position(|f| f[0].bright() >= 95)
            .expect("never reaches full");
        let took = (full - 10) as f32 * dt;
        assert!(took >= 0.06, "full white after {took} s");
        assert!(took <= 0.15, "full white only after {took} s");
        for w in sent.windows(2) {
            let (a, _) = signals(w[0][0]);
            let (b, _) = signals(w[1][0]);
            assert!(b - a <= FIRST_RISE * dt + 2.0 * QUANTUM, "{a} -> {b}");
        }
        // It goes back to dark at once.
        assert_eq!(sent[55], frames[55]);
        // Unprotected, the room whites out on the spot.
        let mut guard = FlashGuard::ambilight(1);
        assert_eq!(guarded(&mut guard, &frames, dt, false), frames);
    }

    #[test]
    fn a_light_that_keeps_flashing_rises_slower() {
        // Dark and bright twice in quick succession, then bright again: the
        // third rise comes after a flash already, and swells.
        let dt = 1.0 / 45.0;
        let dark = color(30, 40, 60, 5);
        let mut frames = vec![vec![dark]; 10];
        for _ in 0..2 {
            frames.extend(vec![vec![white(100)]; 8]);
            frames.extend(vec![vec![dark]; 8]);
        }
        frames.extend(vec![vec![white(100)]; 30]);
        let mut guard = FlashGuard::ambilight(1);
        let sent = guarded(&mut guard, &frames, dt, true);
        let last_rise = 10 + 32;
        let full = (last_rise..sent.len())
            .find(|&i| sent[i][0].bright() >= 95)
            .expect("never reaches full");
        let took = (full - last_rise) as f32 * dt;
        assert!(took >= 0.2, "full white after {took} s");
    }

    #[test]
    fn a_siren_alternating_between_lights_is_held_for_the_room() {
        // Red on the left while the right is dark, then the other way round,
        // each side 2.5 times a second: within each light's budget, but five
        // red flashes a second in the room.
        let dt = 1.0 / 45.0;
        let red = color(255, 0, 0, 100);
        let dark = color(255, 0, 0, 5);
        let mut frames = Vec::new();
        for k in 0..45 * 4 {
            let left = (k / 9) % 2 == 0;
            frames.push(if left {
                vec![red, dark]
            } else {
                vec![dark, red]
            });
        }
        let mut guard = FlashGuard::ambilight(2);
        let sent = guarded(&mut guard, &frames, dt, true);
        let per_light = flashes(&sent, dt);
        assert!(per_light <= 3, "{per_light} flashes a second on one light");
        // Unguarded, the two lights alternate in full every 0.2 s; guarded,
        // the room's alternations are held to its budget.
        let turns = |frames: &[Vec<BulbOutput>]| {
            frames
                .windows(2)
                .filter(|w| {
                    let swap = |f: &Vec<BulbOutput>| signals(f[0]).1 > signals(f[1]).1;
                    swap(&w[0]) != swap(&w[1])
                        && (signals(w[1][0]).1 - signals(w[1][1]).1).abs() > 0.5
                })
                .count()
        };
        assert!(turns(&frames) >= 15, "the siren alternates");
        assert!(
            turns(&sent) * 2 < turns(&frames),
            "{} of {} alternations went through",
            turns(&sent),
            turns(&frames)
        );
    }

    #[test]
    fn a_cut_reaches_every_light_at_once() {
        // Every light turning the same way together is one change of the
        // room, never held for it.
        let dt = 1.0 / 45.0;
        let dark = color(30, 40, 60, 5);
        let bright = color(200, 120, 60, 90);
        let mut frames = vec![vec![dark; 4]; 10];
        frames.extend(vec![vec![bright; 4]; 20]);
        let mut guard = FlashGuard::ambilight(4);
        let sent = guarded(&mut guard, &frames, dt, true);
        assert_eq!(sent.last(), frames.last());
        let arrived = sent.iter().position(|f| f == &frames[29]).expect("arrives");
        assert!(arrived <= 10 + 6, "arrived on frame {arrived}");
    }

    /// A frame of one flat colour.
    fn flat(v: u8) -> Frame {
        let (w, h) = (32u32, 18u32);
        Frame::new(w, h, PixelFormat::Rgba, vec![v; (w * h * 4) as usize])
    }

    #[test]
    fn a_strobing_screen_flashes_three_times_a_second_at_most() {
        // Black and white alternating on screen, every frame and every few.
        let dt = 1.0 / 45.0;
        let mapping = LightMapping::direct(SampleZone {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        for period in [1, 2, 3, 5] {
            let mut engine = Ambilight::new(1, EngineSettings::default());
            engine.set_frame_time(dt);
            let frames: Vec<Vec<BulbOutput>> = (0..45 * 5)
                .map(|k| {
                    let v = if (k / period) % 2 == 0 { 0 } else { 255 };
                    engine.process_outputs(&flat(v), std::slice::from_ref(&mapping))
                })
                .collect();
            assert!(
                flashes(&frames, dt) > 3,
                "the test strobes too gently at {period}"
            );
            let mut guard = FlashGuard::ambilight(1);
            let sent = guarded(&mut guard, &frames, dt, true);
            let n = flashes(&sent, dt);
            assert!(n <= 3, "{n} flashes a second, strobing every {period}");
            // Once the budget is spent, it glides: never again a step big
            // enough to be a transition on its own.
            for w in sent[45..].windows(2) {
                let (a, _) = signals(w[0][0]);
                let (b, _) = signals(w[1][0]);
                assert!(
                    (b - a).abs() < STEP,
                    "a jump of {} strobing every {period}",
                    (b - a).abs()
                );
            }
        }
    }

    /// A groove with a coloured snare and hi-hat pattern far busier than a
    /// light should follow: a beat every half second, snares every 0.075 s,
    /// hats every 0.05 s.
    fn busy_groove(stage: &mut Stage, s: &ShowSettings, secs: f32) -> Vec<Vec<BulbOutput>> {
        (0..(secs / 0.025) as usize)
            .map(|k| {
                let l = Levels {
                    energy: 1.0,
                    bass: 1.0,
                    kick: k % 20 == 0,
                    beat: (k % 20 == 0).then_some(1.0),
                    snare: (k % 5 == 2).then_some(1.0),
                    hat: (k % 3 == 0).then_some(1.0),
                    tempo: Some(0.5),
                    ..Levels::default()
                };
                stage.frame(s, l, 0.025)
            })
            .collect()
    }

    #[test]
    fn coloured_snares_and_hats_flash_three_times_a_second_at_most() {
        let s = ShowSettings {
            flashes: false,
            ..ShowSettings::default()
        };
        // Three on the beat, and one at the ceiling on the snares.
        let mut fixtures = Fixture::row(4);
        fixtures[1].height = 0.9;
        let mut stage = Stage::new(&fixtures);
        let frames = busy_groove(&mut stage, &s, 6.0);
        assert!(flashes(&frames, 0.025) > 3, "the pattern is too tame");
        let mut guard = FlashGuard::show(4);
        let sent = guarded(&mut guard, &frames, 0.025, true);
        let n = flashes(&sent, 0.025);
        assert!(n <= 3, "{n} flashes a second");
        // The show still plays: the lights still move with the music.
        let swing = sent.iter().map(|f| f[0].bright()).max().unwrap()
            - sent.iter().map(|f| f[0].bright()).min().unwrap();
        assert!(swing > 30, "the guard froze the show: {swing}");
    }

    /// A drop into loud music with a snare every 0.125 s, as close as the
    /// analyser lets them come, white allowed: far more white flashes than
    /// three a second.
    fn white_storm() -> Vec<Vec<BulbOutput>> {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(4));
        // A long breakdown first, so the kick lands as a drop: white is kept
        // for the drops.
        let calm = Levels {
            energy: 1.0,
            ..Levels::default()
        };
        for _ in 0..280 {
            stage.frame(&s, calm, 0.025);
        }
        let mut frames = Vec::new();
        for k in 0..240 {
            let l = Levels {
                energy: 1.0,
                bass: 1.0,
                kick: k % 20 == 0,
                beat: (k % 20 == 0).then_some(1.0),
                snare: (k % 5 == 2).then_some(1.0),
                ..Levels::default()
            };
            frames.push(stage.frame(&s, l, 0.025));
        }
        frames
    }

    #[test]
    fn white_flashes_go_through_the_same_guard() {
        let frames = white_storm();
        let raw = flashes_of(&frames, 0.025, glare);
        assert!(raw > 3, "{raw} whites a second only");
        let mut guard = FlashGuard::show(4);
        let sent = guarded(&mut guard, &frames, 0.025, true);
        let n = flashes(&sent, 0.025);
        assert!(n <= 3, "{n} flashes a second");
    }

    #[test]
    fn white_flashes_stay_capped_with_the_protection_off() {
        // The music's promise holds whatever the setting: three white
        // flashes a second at most.
        let frames = white_storm();
        let mut guard = FlashGuard::show(4);
        let sent = guarded(&mut guard, &frames, 0.025, false);
        let n = flashes_of(&sent, 0.025, glare);
        assert!(n <= 3, "{n} white flashes a second");
        // Its coloured punches are the user's choice, and left alone.
        let s = ShowSettings {
            flashes: false,
            ..ShowSettings::default()
        };
        let mut stage = Stage::new(&Fixture::row(4));
        let colours = busy_groove(&mut stage, &s, 4.0);
        let mut guard = FlashGuard::show(4);
        assert_eq!(guarded(&mut guard, &colours, 0.025, false), colours);
    }

    #[test]
    fn red_flashes_are_counted_on_their_own() {
        // Red and white at the same strength: hardly a change of brightness,
        // all a change of red.
        let dt = 0.025;
        let frames: Vec<Vec<BulbOutput>> = (0..200)
            .map(|k| {
                if (k / 2) % 2 == 0 {
                    vec![color(255, 0, 0, 100)]
                } else {
                    vec![BulbOutput::White {
                        kelvin: 6500,
                        bright: 45,
                    }]
                }
            })
            .collect();
        assert!(flashes(&frames, dt) > 3);
        let mut guard = FlashGuard::new(1);
        let sent = guarded(&mut guard, &frames, dt, true);
        assert!(flashes(&sent, dt) <= 3);
    }

    #[test]
    fn turned_off_it_changes_nothing() {
        let frames: Vec<Vec<BulbOutput>> = (0..100)
            .map(|k| vec![if k % 2 == 0 { white(100) } else { white(1) }])
            .collect();
        let mut guard = FlashGuard::ambilight(1);
        assert_eq!(guarded(&mut guard, &frames, 0.025, false), frames);
        assert!(!guard.held(0));
    }

    #[test]
    fn turned_on_mid_strobe_it_protects_at_once() {
        // It watched while off, so the strobe has already spent the budget.
        let frames: Vec<Vec<BulbOutput>> = (0..40)
            .map(|k| vec![if k % 2 == 0 { white(100) } else { white(1) }])
            .collect();
        let mut guard = FlashGuard::new(1);
        guarded(&mut guard, &frames, 0.025, false);
        let mut next = vec![white(100)];
        guard.apply(&mut next, 0.025, true);
        assert!(next[0].bright() < 20, "{:?}", next[0]);
        assert!(guard.held(0));
    }

    #[test]
    fn a_white_and_a_colour_meet_smoothly() {
        let a = white(80);
        let b = color(0, 0, 255, 80);
        let mut prev = signals(a).0;
        for k in 1..=20 {
            let (g, _) = signals(mix(a, b, k as f32 / 20.0));
            assert!((g - prev).abs() < 0.1, "a jump at {k}: {prev} -> {g}");
            prev = g;
        }
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
    }
}
