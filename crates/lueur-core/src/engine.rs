//! The ambilight engine: turn a captured frame into one colour per light.
//!
//! This module is deliberately pure and synchronous — given a [`Frame`] and the
//! set of [`SampleZone`]s, it returns the colours to send. All timing, capture
//! and network I/O live in the desktop shell. Keeping it pure means the
//! colour-critical path is fully deterministic and unit-tested.

use crate::color::{saturate, saturation, BulbOutput, ColorAccumulator, ColorSmoother, Rgb};
use crate::geometry::{LightMapping, SampleZone};
use serde::{Deserialize, Serialize};

/// Byte order of a captured frame's pixels. Screen-capture backends differ, so
/// we let the caller tell us rather than guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    Rgba,
    Bgra,
}

/// A captured frame: a tightly-packed 4-bytes-per-pixel buffer.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    /// `width * height * 4` bytes, row-major, top-left origin.
    pub data: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Self {
        Frame {
            width,
            height,
            format,
            data,
        }
    }

    #[inline]
    fn pixel(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        // Bounds are guaranteed by the caller's clamped zone maths, but stay safe.
        match self.data.get(idx..idx + 4) {
            Some(px) => match self.format {
                PixelFormat::Rgba => (px[0], px[1], px[2]),
                PixelFormat::Bgra => (px[2], px[1], px[0]),
            },
            None => (0, 0, 0),
        }
    }

    /// Average the colour inside a normalised zone, visiting roughly every
    /// `step`-th pixel in each axis (>=1). Larger `step` = cheaper, slightly
    /// noisier — a good latency/quality knob.
    ///
    /// Pixels are weighted by a separable bell curve centred on the zone, so a
    /// zone's edge fades out rather than cutting off. Without it, two adjacent
    /// lights swing apart the instant a bright object crosses the hard border
    /// between their rectangles; with it, the handover is gradual. The weights
    /// are computed once per row and once per column, not per pixel.
    pub fn average_zone(&self, zone: &SampleZone, step: u32) -> Rgb {
        if self.width == 0 || self.height == 0 {
            return Rgb::BLACK;
        }
        let step = step.max(1);

        let x0 = (zone.x * self.width as f32).floor() as u32;
        let y0 = (zone.y * self.height as f32).floor() as u32;
        let x1 = ((zone.x + zone.w) * self.width as f32).ceil() as u32;
        let y1 = ((zone.y + zone.h) * self.height as f32).ceil() as u32;

        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return Rgb::BLACK;
        }

        let cx = (x0 + x1) as f32 * 0.5;
        let cy = (y0 + y1) as f32 * 0.5;
        let hx = ((x1 - x0) as f32 * 0.5).max(1.0);
        let hy = ((y1 - y0) as f32 * 0.5).max(1.0);

        // Column weights, computed once and reused for every row.
        let mut col_w: Vec<f32> = Vec::with_capacity(((x1 - x0) / step + 1) as usize);
        let mut x = x0;
        while x < x1 {
            col_w.push(taper((x as f32 + 0.5 - cx) / hx));
            x += step;
        }

        let mut acc = ColorAccumulator::new();
        let mut y = y0;
        while y < y1 {
            let row_w = taper((y as f32 + 0.5 - cy) / hy);
            let mut x = x0;
            let mut i = 0usize;
            while x < x1 {
                let (r, g, b) = self.pixel(x, y);
                acc.add_weighted(r, g, b, row_w * col_w[i]);
                i += 1;
                x += step;
            }
            y += step;
        }
        acc.average()
    }
}

/// Sample weight at normalised distance `d` from a zone's centre (0 = centre,
/// 1 = edge). A Gaussian, kept wide enough that edge pixels still contribute
/// (~0.14) — the goal is a soft handover, not a spotlight on the centre.
#[inline]
fn taper(d: f32) -> f32 {
    (-2.0 * d * d).exp()
}

