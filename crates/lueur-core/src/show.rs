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

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT, Rgb, full_chroma};
use crate::stage::{Fixture, Levels, Stage};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

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

/// The colour show's combinations: three colours each, laid across the
/// lights left to right, that sit well together on a bulb and still mix into
/// a colour on the walls (neighbours stay within a quarter turn of the
/// wheel). The show glides from each to the next, looping: every colour
/// moves only a short way round the wheel to its place in the next one, and
/// the last leads back into the first, so the loop has no seam. The
/// yellow-greens are left out, and no glide passes through them: chartreuse
/// and lime turn sickly on a bulb.
const COMBOS: &[[Rgb; 3]] = &[
    // Embers: gold, orange, red.
    [
        Rgb::new(255, 160, 0),
        Rgb::new(255, 70, 0),
        Rgb::new(255, 0, 40),
    ],
    // Dusk: orange, coral, raspberry.
    [
        Rgb::new(255, 100, 0),
        Rgb::new(255, 20, 50),
        Rgb::new(235, 0, 150),
    ],
    // Flamingo: coral, hot pink, violet.
    [
        Rgb::new(255, 30, 60),
        Rgb::new(255, 0, 140),
        Rgb::new(170, 0, 255),
    ],
    // Neon: hot pink, violet, azure.
    [
        Rgb::new(255, 0, 110),
        Rgb::new(130, 0, 255),
        Rgb::new(0, 150, 255),
    ],
    // Twilight: magenta, indigo, blue.
    [
        Rgb::new(220, 0, 230),
        Rgb::new(90, 20, 255),
        Rgb::new(0, 110, 255),
    ],
    // Lagoon: violet, blue, cyan.
    [
        Rgb::new(120, 0, 255),
        Rgb::new(20, 60, 255),
        Rgb::new(0, 200, 240),
    ],
    // Ocean: blue, azure, turquoise.
    [
        Rgb::new(30, 50, 255),
        Rgb::new(0, 140, 255),
        Rgb::new(0, 230, 190),
    ],
    // Aurora: azure, turquoise, emerald.
    [
        Rgb::new(0, 170, 255),
        Rgb::new(0, 230, 170),
        Rgb::new(0, 255, 110),
    ],
    // Glacier: indigo, azure, cyan.
    [
        Rgb::new(90, 40, 255),
        Rgb::new(0, 150, 255),
        Rgb::new(0, 220, 220),
    ],
    // Orchid: purple, indigo, azure.
    [
        Rgb::new(200, 0, 255),
        Rgb::new(90, 40, 255),
        Rgb::new(0, 130, 255),
    ],
    // Berry: raspberry, magenta, violet.
    [
        Rgb::new(255, 0, 90),
        Rgb::new(200, 0, 220),
        Rgb::new(100, 20, 255),
    ],
    // Coral: orange, red, hot pink.
    [
        Rgb::new(255, 70, 0),
        Rgb::new(255, 10, 40),
        Rgb::new(255, 0, 150),
    ],
];

/// Seconds the colour show takes from one combination to the next.
const COMBO_SECS: f32 = 25.0;
/// Seconds for the colours to drift across the lights and back, so the room
/// never sits still even while a combination holds.
const DRIFT_SECS: f32 = 20.0;
/// How far the drift carries the colours, as a share of the room.
const DRIFT: f32 = 0.12;
/// The shows' clocks go back to zero after this many seconds: an `f32` that
/// kept counting would round each small step away after a few days. A whole
/// number of loops through the combinations and of ocean swells, so those
/// two never jump.
const CLOCK_WRAP: f32 = 3600.0;

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

