//! Colour types and the small amount of colour maths the ambilight engine needs.
//!
//! Everything here is deliberately allocation-free and cheap: it runs once per
//! light per frame, up to ~60 times a second, so it must stay tight.

use serde::{Deserialize, Serialize};

/// An 8-bit sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Pack into the 24-bit integer Yeelight expects (`0xRRGGBB`).
    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Perceived luminance in 0..=255 (Rec. 601 weights). Cheap and good enough
    /// for deciding whether a frame is "dark".
    pub fn luminance(self) -> u8 {
        let y = 0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32;
        y.round().clamp(0.0, 255.0) as u8
    }
}

/// A colour accumulated in linear-ish floating point space.
///
/// Averaging colours in raw sRGB space muddies them (bright reds next to blues
/// average to a dull grey). We accumulate the *squared* channel values and take
/// the square root at the end, which is a fast approximation of averaging in
/// linear light and keeps colours vivid — exactly what a gamer wants at night.
///
/// Samples can carry a weight, which is how the engine tapers a zone's edges:
/// a pixel drifting into the zone arrives at near-zero weight instead of
/// jumping in at full strength, so neighbouring lights stop disagreeing
/// abruptly when something bright crosses the boundary between them.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorAccumulator {
    r2: f64,
    g2: f64,
    b2: f64,
    weight: f64,
    count: u64,
}

impl ColorAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn add(&mut self, r: u8, g: u8, b: u8) {
        self.add_weighted(r, g, b, 1.0);
    }

    /// Add a sample contributing `w` (non-positive weights are ignored).
    #[inline]
    pub fn add_weighted(&mut self, r: u8, g: u8, b: u8, w: f32) {
        if !(w > 0.0) {
            return;
        }
        let w = w as f64;
        let rf = r as f64;
        let gf = g as f64;
        let bf = b as f64;
        self.r2 += w * rf * rf;
        self.g2 += w * gf * gf;
        self.b2 += w * bf * bf;
        self.weight += w;
        self.count += 1;
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    /// Finalise the average. Returns black when nothing was accumulated.
    pub fn average(&self) -> Rgb {
        if self.weight <= 0.0 {
            return Rgb::BLACK;
        }
        let n = self.weight;
        Rgb {
            r: (self.r2 / n).sqrt().round().clamp(0.0, 255.0) as u8,
            g: (self.g2 / n).sqrt().round().clamp(0.0, 255.0) as u8,
            b: (self.b2 / n).sqrt().round().clamp(0.0, 255.0) as u8,
        }
    }
}

/// Per-light exponential smoothing plus brightness/saturation shaping.
///
/// Keeps one smoothed colour and blends each new target into it. `factor` is the
/// weight of the *new* sample: 1.0 = instant (no smoothing, minimum latency),
/// small values = slow and dreamy. Gamers wanting the snappiest response set it
/// high; a cosy movie-night mood sets it low.
#[derive(Debug, Clone, Copy)]
pub struct ColorSmoother {
    r: f32,
    g: f32,
    b: f32,
    seeded: bool,
}

impl Default for ColorSmoother {
    fn default() -> Self {
        Self {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            seeded: false,
        }
    }
}

impl ColorSmoother {
    pub fn new() -> Self {
        Self::default()
    }