/// Runtime settings for the engine. These map directly onto the UI's sliders.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EngineSettings {
    /// Weight of each new frame in the temporal smoother (0..1). Higher =
    /// snappier / lower perceived latency; lower = smoother / cosier.
    pub smoothing: f32,
    /// Overall output brightness scale (0..1).
    pub brightness: f32,
    /// Saturation boost (0 = untouched, 1 = punchy neon).
    pub saturation: f32,
    /// Pixel step used when averaging a zone (>=1). Trades quality for CPU.
    pub sample_step: u32,
    /// Colours whose luminance is below this are pulled to black, so dark scenes
    /// don't leave a muddy glow. 0 disables.
    pub black_threshold: u8,
    /// HSV saturation (0..1) below which a frame is considered achromatic and the
    /// bulb is driven as a colour-temperature white instead of an RGB colour.
    ///
    /// Below this, the hue is noise: a grey-ish frame has no meaningful colour,
    /// and asking an RGB bulb for near-white makes it show a pink/magenta cast.
    /// 0 disables the white path entirely.
    #[serde(default = "default_white_threshold")]
    pub white_threshold: f32,
    /// Colour temperature used by the white path, in Kelvin (1700..6500).
    #[serde(default = "default_white_kelvin")]
    pub white_kelvin: u16,
}

fn default_white_threshold() -> f32 {
    0.18
}

fn default_white_kelvin() -> u16 {
    4000
}

impl Default for EngineSettings {
    fn default() -> Self {
        EngineSettings {
            smoothing: 0.55,
            brightness: 1.0,
            saturation: 0.35,
            sample_step: 4,
            black_threshold: 6,
            white_threshold: default_white_threshold(),
            white_kelvin: default_white_kelvin(),
        }
    }
}

/// How far saturation must climb back above `white_threshold` before we leave the
/// white path. Without this margin, a frame hovering at the threshold would
/// flip-flop between white and colour every frame.
const WHITE_HYSTERESIS: f32 = 1.6;

/// Holds per-light smoothing state across frames.
pub struct Ambilight {
    settings: EngineSettings,
    smoothers: Vec<ColorSmoother>,
    /// Whether each light is currently on the white path. Latched so the
    /// white/colour decision gets hysteresis instead of chattering.
    in_white: Vec<bool>,
}

impl Ambilight {
    pub fn new(light_count: usize, settings: EngineSettings) -> Self {
        Ambilight {
            settings,
            smoothers: vec![ColorSmoother::new(); light_count],
            in_white: vec![false; light_count],
        }
    }

    pub fn settings(&self) -> &EngineSettings {
        &self.settings
    }

    /// Update settings live (e.g. the user drags a slider). Resizing the light
    /// count is handled separately via [`Ambilight::resize`].
    pub fn set_settings(&mut self, settings: EngineSettings) {
        self.settings = settings;
    }

    /// Change how many lights we track, preserving existing smoother state where
    /// possible.
    pub fn resize(&mut self, light_count: usize) {
        self.smoothers.resize(light_count, ColorSmoother::new());
        self.in_white.resize(light_count, false);
    }

