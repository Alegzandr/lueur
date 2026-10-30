//! The ambilight engine: turn a captured frame into one colour per light.
//!
//! This module is deliberately pure and synchronous: given a [`Frame`] and the
//! set of [`SampleZone`]s, it returns the colours to send. All timing, capture
//! and network I/O live in the desktop shell. Keeping it pure means the
//! colour-critical path is fully deterministic and unit-tested.

use crate::bars::Bars;
use crate::color::{
    BulbOutput, ColorAccumulator, ColorSmoother, MAX_KELVIN, MIN_BRIGHT, MIN_KELVIN, Rgb, saturate,
    saturation,
};
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

    /// The brightest channel of the pixel at `x`, `y`.
    #[inline]
    pub(crate) fn value(&self, x: u32, y: u32) -> u8 {
        let (r, g, b) = self.pixel(x, y);
        r.max(g).max(b)
    }

    /// Average the colour inside a normalised zone, visiting roughly every
    /// `step`-th pixel in each axis (>=1). Larger `step` = cheaper, slightly
    /// noisier, a good latency/quality knob.
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
/// (~0.14); the goal is a soft handover, not a spotlight on the centre.
#[inline]
fn taper(d: f32) -> f32 {
    (-2.0 * d * d).exp()
}

/// Runtime settings for the engine. These map directly onto the UI's sliders.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EngineSettings {
    /// Weight of each new frame in the temporal smoother (0..1), for frames
    /// [`REFERENCE_FRAME`] apart; other frame rates get the same response
    /// over time (see [`Ambilight::set_frame_time`]). Higher = snappier /
    /// lower perceived latency; lower = smoother / cosier.
    pub smoothing: f32,
    /// Overall output brightness scale (0..1).
    pub brightness: f32,
    /// Saturation boost (0 = untouched, 1 = punchy neon).
    pub saturation: f32,
    /// Pixel step used when averaging a zone (>=1). Trades quality for CPU.
    pub sample_step: u32,
    /// Colours whose brightest channel is below this are pulled to black, so
    /// dark scenes don't leave a muddy glow. 0 disables.
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
    /// Leave black bars around the picture out of every zone (see
    /// [`crate::bars`]), so a film narrower or wider than the screen does
    /// not darken the lights at its edges.
    #[serde(default = "crate::default_true")]
    pub crop_bars: bool,
    /// How a colour's intensity becomes the bulb's brightness: the bulb
    /// follows the intensity raised to this power (1..=3). At 1 it follows
    /// the screen's encoded value, so a dark scene still lights the room
    /// well; at 2.2 it follows the light the screen actually gives, and a
    /// dark scene leaves the room as dark as the picture.
    #[serde(default = "default_brightness_curve")]
    pub brightness_curve: f32,
    /// Older versions' wall colour, now the room's
    /// ([`crate::geometry::Room::wall`]). Read once (see
    /// [`crate::config::AppConfig::from_json`]), never written.
    #[serde(default, rename = "wall", skip_serializing)]
    pub(crate) legacy_wall: Option<Rgb>,
}

fn default_white_threshold() -> f32 {
    0.18
}

fn default_white_kelvin() -> u16 {
    4000
}

fn default_brightness_curve() -> f32 {
    1.0
}

/// The steepest [`EngineSettings::brightness_curve`]: past the screen's own
/// curve, a dark scene would be darker in the room than on the screen.
pub const MAX_BRIGHTNESS_CURVE: f32 = 3.0;

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
            crop_bars: true,
            brightness_curve: default_brightness_curve(),
            legacy_wall: None,
        }
    }
}

/// How far saturation must climb back above `white_threshold` before we leave the
/// white path. Without this margin, a frame hovering at the threshold would
/// flip-flop between white and colour every frame.
const WHITE_HYSTERESIS: f32 = 1.6;

