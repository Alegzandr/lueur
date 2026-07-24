//! The ambilight engine: turn a captured frame into one colour per light.
//!
//! This module is deliberately pure and synchronous — given a [`Frame`] and the
//! set of [`SampleZone`]s, it returns the colours to send. All timing, capture
//! and network I/O live in the desktop shell. Keeping it pure means the
//! colour-critical path is fully deterministic and unit-tested.

use crate::color::{shape, ColorAccumulator, ColorSmoother, Rgb};
use crate::geometry::SampleZone;
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

        let mut acc = ColorAccumulator::new();
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                let (r, g, b) = self.pixel(x, y);
                acc.add(r, g, b);
                x += step;
            }
            y += step;
        }
        acc.average()
    }
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
}

impl Default for EngineSettings {
    fn default() -> Self {
        EngineSettings {
            smoothing: 0.55,
            brightness: 1.0,
            saturation: 0.35,
            sample_step: 4,
            black_threshold: 6,
        }
    }
}

/// Holds per-light smoothing state across frames.
pub struct Ambilight {
    settings: EngineSettings,
    smoothers: Vec<ColorSmoother>,
}

impl Ambilight {
    pub fn new(light_count: usize, settings: EngineSettings) -> Self {
        Ambilight {
            settings,
            smoothers: vec![ColorSmoother::new(); light_count],
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
    }

    /// Compute the colour to send to each light for this frame.
    ///
    /// `zones` must be the same length as the configured light count; extra or
    /// missing zones are handled gracefully (clamped to the shorter length).
    pub fn process(&mut self, frame: &Frame, zones: &[SampleZone]) -> Vec<Rgb> {
        let n = zones.len().min(self.smoothers.len());
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let raw = frame.average_zone(&zones[i], self.settings.sample_step);
            let smoothed = self.smoothers[i].push(raw, self.settings.smoothing);
            let mut shaped = shape(smoothed, self.settings.saturation, self.settings.brightness);
            if self.settings.black_threshold > 0
                && shaped.luminance() < self.settings.black_threshold
            {
                shaped = Rgb::BLACK;
            }
            out.push(shaped);
        }
        out
    }

    /// Reset all smoothing state (e.g. when (re)starting the effect).
    pub fn reset(&mut self) {
        for s in &mut self.smoothers {
            s.reset();
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

    #[test]
    fn process_maps_zones_to_colors() {
        let f = split_frame((200, 0, 0), (0, 0, 200));
        let settings = EngineSettings {
            smoothing: 1.0,
            brightness: 1.0,
            saturation: 0.0,
            sample_step: 1,
            black_threshold: 0,
        };
        let mut amb = Ambilight::new(2, settings);
        let zones = vec![
            SampleZone::new(0.0, 0.0, 0.2, 1.0),
            SampleZone::new(0.8, 0.0, 0.2, 1.0),
        ];
        let out = amb.process(&f, &zones);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Rgb::new(200, 0, 0));
        assert_eq!(out[1], Rgb::new(0, 0, 200));
    }

    #[test]
    fn black_threshold_kills_dim_output() {
        let f = split_frame((3, 3, 3), (3, 3, 3));
        let settings = EngineSettings {
            smoothing: 1.0,
            brightness: 1.0,
            saturation: 0.0,
            sample_step: 1,
            black_threshold: 10,
        };
        let mut amb = Ambilight::new(1, settings);
        let out = amb.process(&f, &[SampleZone::new(0.0, 0.0, 1.0, 1.0)]);
        assert_eq!(out[0], Rgb::BLACK);
    }

    #[test]
    fn resize_grows_and_shrinks() {
        let mut amb = Ambilight::new(2, EngineSettings::default());
        amb.resize(5);
        let f = split_frame((10, 20, 30), (10, 20, 30));
        let zones = vec![SampleZone::new(0.0, 0.0, 1.0, 1.0); 5];
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
