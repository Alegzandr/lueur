//! Light shows that need no screen: a rainbow, a sunset, the sea, a candle,
//! and lights that dance to the music.
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

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT, Rgb, full_chroma};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// The light shows. Each is a finished recipe: picking one is the only
/// decision, so there is nothing to tune and nothing to get wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShowKind {
    /// Colours change on the beat, brightness follows the loudness.
    #[default]
    Music,
    /// The whole colour wheel, turning slowly through the room.
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
}

/// The show the window and the tray start, and how bright it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowSettings {
    pub kind: ShowKind,
    /// 1..=100.
    pub bright: u8,
}

impl Default for ShowSettings {
    fn default() -> Self {
        ShowSettings {
            kind: ShowKind::default(),
            bright: 80,
        }
    }
}

impl ShowSettings {
    fn bright(&self) -> f32 {
        f32::from(self.bright.clamp(MIN_BRIGHT, MAX_BRIGHT)) / 100.0
    }
}

/// Eight hues at even steps for the eye rather than for the maths: plain
/// HSV races through red, orange and yellow and lingers in green.
const RAINBOW: &[Rgb] = &[
    Rgb::new(255, 0, 0),
    Rgb::new(255, 110, 0),
    Rgb::new(255, 215, 0),
    Rgb::new(0, 255, 50),
    Rgb::new(0, 225, 255),
    Rgb::new(0, 70, 255),
    Rgb::new(140, 0, 255),
    Rgb::new(255, 0, 170),
];

/// From a low amber sun to the last rose and plum of the sky, and back.
const SUNSET: &[Rgb] = &[
    Rgb::new(255, 140, 10),
    Rgb::new(255, 85, 10),
    Rgb::new(255, 45, 35),
    Rgb::new(255, 30, 105),
    Rgb::new(190, 30, 180),
    Rgb::new(255, 40, 70),
];

/// The sea from its depths to a breaking crest.
const OCEAN: &[Rgb] = &[
    Rgb::new(20, 35, 255),
    Rgb::new(0, 105, 255),
    Rgb::new(0, 185, 235),
    Rgb::new(0, 235, 175),
];

/// Hot, contrasting colours, ordered so each beat lands somewhere new.
const PARTY: &[Rgb] = &[
    Rgb::new(255, 0, 80),
    Rgb::new(0, 120, 255),
    Rgb::new(255, 150, 0),
    Rgb::new(170, 0, 255),
    Rgb::new(0, 220, 160),
];

/// A candle at its dimmest (reddest) and brightest (yellowest).
const EMBER: Rgb = Rgb::new(255, 78, 4);
const FLAME: Rgb = Rgb::new(255, 152, 42);

/// A bulb output for a colour at a 0..=1 brightness.
fn output(rgb: Rgb, level: f32) -> BulbOutput {
    let pct = (level.clamp(0.0, 1.0) * 100.0).round() as u8;
    BulbOutput::Color {
        rgb: full_chroma(rgb),
        bright: pct.clamp(MIN_BRIGHT, MAX_BRIGHT),
    }
}

/// How much to dim a colour so it looks as bright as the others.
///
/// A bulb drives every hue at the same power, but a full yellow or green
/// reads far brighter than a full blue. Holding the light ones back a little
/// keeps a show's level steady as it travels round the wheel; blue, the
/// darkest, plays at exactly the brightness asked for.
fn even(rgb: Rgb) -> f32 {
    const PULL: f32 = 0.3;
    let c = full_chroma(rgb);
    let y = (0.2126 * f32::from(c.r) + 0.7152 * f32::from(c.g) + 0.0722 * f32::from(c.b)) / 255.0;
    (1.0 - PULL * y) / (1.0 - PULL * 0.0722)
}

/// Hue (degrees), saturation and value of a colour.
fn hsv(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb::new(to(r), to(g), to(b))
}

/// Blend two colours by `t` along the hue circle, the short way round.
/// Mixing in RGB would pass through grey between opposite colours; going
/// round the wheel keeps every step as vivid as the ends.
pub fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let (ha, sa, va) = hsv(a);
    let (hb, sb, vb) = hsv(b);
    // A grey end has no hue of its own: borrow the other's.
    let (ha, hb) = match (sa < 0.05, sb < 0.05) {
        (true, false) => (hb, hb),
        (false, true) => (ha, ha),
        _ => (ha, hb),
    };
    let mut dh = hb - ha;
    if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    from_hsv(ha + dh * t, sa + (sb - sa) * t, va + (vb - va) * t)
}