/// The least spread between a colour's channels (0..=255) that counts as a
/// hue, whatever its saturation. Near black, the saturation is a ratio of
/// tiny numbers: (10, 10, 12) is 17% saturated, yet it is a grey with one
/// unit of noise. Below this the light takes the white path; it leaves it
/// only past `CHROMA * WHITE_HYSTERESIS`.
const CHROMA: f32 = 8.0;

/// How much of the smoothing factor a light keeps while the picture is
/// still. Noise (compression, film grain, a blinking cursor) moves a zone's
/// average by a unit or two every frame; followed at full speed it becomes
/// a shimmer on the bulb and a command per frame.
const STILL_FACTOR: f32 = 0.35;

/// Changes (largest channel, 0..=255) at or below this count as noise,
/// and at or above `MOVE_DELTA` as real movement, followed at full speed.
const NOISE_DELTA: f32 = 1.5;
const MOVE_DELTA: f32 = 16.0;

/// How much of the user's smoothing factor applies when the zone moved from
/// `prev` to `next`: all of it for real movement, so cuts stay instant, and
/// a fraction for noise, so a still picture holds still.
fn steadiness(prev: Option<Rgb>, next: Rgb) -> f32 {
    let Some(prev) = prev else {
        return 1.0;
    };
    let d = f32::from(
        prev.r
            .abs_diff(next.r)
            .max(prev.g.abs_diff(next.g))
            .max(prev.b.abs_diff(next.b)),
    );
    let x = ((d - NOISE_DELTA) / (MOVE_DELTA - NOISE_DELTA)).clamp(0.0, 1.0);
    STILL_FACTOR + (1.0 - STILL_FACTOR) * x * x * (3.0 - 2.0 * x)
}

/// The frame interval, in seconds, that [`EngineSettings::smoothing`] is
/// tuned for: the default 45 fps.
pub const REFERENCE_FRAME: f32 = 1.0 / 45.0;

/// The weight that, applied every `dt` seconds, moves a light as fast as
/// `factor` applied every [`REFERENCE_FRAME`]. Without it the same setting
/// reads twice as sluggish at half the frame rate.
fn per_frame(factor: f32, dt: f32) -> f32 {
    let f = factor.clamp(0.0, 1.0);
    1.0 - (1.0 - f).powf(dt.max(0.0) / REFERENCE_FRAME)
}

/// How far a near-grey's leftover tint may move the white, in kelvin per
/// unit of warmth (red over blue, relative to the peak).
const TINT_KELVIN: f32 = 3500.0;
/// Warmth below this is noise and leaves the white alone.
const TINT_DEADBAND: f32 = 0.05;

/// The white for an achromatic colour: `kelvin`, nudged warmer or cooler by
/// whatever tint the grey still carries, so a sepia flashback glows warm and
/// a moonlit night cool rather than every grey becoming the same white.
/// Rounded to [`MIRED_STEP`] so a steady scene is not re-sent for invisible
/// changes.
fn tinted_kelvin(color: Rgb, kelvin: u16) -> u16 {
    let peak = color.value();
    if peak == 0 {
        return kelvin;
    }
    let warmth = (f32::from(color.r) - f32::from(color.b)) / f32::from(peak);
    let excess = (warmth.abs() - TINT_DEADBAND).max(0.0).copysign(warmth);
    if excess == 0.0 {
        return kelvin;
    }
    let k = (f32::from(kelvin) - excess * TINT_KELVIN).max(1.0);
    let mired = ((1e6 / k / MIRED_STEP).round() * MIRED_STEP).max(MIRED_STEP);
    (1e6 / mired).clamp(f32::from(MIN_KELVIN), f32::from(MAX_KELVIN)) as u16
}

/// The white's resolution, in mireds: about the smallest shift the eye can
/// tell. Even steps in kelvin would not do: 100 K is a jump at 2000 K and
/// barely anything at 6500 K.
const MIRED_STEP: f32 = 5.0;

