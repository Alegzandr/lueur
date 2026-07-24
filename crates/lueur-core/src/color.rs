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
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorAccumulator {
    r2: f64,
    g2: f64,
    b2: f64,
    count: u64,
}

impl ColorAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn add(&mut self, r: u8, g: u8, b: u8) {
        let rf = r as f64;
        let gf = g as f64;
        let bf = b as f64;
        self.r2 += rf * rf;
        self.g2 += gf * gf;
        self.b2 += bf * bf;
        self.count += 1;
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    /// Finalise the average. Returns black when nothing was accumulated.
    pub fn average(&self) -> Rgb {
        if self.count == 0 {
            return Rgb::BLACK;
        }
        let n = self.count as f64;
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

/// Boost saturation by `amount` (0.0 = unchanged, 1.0 = fully saturated) and
/// scale overall brightness by `brightness` (0..=1). Applied after smoothing so
/// the mood settings feel immediate.
pub fn shape(color: Rgb, saturation: f32, brightness: f32) -> Rgb {
    let sat = saturation.clamp(0.0, 1.0);
    let bri = brightness.clamp(0.0, 1.0);

    let r = color.r as f32;
    let g = color.g as f32;
    let b = color.b as f32;
    // Grey point = luminance; push channels away from it to saturate.
    let grey = 0.299 * r + 0.587 * g + 0.114 * b;
    let sr = grey + (r - grey) * (1.0 + sat);
    let sg = grey + (g - grey) * (1.0 + sat);
    let sb = grey + (b - grey) * (1.0 + sat);

    Rgb {
        r: (sr * bri).round().clamp(0.0, 255.0) as u8,
        g: (sg * bri).round().clamp(0.0, 255.0) as u8,
        b: (sb * bri).round().clamp(0.0, 255.0) as u8,
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
}