/// The colour at `pos` round a looping palette (1.0 = one colour further on),
/// moving at an even pace: no colour holds, so nothing ever seems to stall.
fn palette_at(colors: &[Rgb], pos: f32) -> Rgb {
    let len = colors.len();
    let pos = pos.rem_euclid(len as f32);
    let i = (pos.floor() as usize).min(len - 1);
    blend(colors[i], colors[(i + 1) % len], pos - pos.floor())
}

/// The colour at `x` (0..=1) along a palette that does not loop.
fn ramp(colors: &[Rgb], x: f32) -> Rgb {
    let pos = x.clamp(0.0, 1.0) * (colors.len() - 1) as f32;
    let i = (pos.floor() as usize).min(colors.len() - 2);
    blend(colors[i], colors[i + 1], pos - i as f32)
}

/// Where light `i` of `n` sits, spreading the room over `width`.
fn spread(i: usize, n: usize, width: f32) -> f32 {
    if n > 1 {
        i as f32 * width / n as f32
    } else {
        0.0
    }
}

/// A repeatable pseudo-random value in -1..=1 for a lattice point.
fn hash(seed: u32, i: i32) -> f32 {
    let mut h = seed ^ (i as u32).wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 23) as f32 - 1.0
}

/// Smooth noise in about -1..=1: random values at whole numbers, joined by
/// a Catmull-Rom curve, so it never stops dead the way an eased step does.
/// Organic movement, and no two seeds ever fall into step.
fn noise(seed: u32, x: f32) -> f32 {
    let i = x.floor() as i32;
    let f = x - x.floor();
    let p0 = hash(seed, i - 1);
    let p1 = hash(seed, i);
    let p2 = hash(seed, i + 1);
    let p3 = hash(seed, i + 2);
    let v = p1
        + 0.5
            * f
            * (p2 - p0
                + f * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + f * (3.0 * (p1 - p2) + p3 - p0)));
    v.clamp(-1.0, 1.0)
}

/// A seed of its own for each light and each kind of movement.
fn seed(light: usize, channel: u32) -> u32 {
    (light as u32).wrapping_mul(0x2c1b_3c6d) ^ channel.wrapping_mul(0x297a_2d39)
}

/// The rainbow: the wheel turning at an even pace, a full turn every 32 s.
/// The room covers half the wheel at once, so neighbouring lights blend on
/// the walls into a gradient instead of cancelling out.
fn rainbow(t: f32, i: usize, n: usize, bright: f32) -> BulbOutput {
    let pos = t / 4.0 + spread(i, n, RAINBOW.len() as f32 * 0.5);
    let rgb = palette_at(RAINBOW, pos);
    output(rgb, bright * even(rgb))
}

/// The sunset: each light wanders through the warm colours at its own pace,
/// and swells and settles slowly, like the sky as clouds pass.
fn sunset(t: f32, i: usize, n: usize, bright: f32) -> BulbOutput {
    let pos = t / 14.0 + spread(i, n, 1.6) + 0.9 * noise(seed(i, 1), t / 23.0);
    let rgb = palette_at(SUNSET, pos);
    let swell = 0.86 + 0.14 * noise(seed(i, 2), t / 9.0);
    output(rgb, bright * swell * even(rgb))
}

/// The sea: a swell rolls through the room from one light to the next,
/// never quite regular. Each crest lifts the light and turns it to teal;
/// each trough sinks it to a deep blue. The tide slowly drifts the whole
/// sea bluer or greener.
fn ocean(t: f32, i: usize, n: usize, bright: f32) -> BulbOutput {
    let lag = if n > 1 { 0.9 / n as f32 } else { 0.0 };
    let phase = t / 7.5 - i as f32 * lag + 0.3 * noise(seed(i, 3), t / 11.0);
    let wave = 0.5 - 0.5 * (TAU * phase).cos();
    let tide = 0.5 + 0.5 * noise(seed(0, 4), t / 40.0);
    let rgb = ramp(OCEAN, 0.7 * wave + 0.3 * tide);
    let level = 0.5 + 0.5 * wave;
    output(rgb, bright * level * even(rgb))
}