/// Smallest change of a bulb's hue worth showing, as the largest channel
/// difference between two full-chroma colours (0..=255).
///
/// A shot that stays the same while something moves in it (a face turning, a
/// slow pan) moves a zone's average by a few units every frame. The bulb gets
/// that colour at full chroma, so the wobble reaches it as a hue that never
/// settles. Under this, the bulb keeps the hue it shows.
const HUE_HOLD: f32 = 10.0;

/// How many units a zone's average wobbles by on a shot that does not change.
/// Scaled to full chroma, those units weigh more the darker the colour: three
/// units are 1% of a bright red and a third of a red at 10. The hold widens
/// to match, so a dark scene does not change hue on noise alone.
const HUE_NOISE: f32 = 3.0;

/// How far the white must move, in mireds, before the bulb follows. Two
/// steps of [`MIRED_STEP`], so a tint sitting on a rounding boundary does not
/// flip the bulb between two whites.
const WHITE_HOLD: f32 = 2.0 * MIRED_STEP;

/// `next`, keeping the hue or white `prev` showed when the difference is
/// within noise (see [`HUE_HOLD`]). The brightness always follows. `peak` is
/// the brightest channel of the colour `next` was built from.
///
/// The hold is measured from what the bulb shows, not from the previous
/// frame, so a slow drift still arrives: in steps of the hold, which a bulb's
/// fade turns into a glide.
fn hold(prev: Option<BulbOutput>, next: BulbOutput, peak: u8) -> BulbOutput {
    match (prev, next) {
        (Some(BulbOutput::Color { rgb: was, .. }), BulbOutput::Color { rgb, bright }) => {
            let moved = was
                .r
                .abs_diff(rgb.r)
                .max(was.g.abs_diff(rgb.g))
                .max(was.b.abs_diff(rgb.b));
            let noise = HUE_NOISE * 255.0 / f32::from(peak.max(1));
            if f32::from(moved) < HUE_HOLD.max(noise) {
                BulbOutput::Color { rgb: was, bright }
            } else {
                next
            }
        }
        (Some(BulbOutput::White { kelvin: was, .. }), BulbOutput::White { kelvin, bright }) => {
            let mireds = |k: u16| 1e6 / f32::from(k.max(1));
            if (mireds(was) - mireds(kelvin)).abs() < WHITE_HOLD {
                BulbOutput::White {
                    kelvin: was,
                    bright,
                }
            } else {
                next
            }
        }
        _ => next,
    }
}

/// Holds per-light smoothing state across frames.
pub struct Ambilight {
    settings: EngineSettings,
    smoothers: Vec<ColorSmoother>,
    /// Whether each light is currently on the white path. Latched so the
    /// white/colour decision gets hysteresis instead of chattering.
    in_white: Vec<bool>,
    /// What each light was last given, so a black frame can dim it without
    /// changing its colour.
    shown: Vec<Option<BulbOutput>>,
    /// Each light's colour this frame as the screen gave it, smoothed but
    /// not yet saturated: what the white/colour decision is made on, since
    /// the saturation boost would turn a grey's noise into a hue.
    seen: Vec<Rgb>,
    /// The black bars around the picture, followed across frames.
    bars: Bars,
    /// Seconds since the previous frame.
    frame_time: f32,
}

impl Ambilight {
    pub fn new(light_count: usize, settings: EngineSettings) -> Self {
        Ambilight {
            settings,
            smoothers: vec![ColorSmoother::new(); light_count],
            in_white: vec![false; light_count],
            shown: vec![None; light_count],
            seen: vec![Rgb::BLACK; light_count],
            bars: Bars::new(),
            frame_time: REFERENCE_FRAME,
        }
    }