    /// Compute the screen-referred colour for each light this frame.
    ///
    /// This is the colour *as seen on screen* (sampled, smoothed, saturated) —
    /// brightness is deliberately not applied here, because bulbs take intensity
    /// on their own channel. Use [`Ambilight::process_outputs`] to drive hardware.
    ///
    /// `mappings` must be the same length as the configured light count; extra
    /// or missing entries are handled gracefully (clamped to the shorter
    /// length). Each mapping's `responsiveness` scales the smoothing factor, so
    /// a lamp across the room drifts rather than snaps.
    pub fn process(&mut self, frame: &Frame, mappings: &[LightMapping]) -> Vec<Rgb> {
        let n = mappings.len().min(self.smoothers.len());
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let m = &mappings[i];
            let raw = frame.average_zone(&m.zone, self.settings.sample_step);
            // Floor the factor above 0: a zero would freeze the light forever.
            let smoothing = (self.settings.smoothing * m.responsiveness).clamp(0.01, 1.0);
            let smoothed = self.smoothers[i].push(raw, smoothing);
            let mut shaped = saturate(smoothed, self.settings.saturation);
            if self.settings.black_threshold > 0
                && shaped.luminance() < self.settings.black_threshold
            {
                shaped = Rgb::BLACK;
            }
            out.push(shaped);
        }
        out
    }

    /// Compute what to actually push to each bulb this frame.
    ///
    /// Splits every colour into chromaticity + brightness, and falls back to a
    /// colour-temperature white when the frame carries no usable hue.
    ///
    /// This is where a mapping's distance `gain` lands: it scales the bulb's
    /// intensity, never its hue, so a far lamp reads as a softer glow of the
    /// same colour rather than a washed-out one.
    pub fn process_outputs(&mut self, frame: &Frame, mappings: &[LightMapping]) -> Vec<BulbOutput> {
        let colors = self.process(frame, mappings);
        let brightness = self.settings.brightness;
        let kelvin = self.settings.white_kelvin;
        let enter = self.settings.white_threshold.clamp(0.0, 1.0);
        let leave = (enter * WHITE_HYSTERESIS).min(1.0);

        colors
            .into_iter()
            .enumerate()
            .map(|(i, color)| {
                let gain = mappings.get(i).map(|m| m.gain).unwrap_or(1.0);
                let brightness = (brightness * gain).clamp(0.0, 1.0);
                let sat = saturation(color);
                // A latched decision with two thresholds: drop into white below
                // `enter`, but only climb back out once clearly past `leave`.
                let white = if enter <= 0.0 {
                    false
                } else if self.in_white[i] {
                    sat <= leave
                } else {
                    sat < enter
                };
                self.in_white[i] = white;
                BulbOutput::build(color, brightness, white, kelvin)
            })
            .collect()
    }

    /// Reset all smoothing state (e.g. when (re)starting the effect).
    pub fn reset(&mut self) {
        for s in &mut self.smoothers {
            s.reset();
        }
        for w in &mut self.in_white {
            *w = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a frame split left/right into two solid colours.
    fn split_frame(left: (u8, u8, u8), right: (u8, u8, u8)) -> Frame {
        let (w, h) = (100u32, 50u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                let (r, g, b) = if x < w / 2 { left } else { right };
                data[idx] = r;
                data[idx + 1] = g;
                data[idx + 2] = b;
                data[idx + 3] = 255;
            }
        }
        Frame::new(w, h, PixelFormat::Rgba, data)
    }

    #[test]
    fn average_zone_reads_correct_region() {
        let f = split_frame((255, 0, 0), (0, 0, 255));
        let left = f.average_zone(&SampleZone::new(0.0, 0.0, 0.2, 1.0), 1);
        let right = f.average_zone(&SampleZone::new(0.8, 0.0, 0.2, 1.0), 1);
        assert_eq!(left, Rgb::new(255, 0, 0));
        assert_eq!(right, Rgb::new(0, 0, 255));
    }

    #[test]
    fn bgra_channels_are_swapped() {
        let mut data = vec![0u8; 4];
        // BGRA bytes for pure red = [0,0,255,255].
        data[0] = 0;
        data[1] = 0;
        data[2] = 255;
        data[3] = 255;
        let f = Frame::new(1, 1, PixelFormat::Bgra, data);
        assert_eq!(f.average_zone(&SampleZone::new(0.0, 0.0, 1.0, 1.0), 1), Rgb::new(255, 0, 0));
    }

    /// A mapping that samples the given zone with no distance shaping.
    fn z(x: f32, y: f32, w: f32, h: f32) -> LightMapping {
        LightMapping::direct(SampleZone::new(x, y, w, h))
    }

    /// One light sampling the whole frame.
    fn full() -> [LightMapping; 1] {
        [z(0.0, 0.0, 1.0, 1.0)]
    }

    /// Settings that pass colours through untouched, so a test sees exactly what
    /// it put in.
    fn passthrough() -> EngineSettings {
        EngineSettings {
            smoothing: 1.0,
            brightness: 1.0,
            saturation: 0.0,
            sample_step: 1,
            black_threshold: 0,
            white_threshold: 0.0,
            white_kelvin: 4000,
        }
    }

    #[test]
    fn process_maps_zones_to_colors() {
        let f = split_frame((200, 0, 0), (0, 0, 200));
        let settings = passthrough();
        let mut amb = Ambilight::new(2, settings);
        let zones = vec![z(0.0, 0.0, 0.2, 1.0), z(0.8, 0.0, 0.2, 1.0)];
        let out = amb.process(&f, &zones);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Rgb::new(200, 0, 0));
        assert_eq!(out[1], Rgb::new(0, 0, 200));
    }

    #[test]
    fn black_threshold_kills_dim_output() {
        let f = split_frame((3, 3, 3), (3, 3, 3));
        let settings = EngineSettings {
            black_threshold: 10,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let out = amb.process(&f, &full());
        assert_eq!(out[0], Rgb::BLACK);
    }

    #[test]
    fn process_does_not_bake_brightness_into_rgb() {
        // Brightness belongs on the bulb's own channel; scaling RGB here would be
        // discarded by set_rgb and silently do nothing.
        let f = split_frame((200, 100, 50), (200, 100, 50));
        let settings = EngineSettings {
            brightness: 0.25,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let out = amb.process(&f, &full());
        assert_eq!(out[0], Rgb::new(200, 100, 50));
    }

    #[test]
    fn outputs_split_hue_from_intensity() {
        // A dim but clearly chromatic frame: the bulb must get full-chroma RGB
        // plus a low brightness, not a dim RGB triple.
        let f = split_frame((40, 10, 8), (40, 10, 8));
        let settings = EngineSettings {
            white_threshold: 0.18,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let out = amb.process_outputs(&f, &full());
        match out[0] {
            BulbOutput::Color { rgb, bright } => {
                assert_eq!(rgb.r, 255, "chromaticity should be normalised");
                assert!(bright < 15, "dark frame should dim the bulb, got {bright}");
            }
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn achromatic_frame_takes_the_white_path() {
        // This is the wallpaper colour measured off the real screenshot: 8%
        // saturation. Driving an RGB bulb with it is what produced the pink.
        let f = split_frame((63, 61, 66), (63, 61, 66));
        let settings = EngineSettings {
            white_threshold: 0.18,
            white_kelvin: 4000,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let out = amb.process_outputs(&f, &full());
        match out[0] {
            BulbOutput::White { kelvin, bright } => {
                assert_eq!(kelvin, 4000);
                assert!((20..=30).contains(&bright), "got {bright}");
            }
            other => panic!("expected White, got {other:?}"),
        }
    }

    #[test]
    fn zero_white_threshold_disables_the_white_path() {
        let f = split_frame((63, 61, 66), (63, 61, 66));
        let mut amb = Ambilight::new(1, passthrough());
        let out = amb.process_outputs(&f, &full());
        assert!(matches!(out[0], BulbOutput::Color { .. }));
    }

    #[test]
    fn white_decision_has_hysteresis() {
        let settings = EngineSettings {
            white_threshold: 0.2, // leaves white only above 0.32
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let zone = full();

        // Latch into white on a neutral frame.
        let grey = split_frame((80, 80, 80), (80, 80, 80));
        assert!(matches!(
            amb.process_outputs(&grey, &zone)[0],
            BulbOutput::White { .. }
        ));

        // 25% saturation is past `enter` but inside the hysteresis band: stay white.
        let faint = split_frame((80, 66, 60), (80, 66, 60));
        assert!(
            (0.2..0.32).contains(&crate::color::saturation(Rgb::new(80, 66, 60))),
            "test fixture must sit inside the hysteresis band"
        );
        assert!(matches!(
            amb.process_outputs(&faint, &zone)[0],
            BulbOutput::White { .. }
        ));

        // Clearly chromatic: leave white.
        let vivid = split_frame((200, 40, 40), (200, 40, 40));
        assert!(matches!(
            amb.process_outputs(&vivid, &zone)[0],
            BulbOutput::Color { .. }
        ));
    }

    #[test]
    fn reset_clears_white_latch() {
        let settings = EngineSettings {
            white_threshold: 0.2,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let zone = full();
        let grey = split_frame((80, 80, 80), (80, 80, 80));
        amb.process_outputs(&grey, &zone);
        amb.reset();
        // With the latch cleared, a mid-band frame must be judged on `enter` alone.
        let faint = split_frame((80, 66, 60), (80, 66, 60));
        assert!(matches!(
            amb.process_outputs(&faint, &zone)[0],
            BulbOutput::Color { .. }
        ));
    }

    #[test]
    fn resize_keeps_white_latch_in_step() {
        let mut amb = Ambilight::new(1, EngineSettings::default());
        amb.resize(4);
        let f = split_frame((10, 20, 30), (10, 20, 30));
        let zones = vec![z(0.0, 0.0, 1.0, 1.0); 4];
        // Would panic on an out-of-bounds `in_white` index if resize missed it.
        assert_eq!(amb.process_outputs(&f, &zones).len(), 4);
    }

    #[test]
    fn resize_grows_and_shrinks() {
        let mut amb = Ambilight::new(2, EngineSettings::default());
        amb.resize(5);
        let f = split_frame((10, 20, 30), (10, 20, 30));
        let zones = vec![z(0.0, 0.0, 1.0, 1.0); 5];
        assert_eq!(amb.process(&f, &zones).len(), 5);
        amb.resize(1);
        assert_eq!(amb.process(&f, &zones).len(), 1);
    }

    #[test]
    fn empty_frame_is_safe() {
        let f = Frame::new(0, 0, PixelFormat::Rgba, vec![]);
        assert_eq!(f.average_zone(&SampleZone::new(0.0, 0.0, 1.0, 1.0), 2), Rgb::BLACK);
    }
}