/// A small, fast, deterministic random source: enough for a flame.
#[derive(Debug, Clone, Copy)]
struct XorShift(u32);

impl XorShift {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// A candle per light: a warm flame that wavers on its own.
///
/// A real flame has three movements, and each is here: a slow sway, a fast
/// shallow flutter, and now and then a draught that ducks it hard and lets
/// it climb back. A dimmer flame is a redder one, so colour follows level.
/// A faint shared draught keeps the candles feeling like one room.
#[derive(Debug, Clone)]
pub struct Candle {
    flames: Vec<Flame>,
    rng: XorShift,
    t: f32,
}

#[derive(Debug, Clone, Copy, Default)]
struct Flame {
    /// How deep the current draught pulls the flame down, 0..=1.
    gust: f32,
    /// Where the draught is heading; it decays back to calm.
    gust_target: f32,
}

impl Candle {
    pub fn new(n: usize) -> Self {
        Candle {
            flames: vec![Flame::default(); n],
            rng: XorShift(0x9e37_79b9),
            t: 0.0,
        }
    }

    /// Advance by `dt` seconds.
    pub fn frame(&mut self, settings: &ShowSettings, dt: f32) -> Vec<BulbOutput> {
        // Draughts per second, per flame: one every quarter of a minute or so.
        const GUSTS: f32 = 0.07;
        self.t += dt;
        let t = self.t;
        let bright = settings.bright();
        let room = 0.025 * noise(seed(0, 10), t * 0.5);
        let rng = &mut self.rng;
        self.flames
            .iter_mut()
            .enumerate()
            .map(|(i, f)| {
                if rng.next() < GUSTS * dt {
                    f.gust_target = f.gust_target.max(0.25 + 0.25 * rng.next());
                }
                f.gust_target *= (-dt * 2.8).exp();
                f.gust += (f.gust_target - f.gust) * (dt * 18.0).min(1.0);

                let sway = 0.06 * noise(seed(i, 11), t * 1.1);
                // A draught makes the flame flutter harder as well as dip.
                let flutter = (0.035 + 0.12 * f.gust) * noise(seed(i, 12), t * 8.0);
                let level = (0.86 + sway + flutter + room - f.gust).clamp(0.2, 1.0);
                let warmth = ((level - 0.45) / 0.55).clamp(0.0, 1.0);
                output(blend(EMBER, FLAME, warmth), level * bright)
            })
            .collect()
    }
}

/// Levels measured from one slice of sound.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Levels {
    /// Loudness relative to what has been playing lately, 0..=1.
    pub energy: f32,
    /// A kick or a strong onset just landed.
    pub beat: bool,
    /// How hard it landed, 0..=1 (0 without a beat).
    pub strength: f32,
}

/// Length of one analysis step: short enough to place a beat within a few
/// milliseconds whatever the frame rate, long enough to measure the bass.
const HOP_SECS: f32 = 0.01;