    /// Blend `target` into the smoothed value with weight `factor` (clamped to
    /// 0..=1) and return the new smoothed colour. The first call snaps directly
    /// to the target so we don't fade up from black on start.
    pub fn push(&mut self, target: Rgb, factor: f32) -> Rgb {
        let f = factor.clamp(0.0, 1.0);
        if !self.seeded {
            self.r = target.r as f32;
            self.g = target.g as f32;
            self.b = target.b as f32;
            self.seeded = true;
        } else {
            self.r += (target.r as f32 - self.r) * f;
            self.g += (target.g as f32 - self.g) * f;
            self.b += (target.b as f32 - self.b) * f;
        }
        Rgb {
            r: self.r.round().clamp(0.0, 255.0) as u8,
            g: self.g.round().clamp(0.0, 255.0) as u8,
            b: self.b.round().clamp(0.0, 255.0) as u8,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Boost saturation by `amount` (0.0 = unchanged, 1.0 = fully saturated).
///
/// Pushes each channel away from the colour's own grey point (its luminance),
/// which preserves hue while making it read more strongly.
pub fn saturate(color: Rgb, amount: f32) -> Rgb {
    let sat = amount.clamp(0.0, 1.0);

    let r = color.r as f32;
    let g = color.g as f32;
    let b = color.b as f32;
    // Grey point = luminance; push channels away from it to saturate.
    let grey = 0.299 * r + 0.587 * g + 0.114 * b;

    Rgb {
        r: (grey + (r - grey) * (1.0 + sat)).round().clamp(0.0, 255.0) as u8,
        g: (grey + (g - grey) * (1.0 + sat)).round().clamp(0.0, 255.0) as u8,
        b: (grey + (b - grey) * (1.0 + sat)).round().clamp(0.0, 255.0) as u8,
    }
}

/// [`saturate`] followed by an overall brightness scale (0..=1).
///
/// Kept for callers that want a single screen-referred colour (previews, tests).
/// The live path does *not* use this: bulbs take brightness on their own channel,
/// so scaling RGB there would be silently discarded — see [`BulbOutput`].
pub fn shape(color: Rgb, saturation: f32, brightness: f32) -> Rgb {
    let bri = brightness.clamp(0.0, 1.0);
    let c = saturate(color, saturation);
    Rgb {
        r: (c.r as f32 * bri).round().clamp(0.0, 255.0) as u8,
        g: (c.g as f32 * bri).round().clamp(0.0, 255.0) as u8,
        b: (c.b as f32 * bri).round().clamp(0.0, 255.0) as u8,
    }
}

/// HSV saturation of a colour, 0.0 (neutral grey) .. 1.0 (fully chromatic).
///
/// This is the measure that decides whether a colour carries a *usable* hue.
/// Black returns 0.
pub fn saturation(color: Rgb) -> f32 {
    let max = color.r.max(color.g).max(color.b);
    if max == 0 {
        return 0.0;
    }
    let min = color.r.min(color.g).min(color.b);
    (max - min) as f32 / max as f32
}

/// Scale a colour so its brightest channel hits 255, preserving hue and HSV
/// saturation exactly. Black is returned unchanged.
///
/// This is what a Yeelight bulb actually wants from `set_rgb`: the command sets
/// *chromaticity* only, and the bulb applies its own `bright` property on top. A
/// dim triple like `rgb(63, 61, 66)` is therefore rendered at whatever
/// brightness the bulb already had — so we must hand it full-chroma RGB and pass
/// the intensity separately via `set_bright`.
pub fn full_chroma(color: Rgb) -> Rgb {
    let max = color.r.max(color.g).max(color.b);
    if max == 0 {
        return Rgb::BLACK;
    }
    let scale = 255.0 / max as f32;
    Rgb {
        r: (color.r as f32 * scale).round().clamp(0.0, 255.0) as u8,
        g: (color.g as f32 * scale).round().clamp(0.0, 255.0) as u8,
        b: (color.b as f32 * scale).round().clamp(0.0, 255.0) as u8,
    }
}

/// Yeelight's `bright` range.
pub const MIN_BRIGHT: u8 = 1;
pub const MAX_BRIGHT: u8 = 100;

/// Map a colour's luminance onto a bulb brightness percentage (1..=100),
/// scaled by the user's overall `brightness` setting (0..=1).
///
/// The bulb cannot go below 1%, so a fully black frame lands on 1 rather than
/// off — turning the bulb off and on per frame would strobe.
pub fn bulb_brightness(color: Rgb, brightness: f32) -> u8 {
    let bri = brightness.clamp(0.0, 1.0);
    let pct = (color.luminance() as f32 / 255.0) * bri * 100.0;
    (pct.round().clamp(MIN_BRIGHT as f32, MAX_BRIGHT as f32)) as u8
}

/// Yeelight's supported colour-temperature range, in Kelvin.
pub const MIN_KELVIN: u16 = 1700;
pub const MAX_KELVIN: u16 = 6500;

/// What to actually push to a bulb for one computed colour.
///
/// Splitting hue from intensity is not an optimisation — it is the only correct
/// way to drive a Yeelight. See [`full_chroma`] for why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BulbOutput {
    /// The frame is chromatic enough for a hue to mean something.
    Color { rgb: Rgb, bright: u8 },
    /// The frame is too close to neutral for a hue to mean anything. Asking an
    /// RGB bulb for near-white makes it mix its LEDs uncalibrated, which reads
    /// as a pink/magenta cast; a real colour-temperature white is both accurate
    /// and what the eye expects from a grey scene.
    White { kelvin: u16, bright: u8 },
}

impl BulbOutput {
    pub fn bright(self) -> u8 {
        match self {
            BulbOutput::Color { bright, .. } | BulbOutput::White { bright, .. } => bright,
        }
    }

    /// Build the output for `color`, given whether the white path was chosen.
    /// The choice itself (with hysteresis) lives in the engine, which owns the
    /// per-light state needed to keep it stable across frames.
    pub fn build(color: Rgb, brightness: f32, white: bool, kelvin: u16) -> Self {
        let bright = bulb_brightness(color, brightness);
        if white {
            BulbOutput::White {
                kelvin: kelvin.clamp(MIN_KELVIN, MAX_KELVIN),
                bright,
            }
        } else {
            BulbOutput::Color {
                rgb: full_chroma(color),
                bright,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_u32_packs_channels() {
        assert_eq!(Rgb::new(0x12, 0x34, 0x56).to_u32(), 0x123456);
        assert_eq!(Rgb::new(255, 255, 255).to_u32(), 0xFFFFFF);
        assert_eq!(Rgb::BLACK.to_u32(), 0);
    }

    #[test]
    fn accumulator_averages_in_linear_space() {
        let mut acc = ColorAccumulator::new();
        acc.add(0, 0, 0);
        acc.add(255, 255, 255);
        let avg = acc.average();
        // Linear-space average of black and white is brighter than 128.
        assert!(avg.r > 170, "expected vivid mid, got {}", avg.r);
    }

    #[test]
    fn weights_shift_the_average() {
        // A nearly-zero-weight white barely moves an otherwise black average —
        // this is what makes a zone's edge taper instead of jump.
        let mut acc = ColorAccumulator::new();
        acc.add(0, 0, 0);
        acc.add_weighted(255, 255, 255, 0.01);
        assert!(acc.average().r < 40, "got {}", acc.average().r);
    }

    #[test]
    fn zero_weight_samples_are_ignored() {
        let mut acc = ColorAccumulator::new();
        acc.add_weighted(255, 255, 255, 0.0);
        assert_eq!(acc.average(), Rgb::BLACK);
        assert_eq!(acc.count(), 0);
    }

    #[test]
    fn empty_accumulator_is_black() {
        assert_eq!(ColorAccumulator::new().average(), Rgb::BLACK);
    }

    #[test]
    fn smoother_snaps_on_first_sample() {
        let mut s = ColorSmoother::new();
        let out = s.push(Rgb::new(200, 100, 50), 0.1);
        assert_eq!(out, Rgb::new(200, 100, 50));
    }

    #[test]
    fn smoother_moves_toward_target() {
        let mut s = ColorSmoother::new();
        s.push(Rgb::new(0, 0, 0), 0.5); // seed at black
        let out = s.push(Rgb::new(100, 100, 100), 0.5);
        assert_eq!(out, Rgb::new(50, 50, 50));
    }

    #[test]
    fn full_factor_is_instant() {
        let mut s = ColorSmoother::new();
        s.push(Rgb::BLACK, 1.0);
        let out = s.push(Rgb::new(10, 20, 30), 1.0);
        assert_eq!(out, Rgb::new(10, 20, 30));
    }

    #[test]
    fn shape_zero_brightness_is_black() {
        assert_eq!(shape(Rgb::new(200, 50, 10), 0.0, 0.0), Rgb::BLACK);
    }

    #[test]
    fn shape_identity() {
        let c = Rgb::new(120, 130, 140);
        assert_eq!(shape(c, 0.0, 1.0), c);
    }

    #[test]
    fn saturate_preserves_grey() {
        // A neutral grey has nothing to push away from its grey point.
        assert_eq!(saturate(Rgb::new(80, 80, 80), 1.0), Rgb::new(80, 80, 80));
    }

    #[test]
    fn saturation_measures_chroma() {
        assert_eq!(saturation(Rgb::new(100, 100, 100)), 0.0);
        assert_eq!(saturation(Rgb::new(255, 0, 0)), 1.0);
        assert_eq!(saturation(Rgb::BLACK), 0.0);
        // The colour measured off the real wallpaper: barely any hue at all.
        assert!(saturation(Rgb::new(63, 61, 66)) < 0.1);
    }

    #[test]
    fn full_chroma_preserves_hue_and_saturation() {
        let dim = Rgb::new(30, 15, 60);
        let full = full_chroma(dim);
        assert_eq!(
            full.r.max(full.g).max(full.b),
            255,
            "should hit the ceiling"
        );
        // Channel ratios — and therefore hue — must survive the scaling.
        let ratio_before = dim.r as f32 / dim.b as f32;
        let ratio_after = full.r as f32 / full.b as f32;
        assert!((ratio_before - ratio_after).abs() < 0.02, "hue drifted");
        assert!((saturation(dim) - saturation(full)).abs() < 0.02);
    }

    #[test]
    fn full_chroma_leaves_black_alone() {
        assert_eq!(full_chroma(Rgb::BLACK), Rgb::BLACK);
    }

    #[test]
    fn bulb_brightness_tracks_luminance() {
        assert_eq!(bulb_brightness(Rgb::new(255, 255, 255), 1.0), 100);
        // A dark frame must dim the bulb — this is the intensity dimension that
        // set_rgb alone can never convey.
        let dark = bulb_brightness(Rgb::new(63, 61, 66), 1.0);
        assert!((20..=30).contains(&dark), "expected a dim bulb, got {dark}");
        // Never 0: the bulb's floor is 1%, and toggling power would strobe.
        assert_eq!(bulb_brightness(Rgb::BLACK, 1.0), MIN_BRIGHT);
    }

    #[test]
    fn bulb_brightness_honours_user_scale() {
        let full = bulb_brightness(Rgb::new(200, 200, 200), 1.0);
        let half = bulb_brightness(Rgb::new(200, 200, 200), 0.5);
        assert!(
            (half as i32 * 2 - full as i32).abs() <= 1,
            "{half} vs {full}"
        );
    }

    #[test]
    fn bulb_output_color_is_full_chroma_plus_bright() {
        match BulbOutput::build(Rgb::new(30, 15, 60), 1.0, false, 2700) {
            BulbOutput::Color { rgb, bright } => {
                assert_eq!(rgb.r.max(rgb.g).max(rgb.b), 255);
                assert!(bright > 0 && bright < 20, "dim scene, got {bright}");
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn bulb_output_white_clamps_kelvin() {
        match BulbOutput::build(Rgb::new(80, 80, 80), 1.0, true, 100) {
            BulbOutput::White { kelvin, .. } => assert_eq!(kelvin, MIN_KELVIN),
            other => panic!("expected White, got {other:?}"),
        }
        match BulbOutput::build(Rgb::new(80, 80, 80), 1.0, true, u16::MAX) {
            BulbOutput::White { kelvin, .. } => assert_eq!(kelvin, MAX_KELVIN),
            other => panic!("expected White, got {other:?}"),
        }
    }
}
