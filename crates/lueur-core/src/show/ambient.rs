//! The shows that need no music: the colours, the sunset, the sea and the
//! candle.

use super::colour::{blend, clean_blend, even, output, palette_at, ramp};
use super::palettes::{COMBOS, EMBER, FLAME, OCEAN, SUNSET};
use super::{CLOCK_WRAP, ShowSettings};
use crate::color::{BulbOutput, Rgb};
use crate::stage::Fixture;
use std::f32::consts::TAU;

/// Seconds the colour show takes from one combination to the next.
pub(super) const COMBO_SECS: f32 = 25.0;
/// Seconds for the colours to drift across the lights and back, so the room
/// never sits still even while a combination holds.
const DRIFT_SECS: f32 = 20.0;
/// How far the drift carries the colours, as a share of the room.
const DRIFT: f32 = 0.12;

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
pub(super) fn noise(seed: u32, x: f32) -> f32 {
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
pub(super) fn rainbow(t: f32, place: f32, bright: f32) -> BulbOutput {
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
pub(super) fn places(fixtures: &[Fixture]) -> Vec<f32> {
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
pub(super) fn sunset(t: f32, i: usize, n: usize, bright: f32) -> BulbOutput {
    let pos = t / 14.0 + spread(i, n, 1.6) + 0.9 * noise(seed(i, 1), t / 23.0);
    let rgb = palette_at(SUNSET, pos);
    let swell = 0.86 + 0.14 * noise(seed(i, 2), t / 9.0);
    output(rgb, bright * swell * even(rgb))
}

/// The sea: a swell rolls through the room from one light to the next,
/// never quite regular. Each crest lifts the light and turns it to teal;
/// each trough sinks it to a deep blue. The tide slowly drifts the whole
/// sea bluer or greener.
pub(super) fn ocean(t: f32, i: usize, n: usize, bright: f32) -> BulbOutput {
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