/// Turns raw samples into [`Levels`]: how loud it is now compared with the
/// last few seconds, and where the beats fall.
///
/// The bass carries the beat, so onsets are read from a low-passed signal,
/// in decibels so a kick stands out the same at any volume: a beat is the
/// bass jumping above its own recent level by more than the music usually
/// does. That bar adapts to the track, so a busy bass line does not fire on
/// every note and a sparse one is not missed. Loudness is self-normalising
/// too, so the show looks the same with the volume at 10% or at 100%.
#[derive(Debug, Clone)]
pub struct Analyzer {
    rate: f32,
    hop: usize,
    /// Low-pass coefficient, and its two stages.
    alpha: f32,
    lp1: f32,
    lp2: f32,
    /// The hop being gathered: sum of squares, of the bass too, and count.
    sum: f32,
    bass: f32,
    count: usize,
    env: f32,
    peak: f32,
    bass_peak: f32,
    /// The bass's recent level, in dB (NaN until the first hop).
    slow_db: f32,
    /// How far the bass usually rises above it: mean and mean deviation.
    rise_mean: f32,
    rise_dev: f32,
    since_beat: f32,
    heard: f32,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(1) as f32;
        Analyzer {
            rate,
            hop: ((rate * HOP_SECS) as usize).max(1),
            // One-pole low-pass at ~150 Hz; two in a row isolate the kick
            // and the bass line from the vocals.
            alpha: 1.0 - (-TAU * 150.0 / rate).exp(),
            lp1: 0.0,
            lp2: 0.0,
            sum: 0.0,
            bass: 0.0,
            count: 0,
            env: 0.0,
            peak: 1e-4,
            bass_peak: 1e-4,
            slow_db: f32::NAN,
            rise_mean: 0.0,
            rise_dev: 1.0,
            since_beat: 1.0,
            heard: 0.0,
        }
    }

    /// Read the mono samples captured since the last call.
    pub fn feed(&mut self, samples: &[f32]) -> Levels {
        let mut levels = Levels {
            energy: self.energy(),
            ..Levels::default()
        };
        for &s in samples {
            let s = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.lp1 += (s - self.lp1) * self.alpha;
            self.lp2 += (self.lp1 - self.lp2) * self.alpha;
            self.sum += s * s;
            self.bass += self.lp2 * self.lp2;
            self.count += 1;
            if self.count == self.hop {
                if let Some(strength) = self.step() {
                    levels.beat = true;
                    levels.strength = levels.strength.max(strength);
                }
                levels.energy = self.energy();
            }
        }
        levels
    }

    fn energy(&self) -> f32 {
        (self.env / self.peak).clamp(0.0, 1.0)
    }

    /// Close one hop. The strength of the beat it holds, if any.
    fn step(&mut self) -> Option<f32> {
        let n = self.count as f32;
        let rms = (self.sum / n).sqrt();
        let bass = (self.bass / n).sqrt();
        (self.sum, self.bass, self.count) = (0.0, 0.0, 0);
        let dt = n / self.rate;
        let k = |tau: f32| 1.0 - (-dt / tau).exp();

        // Fast attack, slower release: lights jump with the music and settle.
        let tau = if rms > self.env { 0.01 } else { 0.25 };
        self.env += (rms - self.env) * k(tau);
        // The loudest of the last few seconds sets the scale.
        self.peak = (self.peak * (1.0 - k(6.0))).max(self.env).max(1e-4);
        self.bass_peak = (self.bass_peak * (1.0 - k(6.0))).max(bass).max(1e-4);

        let db = 20.0 * bass.max(1e-6).log10();
        if self.slow_db.is_nan() {
            // The first hop sets the level: music already playing is not a beat.
            self.slow_db = db;
        }
        let rise = (db - self.slow_db).max(0.0);
        self.slow_db += (db - self.slow_db) * k(0.25);
        self.since_beat += dt;
        self.heard += dt;

        // A beat clears both a fixed bar and what this track usually does,
        // carries real bass (not hiss in a pause), and is not the tail of the
        // last one. The first half second only learns the level.
        let bar = (self.rise_mean + 1.8 * self.rise_dev).max(4.0);
        let beat = self.heard > 0.5
            && rise > bar
            && bass > 0.06 * self.bass_peak
            && bass > 3e-4
            && self.since_beat > 0.22;
        self.rise_mean += (rise - self.rise_mean) * k(1.5);
        self.rise_dev += ((rise - self.rise_mean).abs() - self.rise_dev) * k(1.5);
        if !beat {
            return None;
        }
        self.since_beat = 0.0;
        Some((0.55 + (rise - bar) / 12.0).clamp(0.55, 1.0))
    }
}

/// Lights that dance: a beat flashes them and moves them on to the next
/// colour, loudness sets how bright they burn in between.
///
/// Between beats the level stays well short of full, so even a loud,
/// compressed track keeps a visible pulse. When nothing marks a beat (a
/// quiet passage, speech, silence) the colours drift on their own so the
/// room never freezes.
#[derive(Debug, Clone, Default)]
pub struct Dance {
    /// Palette position the lights are gliding towards.
    step: f32,
    shown: f32,
    flash: f32,
    level: f32,
    /// Seconds since the last beat.
    quiet: f32,
    /// A beat landed this frame.
    accent: bool,
}