/// The music's harmonies: bolder than the rainbow's, three colours each that
/// still sit well side by side. Every way round a harmony steers clear of the
/// yellow-greens, so even its drift between beats stays clean.
pub(crate) const PARTY: &[[Rgb; 3]] = &[
    // Neon: hot pink, violet, cyan.
    [
        Rgb::new(255, 0, 110),
        Rgb::new(130, 0, 255),
        Rgb::new(0, 190, 255),
    ],
    // Fire: red, orange, magenta.
    [
        Rgb::new(255, 10, 0),
        Rgb::new(255, 110, 0),
        Rgb::new(255, 0, 160),
    ],
    // Synthwave: magenta, blue, orange.
    [
        Rgb::new(255, 0, 200),
        Rgb::new(30, 40, 255),
        Rgb::new(255, 90, 0),
    ],
    // Deep sea: blue, turquoise, violet.
    [
        Rgb::new(0, 60, 255),
        Rgb::new(0, 230, 190),
        Rgb::new(150, 0, 255),
    ],
    // Aurora: emerald, azure, violet.
    [
        Rgb::new(0, 255, 120),
        Rgb::new(0, 170, 255),
        Rgb::new(120, 0, 255),
    ],
];

/// The music's harmonies for a slow or gentle track (a ballad, a band
/// without drums): warm and a little pale, the ambers and powdered roses a
/// lighting desk keeps for a quiet song. Neon is for a track that earns it.
/// Like the party's, every way round them steers clear of the yellow-greens.
pub(crate) const SOFT: &[[Rgb; 3]] = &[
    // Amber: amber, rose, gold.
    [
        Rgb::new(255, 120, 20),
        Rgb::new(255, 70, 100),
        Rgb::new(255, 165, 60),
    ],
    // Powder: powdered rose, peach, lavender.
    [
        Rgb::new(255, 110, 150),
        Rgb::new(255, 150, 100),
        Rgb::new(185, 130, 255),
    ],
    // Dusk: coral, plum, amber.
    [
        Rgb::new(255, 90, 60),
        Rgb::new(200, 60, 180),
        Rgb::new(255, 140, 40),
    ],
    // Moonlight: lavender, soft blue, rose.
    [
        Rgb::new(160, 140, 255),
        Rgb::new(110, 160, 255),
        Rgb::new(255, 130, 185),
    ],
];

/// A candle at its dimmest (reddest) and brightest (yellowest).
const EMBER: Rgb = Rgb::new(255, 78, 4);
const FLAME: Rgb = Rgb::new(255, 152, 42);