    /// Tell the engine how long it has been since the previous frame, so the
    /// smoothing keeps the same pace in time whatever the frame rate.
    pub fn set_frame_time(&mut self, seconds: f32) {
        if seconds.is_finite() && seconds > 0.0 {
            self.frame_time = seconds;
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
        self.shown.resize(light_count, None);
        self.seen.resize(light_count, Rgb::BLACK);
    }

    /// Compute the screen-referred colour for each light this frame.
    ///
    /// This is the colour *as seen on screen* (sampled, smoothed, saturated);
    /// brightness is deliberately not applied here, because bulbs take intensity
    /// on their own channel. Use [`Ambilight::process_outputs`] to drive hardware.
    ///
    /// `mappings` must be the same length as the configured light count; extra
    /// or missing entries are handled gracefully (clamped to the shorter
    /// length). Each mapping's `responsiveness` scales the smoothing factor, so
    /// a lamp across the room drifts rather than snaps; a still picture scales
    /// it down further (see [`steadiness`]), so noise never reaches the bulb.
    pub fn process(&mut self, frame: &Frame, mappings: &[LightMapping]) -> Vec<Rgb> {
        let n = mappings.len().min(self.smoothers.len());
        let picture = if self.settings.crop_bars {
            self.bars.see(frame, self.frame_time)
        } else {
            self.bars.reset();
            self.bars.picture()
        };
        let mut out = Vec::with_capacity(n);
        for (i, m) in mappings.iter().take(n).enumerate() {
            let zone = m.zone.within(picture);
            let raw = frame.average_zone(&zone, self.settings.sample_step);
            let steady = steadiness(self.smoothers[i].current(), raw);
            let factor = self.settings.smoothing * m.responsiveness * steady;
            // Floor the factor above 0: a zero would freeze the light forever.
            let smoothing = per_frame(factor, self.frame_time).clamp(0.01, 1.0);
            let smoothed = self.smoothers[i].push(raw, smoothing);
            self.seen[i] = smoothed;
            let mut shaped = saturate(smoothed, self.settings.saturation);
            if self.settings.black_threshold > 0 && shaped.value() < self.settings.black_threshold {
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
    /// A black frame dims the bulb to its floor but keeps what it showed: a
    /// fade to black and back is then a pure dimming, rather than a hop to
    /// white and back that a bulb renders as a visible blink.
    ///
    /// This is where a mapping's distance `gain` lands: it scales the bulb's
    /// intensity, never its hue, so a far lamp reads as a softer glow of the
    /// same colour rather than a washed-out one.
    ///
    /// A hue or white within noise of what the bulb shows is kept (see
    /// [`hold`]), so movement inside an unchanged shot leaves the bulbs alone.
    pub fn process_outputs(&mut self, frame: &Frame, mappings: &[LightMapping]) -> Vec<BulbOutput> {
        let colors = self.process(frame, mappings);
        let brightness = self.settings.brightness;
        let kelvin = self.settings.white_kelvin;
        let curve = self
            .settings
            .brightness_curve
            .clamp(1.0, MAX_BRIGHTNESS_CURVE);
        let enter = self.settings.white_threshold.clamp(0.0, 1.0);
        let leave = (enter * WHITE_HYSTERESIS).min(1.0);
        let leave_chroma = CHROMA * WHITE_HYSTERESIS;

        colors
            .into_iter()
            .enumerate()
            .map(|(i, color)| {
                if color == Rgb::BLACK {
                    let out = match self.shown[i] {
                        Some(BulbOutput::Color { rgb, .. }) => BulbOutput::Color {
                            rgb,
                            bright: MIN_BRIGHT,
                        },
                        Some(BulbOutput::White { kelvin, .. }) => BulbOutput::White {
                            kelvin,
                            bright: MIN_BRIGHT,
                        },
                        None => BulbOutput::White {
                            kelvin,
                            bright: MIN_BRIGHT,
                        },
                    };
                    self.shown[i] = Some(out);
                    return out;
                }
                let gain = mappings.get(i).map(|m| m.gain).unwrap_or(1.0);
                // The bulb's brightness is the intensity times `brightness`;
                // the curve's extra power of the intensity scales the latter.
                let intensity = f32::from(color.value()) / 255.0;
                let steepen = intensity.powf(curve - 1.0);
                let brightness = (brightness * gain * steepen).clamp(0.0, 1.0);
                // Decided on the colour before the saturation boost, by its
                // saturation and by the spread of its channels (see
                // [`CHROMA`]).
                let seen = self.seen[i];
                let sat = saturation(seen);
                let chroma = f32::from(seen.value() - seen.r.min(seen.g).min(seen.b));
                // A latched decision with two thresholds: drop into white below
                // `enter`, but only climb back out once clearly past `leave`.
                let white = if enter <= 0.0 {
                    false
                } else if self.in_white[i] {
                    sat <= leave || chroma <= leave_chroma
                } else {
                    sat < enter || chroma < CHROMA
                };
                self.in_white[i] = white;
                let built =
                    BulbOutput::build(color, brightness, white, tinted_kelvin(color, kelvin));
                let out = hold(self.shown[i], built, color.value());
                self.shown[i] = Some(out);
                out
            })
            .collect()
    }

    /// Reset all smoothing state (e.g. when (re)starting the effect).
    pub fn reset(&mut self) {
        for s in &mut self.smoothers {
            s.reset();
        }
        self.in_white.fill(false);
        self.shown.fill(None);
        self.bars.reset();
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
        assert_eq!(
            f.average_zone(&SampleZone::new(0.0, 0.0, 1.0, 1.0), 1),
            Rgb::new(255, 0, 0)
        );
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
            crop_bars: false,
            brightness_curve: 1.0,
            legacy_wall: None,
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
                assert!(bright < 20, "dark frame should dim the bulb, got {bright}");
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
    fn a_tinted_grey_gets_a_tinted_white() {
        assert_eq!(tinted_kelvin(Rgb::new(80, 80, 80), 4000), 4000);
        assert!(
            tinted_kelvin(Rgb::new(90, 80, 66), 4000) < 3500,
            "sepia is warm"
        );
        assert!(
            tinted_kelvin(Rgb::new(66, 74, 90), 4000) > 4500,
            "moonlight is cool"
        );
        // A one-unit wobble of the tint does not move the white.
        assert_eq!(
            tinted_kelvin(Rgb::new(90, 80, 66), 4000),
            tinted_kelvin(Rgb::new(90, 80, 65), 4000)
        );
        // An untinted grey keeps the user's white exactly.
        assert_eq!(tinted_kelvin(Rgb::new(80, 80, 80), 2700), 2700);
    }

    #[test]
    fn a_black_frame_dims_without_changing_colour() {
        let settings = EngineSettings {
            black_threshold: 10,
            white_threshold: 0.18,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let red = split_frame((200, 0, 0), (200, 0, 0));
        let black = split_frame((0, 0, 0), (0, 0, 0));
        amb.process_outputs(&red, &full());
        assert_eq!(
            amb.process_outputs(&black, &full())[0],
            BulbOutput::Color {
                rgb: Rgb::new(255, 0, 0),
                bright: MIN_BRIGHT
            }
        );
    }

    #[test]
    fn a_dark_blue_is_not_black() {
        // A night sky: barely any luminance, but plainly a colour.
        let settings = EngineSettings {
            black_threshold: 6,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let night = split_frame((0, 0, 40), (0, 0, 40));
        assert_eq!(amb.process(&night, &full())[0], Rgb::new(0, 0, 40));
    }

    #[test]
    fn a_still_picture_holds_still() {
        let settings = EngineSettings {
            smoothing: 0.5,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        let a = split_frame((100, 100, 100), (100, 100, 100));
        let b = split_frame((101, 101, 101), (101, 101, 101));
        amb.process(&a, &full());
        // One unit of noise, followed at half speed, would reach 101.
        amb.process(&b, &full());
        assert_eq!(amb.process(&b, &full())[0], Rgb::new(100, 100, 100));
        // A cut is followed at the full factor.
        let cut = split_frame((200, 100, 100), (200, 100, 100));
        assert_eq!(amb.process(&cut, &full())[0].r, 150);
    }

    /// The hue light 0 is given for a frame of one solid colour.
    fn hue_of(amb: &mut Ambilight, rgb: (u8, u8, u8)) -> Rgb {
        match amb.process_outputs(&split_frame(rgb, rgb), &full())[0] {
            BulbOutput::Color { rgb, .. } => rgb,
            other => panic!("expected Color, got {other:?}"),
        }
    }

    #[test]
    fn movement_inside_a_shot_keeps_the_hue() {
        let mut amb = Ambilight::new(1, passthrough());
        let shown = hue_of(&mut amb, (200, 100, 50));
        // A few units of wobble, as a face turning in the same shot gives.
        assert_eq!(hue_of(&mut amb, (200, 104, 50)), shown);
        assert_eq!(hue_of(&mut amb, (198, 97, 51)), shown);
        // A real change of colour goes through at once.
        assert_ne!(hue_of(&mut amb, (200, 130, 50)), shown);
    }

    #[test]
    fn a_dark_scene_holds_its_hue_through_more_noise() {
        // One unit at a peak of 20 is twelve at full chroma: past the hold
        // for a bright colour, noise for a dark one.
        let mut dark = Ambilight::new(1, passthrough());
        let shown = hue_of(&mut dark, (20, 10, 5));
        assert_eq!(hue_of(&mut dark, (20, 11, 5)), shown);

        let mut bright = Ambilight::new(1, passthrough());
        let shown = hue_of(&mut bright, (200, 100, 50));
        assert_ne!(hue_of(&mut bright, (200, 110, 50)), shown);
    }

    #[test]
    fn a_slow_drift_of_hue_still_arrives() {
        let mut amb = Ambilight::new(1, passthrough());
        let mut last = hue_of(&mut amb, (200, 100, 50));
        for g in 101..=160 {
            last = hue_of(&mut amb, (200, g, 50));
        }
        // Where the drift ended, within one hold.
        let target = crate::color::full_chroma(Rgb::new(200, 160, 50));
        assert!(last.g.abs_diff(target.g) < HUE_HOLD as u8, "{last:?}");
    }

    #[test]
    fn the_brightness_follows_while_the_hue_holds() {
        let mut amb = Ambilight::new(1, passthrough());
        let f = |rgb| split_frame(rgb, rgb);
        let a = amb.process_outputs(&f((200, 100, 50)), &full())[0];
        let b = amb.process_outputs(&f((100, 52, 25)), &full())[0];
        match (a, b) {
            (
                BulbOutput::Color {
                    rgb: ra,
                    bright: ba,
                },
                BulbOutput::Color {
                    rgb: rb,
                    bright: bb,
                },
            ) => {
                assert_eq!(ra, rb);
                assert!(bb < ba, "{ba} -> {bb}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_white_on_a_rounding_boundary_holds() {
        let white = |kelvin| BulbOutput::White { kelvin, bright: 50 };
        // One mired step: the tint wobbled across a rounding boundary.
        assert_eq!(hold(Some(white(4000)), white(3922), 200), white(4000));
        // A clearly warmer white goes through.
        assert_eq!(hold(Some(white(4000)), white(3500), 200), white(3500));
    }

    #[test]
    fn smoothing_keeps_its_pace_across_frame_rates() {
        assert_eq!(per_frame(0.5, REFERENCE_FRAME), 0.5);
        assert_eq!(per_frame(1.0, REFERENCE_FRAME / 3.0), 1.0);
        // Two frames at double the rate land where one reference frame does.
        let half = per_frame(0.5, REFERENCE_FRAME / 2.0);
        assert!(((1.0 - half) * (1.0 - half) - 0.5).abs() < 1e-5, "{half}");

        let settings = EngineSettings {
            smoothing: 0.5,
            ..passthrough()
        };
        let black = split_frame((0, 0, 0), (0, 0, 0));
        let red = split_frame((200, 0, 0), (200, 0, 0));
        let mut slow = Ambilight::new(1, settings);
        let mut fast = Ambilight::new(1, settings);
        fast.set_frame_time(REFERENCE_FRAME / 2.0);
        slow.process(&black, &full());
        fast.process(&black, &full());
        let a = slow.process(&red, &full())[0].r;
        fast.process(&red, &full());
        let b = fast.process(&red, &full())[0].r;
        assert!(a.abs_diff(b) <= 1, "{a} vs {b}");
    }

    #[test]
    fn a_near_black_grey_stays_white_through_its_noise() {
        // The default saturation boost, as in use: it would make a lavender
        // of the first, a white of the second.
        let settings = EngineSettings {
            saturation: 0.35,
            white_threshold: 0.18,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        for rgb in [(10, 10, 12), (10, 10, 11), (9, 10, 13), (11, 10, 12)] {
            let out = amb.process_outputs(&split_frame(rgb, rgb), &full())[0];
            assert!(matches!(out, BulbOutput::White { .. }), "{rgb:?}: {out:?}");
        }
        // A dark but plain colour is still one.
        let night = (4, 8, 40);
        let out = amb.process_outputs(&split_frame(night, night), &full())[0];
        assert!(matches!(out, BulbOutput::Color { .. }), "{out:?}");
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
    fn a_light_at_the_top_follows_the_film_not_its_band() {
        // A 2.39:1 film on a 16:9 screen: black bands above and below a red
        // picture.
        let (w, h) = (160u32, 90u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 12..78 {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[200, 0, 0, 255]);
            }
        }
        let film = Frame::new(w, h, PixelFormat::Rgba, data);
        let top = [z(0.0, 0.0, 1.0, 0.12)];
        let settings = EngineSettings {
            crop_bars: true,
            ..passthrough()
        };
        let mut amb = Ambilight::new(1, settings);
        assert_eq!(amb.process(&film, &top)[0], Rgb::BLACK);
        for _ in 0..(3.0 / REFERENCE_FRAME) as u32 {
            amb.process(&film, &top);
        }
        assert_eq!(amb.process(&film, &top)[0], Rgb::new(200, 0, 0));

        // Turned off, the band is read again.
        amb.set_settings(passthrough());
        assert_eq!(amb.process(&film, &top)[0], Rgb::BLACK);
    }

    #[test]
    fn the_brightness_curve_darkens_dark_scenes_only() {
        let bright_of = |curve: f32, v: u8| {
            let settings = EngineSettings {
                brightness_curve: curve,
                ..passthrough()
            };
            let f = split_frame((v, v / 2, 0), (v, v / 2, 0));
            Ambilight::new(1, settings).process_outputs(&f, &full())[0].bright()
        };
        // A full colour drives the bulb fully on any curve.
        assert_eq!(bright_of(2.2, 255), 100);
        // Half the encoded value is half the bulb at 1, a fifth of it at 2.2:
        // about the light the screen gives there.
        assert_eq!(bright_of(1.0, 128), 50);
        assert!(
            (20..=23).contains(&bright_of(2.2, 128)),
            "{}",
            bright_of(2.2, 128)
        );
        // Never below the bulb's floor, never steeper than the cap.
        assert_eq!(bright_of(2.2, 10), MIN_BRIGHT);
        assert_eq!(bright_of(9.0, 128), bright_of(MAX_BRIGHTNESS_CURVE, 128));
    }

    #[test]
    fn empty_frame_is_safe() {
        let f = Frame::new(0, 0, PixelFormat::Rgba, vec![]);
        assert_eq!(
            f.average_zone(&SampleZone::new(0.0, 0.0, 1.0, 1.0), 2),
            Rgb::BLACK
        );
    }
}