impl Dance {
    pub fn frame(
        &mut self,
        settings: &ShowSettings,
        levels: Levels,
        dt: f32,
        n: usize,
    ) -> Vec<BulbOutput> {
        self.flash *= (-dt / 0.16).exp();
        self.accent = levels.beat;
        if levels.beat {
            self.step = self.step.round() + 1.0;
            self.flash = self.flash.max(levels.strength.clamp(0.0, 1.0));
            self.quiet = 0.0;
        } else {
            self.quiet += dt;
            if self.quiet > 1.5 {
                self.step += dt * 0.12;
            }
        }
        // Colour glides to the new step quickly.
        self.shown += (self.step - self.shown) * (1.0 - (-dt / 0.07).exp());
        let tau = if levels.energy > self.level {
            0.04
        } else {
            0.25
        };
        self.level += (levels.energy - self.level) * (1.0 - (-dt / tau).exp());

        // Silence keeps a low glow rather than going dark.
        let base = 0.14 + 0.5 * self.level.clamp(0.0, 1.0).powf(1.6);
        let level = (base + (1.0 - base) * self.flash) * settings.bright();
        (0..n)
            .map(|i| {
                let pos = self.shown + spread(i, n, PARTY.len() as f32).round();
                output(palette_at(PARTY, pos), level)
            })
            .collect()
    }

    /// A beat landed on the last frame: send it sharp, not faded in.
    pub fn accent(&self) -> bool {
        self.accent
    }
}

/// One running show, whatever its kind.
#[derive(Debug, Clone)]
pub struct Show {
    kind: ShowKind,
    lights: usize,
    t: f32,
    candle: Candle,
    dance: Dance,
}

impl Show {
    pub fn new(kind: ShowKind, lights: usize) -> Self {
        Show {
            kind,
            lights,
            t: 0.0,
            candle: Candle::new(lights),
            dance: Dance::default(),
        }
    }

    /// Advance by `dt` seconds. `levels` is what the music show listens to;
    /// the others ignore it.
    pub fn frame(&mut self, settings: &ShowSettings, dt: f32, levels: Levels) -> Vec<BulbOutput> {
        self.t += dt;
        let (t, n, bright) = (self.t, self.lights, settings.bright());
        match self.kind {
            ShowKind::Music => self.dance.frame(settings, levels, dt, n),
            ShowKind::Candle => self.candle.frame(settings, dt),
            ShowKind::Rainbow => (0..n).map(|i| rainbow(t, i, n, bright)).collect(),
            ShowKind::Sunset => (0..n).map(|i| sunset(t, i, n, bright)).collect(),
            ShowKind::Ocean => (0..n).map(|i| ocean(t, i, n, bright)).collect(),
        }
    }