/// A bulb output for a colour at a 0..=1 brightness.
pub(crate) fn output(rgb: Rgb, level: f32) -> BulbOutput {
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
pub(crate) fn even(rgb: Rgb) -> f32 {
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

/// The yellow-green no glide may pass through, in degrees: the middle of the
/// sickly hues, chartreuse to lime.
const SICKLY_HUE: f32 = 95.0;

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

/// Blend two colours by `t` round the hue circle the way that steers clear
/// of the yellow-greens: the short way, unless that passes through them.
fn clean_blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let (ha, sa, va) = hsv(a);
    let (hb, sb, vb) = hsv(b);
    let mut dh = (hb - ha + 180.0).rem_euclid(360.0) - 180.0;
    let to_sickly = (SICKLY_HUE - ha).rem_euclid(360.0);
    let crosses = if dh >= 0.0 {
        to_sickly < dh
    } else {
        to_sickly > 360.0 + dh
    };
    if crosses {
        dh -= 360.0 * dh.signum();
    }
    let t = t.clamp(0.0, 1.0);
    from_hsv(ha + dh * t, sa + (sb - sa) * t, va + (vb - va) * t)
}

/// The colour at `pos` round a looping palette (1.0 = one colour further on),
/// moving at an even pace: no colour holds, so nothing ever seems to stall.
pub(crate) fn palette_at(colors: &[Rgb], pos: f32) -> Rgb {
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

/// The colour show: a combination of colours that go well together, laid
/// across the lights playing it, left to right, gliding into the next (see
/// [`spectrum`]). `place` is where the light stands among them, 0 at the
/// left to 1 at the right.
fn rainbow(t: f32, place: f32, bright: f32) -> BulbOutput {
    let rgb = spectrum(t, place);
    output(rgb, bright * even(rgb))
}

/// The colour show's colour at time `t` for a light at `place` (0 at the
/// left of the lights playing it, 1 at the right): the combination of the
/// moment laid across the lights, drifting gently from side to side.
///
/// Each combination lingers a while, then glides into the next, every colour
/// along the wheel to its place in it. The glide eases in and out without
/// ever stopping dead, so nothing holds and then lurches on.
fn spectrum(t: f32, place: f32) -> Rgb {
    let pos = (t / COMBO_SECS).rem_euclid(COMBOS.len() as f32);
    let i = (pos.floor() as usize).min(COMBOS.len() - 1);
    let u = pos - pos.floor();
    let glide = 0.25 * u + 0.75 * u * u * (3.0 - 2.0 * u);
    let (from, to) = (&COMBOS[i], &COMBOS[(i + 1) % COMBOS.len()]);
    let combo: [Rgb; 3] = std::array::from_fn(|k| clean_blend(from[k], to[k], glide));
    let x = (1.0 - 2.0 * DRIFT) * place + DRIFT * (1.0 + (TAU * t / DRIFT_SECS).sin());
    let at = x.clamp(0.0, 1.0) * 2.0;
    let k = (at.floor() as usize).min(1);
    clean_blend(combo[k], combo[k + 1], at - k as f32)
}

/// Where each light stands across the room, 0 at the left to 1 at the right.
///
/// Half its real position, half its rank: lights that stand close stay close
/// in colour, yet a cluster never shares one colour while a lone light far
/// off takes the rest of the gradient. Every step along the room is a step
/// along the spectrum.
fn places(fixtures: &[Fixture]) -> Vec<f32> {
    let n = fixtures.len();
    if n < 2 {
        return vec![0.5; n];
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| fixtures[a].x.total_cmp(&fixtures[b].x).then(a.cmp(&b)));
    let lo = fixtures[order[0]].x;
    let width = fixtures[order[n - 1]].x - lo;
    let mut out = vec![0.0; n];
    for (rank, &i) in order.iter().enumerate() {
        let by_rank = rank as f32 / (n - 1) as f32;
        let by_x = if width > 1e-3 {
            (fixtures[i].x - lo) / width
        } else {
            by_rank
        };
        out[i] = 0.5 * (by_rank + by_x);
    }
    out
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
        self.t = (self.t + dt) % CLOCK_WRAP;
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
        ShowSettings {
            kind,
            ..ShowSettings::default()
        }
    }

    /// Play `kind` on `n` lights for `secs`, frame by frame.
    fn play(kind: ShowKind, n: usize, secs: f32) -> Vec<Vec<BulbOutput>> {
        play_at(settings(kind), n, secs)
    }

    fn play_at(s: ShowSettings, n: usize, secs: f32) -> Vec<Vec<BulbOutput>> {
        let kind = s.kind;
        let mut show = Show::new(kind, &Fixture::row(n));
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
    fn the_pace_runs_from_a_quarter_to_four_times() {
        let at = |speed| {
            ShowSettings {
                speed,
                ..settings(ShowKind::Rainbow)
            }
            .pace()
        };
        assert!((at(50) - 1.0).abs() < 1e-6);
        assert!((at(100) - 4.0).abs() < 1e-4);
        assert!(at(1) > 0.24 && at(1) < 0.26);
        assert_eq!(at(0), at(1));
    }

    #[test]
    fn a_faster_show_covers_more_ground() {
        // At double pace, the rainbow is where it would be twice as late.
        let fast = ShowSettings {
            speed: 75,
            ..settings(ShowKind::Rainbow)
        };
        let quick = play_at(fast, 1, 5.0);
        let slow = play(ShowKind::Rainbow, 1, 10.0);
        let (a, _, _) = hsv(rgb(quick[quick.len() - 1][0]));
        let (b, _, _) = hsv(rgb(slow[slow.len() - 1][0]));
        assert!((a - b).abs() < 3.0, "{a}° vs {b}°");
    }

    #[test]
    fn the_fastest_shows_still_flow() {
        for kind in [ShowKind::Rainbow, ShowKind::Sunset, ShowKind::Ocean] {
            let s = ShowSettings {
                speed: 100,
                ..settings(kind)
            };
            for w in play_at(s, 3, 60.0).windows(2) {
                for (a, b) in w[0].iter().zip(&w[1]) {
                    assert!(
                        jump(rgb(*a), rgb(*b)) <= 40,
                        "{kind:?} jumps: {a:?} -> {b:?}"
                    );
                }
            }
        }
    }

    /// How far apart two colours are round the wheel, in degrees.
    fn gap(a: Rgb, b: Rgb) -> f32 {
        let ((a, _, _), (b, _, _)) = (hsv(a), hsv(b));
        (a - b).abs().min(360.0 - (a - b).abs())
    }

    /// Chartreuse and lime: the hues that look sickly on a bulb.
    fn sickly(c: Rgb) -> bool {
        (65.0..=125.0).contains(&hsv(c).0)
    }

    /// One whole loop through the combinations, in seconds.
    const LOOP: f32 = COMBO_SECS * COMBOS.len() as f32;

    #[test]
    fn the_rainbow_lays_a_combination_across_the_room() {
        // Left to right, each light its own colour, close enough to its
        // neighbour to blend, never a sickly colour, all the way round.
        for frame in play(ShowKind::Rainbow, 4, LOOP) {
            for w in frame.windows(2) {
                let g = gap(rgb(w[0]), rgb(w[1]));
                assert!((3.0..=60.0).contains(&g), "lights {g}° apart");
            }
            for o in &frame {
                assert!(!sickly(rgb(*o)), "a sickly colour: {:?}", rgb(*o));
            }
            assert!(gap(rgb(frame[0]), rgb(frame[3])) >= 20.0);
        }
    }

    #[test]
    fn the_combinations_go_well_together() {
        for (i, combo) in COMBOS.iter().enumerate() {
            let next = &COMBOS[(i + 1) % COMBOS.len()];
            for k in 0..3 {
                assert!(!sickly(combo[k]), "combination {i}: {:?}", combo[k]);
                // Each colour has only a short way to go to the next.
                let g = gap(combo[k], next[k]);
                assert!(g <= 110.0, "combination {i}, colour {k}: {g}°");
            }
            for w in combo.windows(2) {
                let g = gap(w[0], w[1]);
                assert!((10.0..=80.0).contains(&g), "combination {i}: {g}°");
            }
        }
    }

    #[test]
    fn a_glide_steers_clear_of_the_yellow_greens() {
        // Emerald to gold the short way is all chartreuse; the long way is not.
        let (emerald, gold) = (Rgb::new(0, 255, 110), Rgb::new(255, 160, 0));
        for k in 0..=20 {
            let c = clean_blend(emerald, gold, k as f32 / 20.0);
            assert!(!sickly(c), "{c:?}");
        }
        // Elsewhere it goes the short way, as a blend does.
        let mid = clean_blend(Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), 0.5);
        assert!(mid.r > 200 && mid.b > 200 && mid.g < 30, "{mid:?}");
    }

    #[test]
    fn the_rainbow_moves_through_every_combination() {
        // Over one loop, warm and cool alike, then back where it began.
        let frames = play(ShowKind::Rainbow, 1, LOOP);
        let hues: Vec<f32> = frames.iter().map(|f| hsv(rgb(f[0])).0).collect();
        let warm = |h: &f32| !(60.0..=330.0).contains(h);
        assert!(hues.iter().any(warm), "never warm");
        assert!(
            hues.iter().any(|h| (150.0..=230.0).contains(h)),
            "never cool"
        );
        let end = rgb(frames[frames.len() - 1][0]);
        assert!(gap(rgb(frames[0][0]), end) < 3.0, "the loop has a seam");
    }

    #[test]
    fn the_rainbow_never_holds_still() {
        // Even while a combination lingers, the colours drift.
        let frames = play(ShowKind::Rainbow, 2, COMBO_SECS);
        for w in frames.chunks(25).collect::<Vec<_>>().windows(2) {
            assert_ne!(w[0][0], w[1][0], "the show stalled");
        }
    }

    #[test]
    fn places_follow_the_room_left_to_right() {
        let mut room = Fixture::row(3);
        room.swap(0, 2);
        let p = places(&room);
        assert_eq!(p, vec![1.0, 0.5, 0.0]);
        assert_eq!(places(&Fixture::row(1)), vec![0.5]);
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
}