    /// Whether the last frame should be sent sharp rather than faded in.
    pub fn accent(&self) -> bool {
        self.kind == ShowKind::Music && self.dance.accent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(o: BulbOutput) -> Rgb {
        match o {
            BulbOutput::Color { rgb, .. } => rgb,
            other => panic!("expected a colour, got {other:?}"),
        }
    }

    fn settings(kind: ShowKind) -> ShowSettings {
        ShowSettings { kind, bright: 80 }
    }

    /// Play `kind` on `n` lights for `secs`, frame by frame.
    fn play(kind: ShowKind, n: usize, secs: f32) -> Vec<Vec<BulbOutput>> {
        let s = settings(kind);
        let mut show = Show::new(kind, n);
        let dt = kind.frame_ms() as f32 / 1000.0;
        (0..(secs / dt) as usize)
            .map(|_| show.frame(&s, dt, Levels::default()))
            .collect()
    }

    /// The largest jump of any channel between two colours.
    fn jump(a: Rgb, b: Rgb) -> u8 {
        a.r.abs_diff(b.r)
            .max(a.g.abs_diff(b.g))
            .max(a.b.abs_diff(b.b))
    }

    #[test]
    fn blending_goes_round_the_wheel() {
        // Red to blue the short way passes through magenta, not grey.
        let mid = blend(Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), 0.5);
        assert!(mid.r > 200 && mid.b > 200 && mid.g < 30, "{mid:?}");
        assert_eq!(
            blend(Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), 0.0),
            Rgb::new(255, 0, 0)
        );
    }

    #[test]
    fn noise_is_smooth_and_bounded() {
        let mut prev = noise(7, 0.0);
        for k in 1..4000 {
            let v = noise(7, k as f32 * 0.005);
            assert!((-1.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.05, "noise jumps at {k}");
            prev = v;
        }
        assert_ne!(noise(1, 3.3), noise(2, 3.3), "seeds differ");
    }

    #[test]
    fn evening_holds_back_the_bright_hues_only() {
        assert!((even(Rgb::new(0, 0, 255)) - 1.0).abs() < 1e-6);
        assert!(even(Rgb::new(255, 255, 0)) < 0.8);
        assert!(even(Rgb::new(255, 0, 0)) > 0.9);
    }

    #[test]
    fn ambient_shows_flow_without_jumps() {
        for kind in [ShowKind::Rainbow, ShowKind::Sunset, ShowKind::Ocean] {
            let frames = play(kind, 3, 120.0);
            for w in frames.windows(2) {
                for (a, b) in w[0].iter().zip(&w[1]) {
                    assert!(
                        jump(rgb(*a), rgb(*b)) <= 12,
                        "{kind:?} jumps: {a:?} -> {b:?}"
                    );
                    assert!(
                        a.bright().abs_diff(b.bright()) <= 2,
                        "{kind:?} brightness jumps"
                    );
                }
            }
        }
    }

    #[test]
    fn ambient_shows_move() {
        for kind in [ShowKind::Rainbow, ShowKind::Sunset, ShowKind::Ocean] {
            let frames = play(kind, 1, 10.0);
            assert_ne!(frames[0], frames[frames.len() - 1], "{kind:?} is frozen");
        }
    }

    #[test]
    fn the_rainbow_spreads_the_room_over_half_the_wheel() {
        // Distinct colours, never opposite ones that would cancel on the wall.
        for frame in play(ShowKind::Rainbow, 2, 32.0) {
            let (a, _, _) = hsv(rgb(frame[0]));
            let (b, _, _) = hsv(rgb(frame[1]));
            let gap = (a - b).abs().min(360.0 - (a - b).abs());
            assert!((40.0..=150.0).contains(&gap), "lights {gap}° apart");
        }
    }

    #[test]
    fn the_sunset_stays_warm() {
        for frame in play(ShowKind::Sunset, 4, 180.0) {
            for o in frame {
                let (h, _, _) = hsv(rgb(o));
                assert!(!(60.0..=280.0).contains(&h), "a cold sunset: {h}°");
            }
        }
    }

    #[test]
    fn the_ocean_stays_blue_and_swells() {
        let frames = play(ShowKind::Ocean, 2, 30.0);
        let mut lo = 100;
        let mut hi = 0;
        for frame in &frames {
            for o in frame {
                let (h, _, _) = hsv(rgb(*o));
                assert!((150.0..=245.0).contains(&h), "the sea turned {h}°");
                lo = lo.min(o.bright());
                hi = hi.max(o.bright());
            }
        }
        assert!(hi - lo > 20, "no swell: {lo}..{hi}");
        // The swell rolls: the two lights are not in step.
        assert!(
            frames
                .iter()
                .any(|f| f[0].bright().abs_diff(f[1].bright()) > 8)
        );
    }

    #[test]
    fn show_ids_round_trip() {
        for k in SHOWS {
            assert_eq!(ShowKind::from_id(k.id()), Some(k));
            let json = serde_json::to_string(&k).unwrap();
            assert_eq!(json, format!("\"{}\"", k.id()));
        }
        assert_eq!(ShowKind::from_id("disco"), None);
    }

    #[test]
    fn a_candle_stays_warm_and_alive() {
        let frames = play(ShowKind::Candle, 2, 60.0);
        let mut levels = Vec::new();
        for frame in &frames {
            for o in frame {
                let c = rgb(*o);
                assert!(c.r == 255 && c.g < 170 && c.b < 60, "{c:?}");
                levels.push(o.bright());
            }
        }
        let lo = *levels.iter().min().unwrap();
        let hi = *levels.iter().max().unwrap();
        assert!(hi - lo >= 6, "the flame never moves: {lo}..{hi}");
        assert!(hi <= 80, "brighter than asked: {hi}");
        // The two flames flicker on their own.
        assert!(frames.iter().any(|f| f[0] != f[1]));
    }

    #[test]
    fn a_candle_dims_redder() {
        let low = blend(EMBER, FLAME, 0.0);
        let high = blend(EMBER, FLAME, 1.0);
        assert!(low.g < high.g);
    }

    fn tone(rate: u32, freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn a_kick_after_quiet_is_a_beat() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let quiet = tone(rate, 60.0, 0.02, 0.04);
        for _ in 0..50 {
            assert!(!a.feed(&quiet).beat);
        }
        let kick = tone(rate, 60.0, 0.8, 0.04);
        let l = a.feed(&kick);
        assert!(l.beat);
        assert!(l.strength > 0.9, "{}", l.strength);
        assert!(l.energy > 0.9);
    }

    #[test]
    fn a_steady_tone_is_not_a_beat() {
        let rate = 44_100;
        let mut a = Analyzer::new(rate);
        let hum = tone(rate, 80.0, 0.5, 0.04);
        let beats = (0..200).filter(|_| a.feed(&hum).beat).count();
        assert!(beats <= 1, "{beats} beats in a hum");
    }

    #[test]
    fn a_steady_kick_is_followed() {
        // Four to the floor at 120 bpm over a quiet pad.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let mut beats = 0;
        for bar in 0..16 {
            let kick = tone(rate, 55.0, 0.7, 0.08);
            let pad = tone(rate, 55.0, 0.03, 0.42);
            let hit = a.feed(&kick).beat;
            a.feed(&pad);
            if bar > 0 && hit {
                beats += 1;
            }
        }
        assert!(beats >= 13, "followed {beats} of 15 kicks");
    }

    #[test]
    fn silence_is_calm() {
        let mut a = Analyzer::new(44_100);
        let l = a.feed(&vec![0.0; 1764]);
        assert!(!l.beat);
        assert_eq!(l.energy, 0.0);
        let mut d = Dance::default();
        let out = d.frame(&ShowSettings::default(), l, 0.025, 1);
        assert!(out[0].bright() < 20);
    }

    #[test]
    fn loud_music_still_pulses() {
        // Compressed music never leaves the top of its range; the beat
        // must still stand out against it.
        let s = ShowSettings::default();
        let mut d = Dance::default();
        let loud = Levels {
            energy: 1.0,
            beat: false,
            strength: 0.0,
        };
        let mut between = 0;
        for _ in 0..40 {
            between = d.frame(&s, loud, 0.025, 1)[0].bright();
        }
        let hit = Levels {
            energy: 1.0,
            beat: true,
            strength: 1.0,
        };
        let flash = d.frame(&s, hit, 0.025, 1)[0].bright();
        assert_eq!(flash, s.bright);
        assert!(flash - between >= 20, "{between} -> {flash}");
        assert!(d.accent());
    }

    #[test]
    fn a_beat_moves_the_colour_on() {
        let s = ShowSettings::default();
        let mut d = Dance::default();
        let calm = Levels {
            energy: 0.5,
            ..Levels::default()
        };
        let before = rgb(d.frame(&s, calm, 0.025, 1)[0]);
        d.frame(
            &s,
            Levels {
                energy: 1.0,
                beat: true,
                strength: 1.0,
            },
            0.025,
            1,
        );
        let mut after = before;
        for _ in 0..20 {
            after = rgb(d.frame(&s, calm, 0.025, 1)[0]);
        }
        assert!(jump(before, after) > 100, "{before:?} -> {after:?}");
        assert!(!d.accent());
    }

    #[test]
    fn without_beats_the_colours_drift() {
        let s = ShowSettings::default();
        let mut d = Dance::default();
        let first = rgb(d.frame(&s, Levels::default(), 0.025, 1)[0]);
        let mut last = first;
        for _ in 0..400 {
            last = rgb(d.frame(&s, Levels::default(), 0.025, 1)[0]);
        }
        assert_ne!(first, last);
    }
}
