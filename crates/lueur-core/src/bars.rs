//! Black bars around the picture.
//!
//! A film wider than the screen leaves black bands above and below it; a 16:9
//! video on an ultrawide screen leaves them at its sides. Read as part of the
//! picture, they pull every light near that edge towards black for the whole
//! film. [`Bars`] finds them and gives the rectangle the picture really
//! fills, which the engine samples instead of the whole frame.
//!
//! One frame cannot tell a bar from a dark scene, so a change is believed
//! only once the frames have agreed on it for a while. And a band counts as a
//! bar only while it is dark on both opposite sides: subtitles or a player's
//! controls light the lower band alone, and do not make the bars go away.

use crate::engine::Frame;
use crate::geometry::SampleZone;

/// A pixel whose brightest channel is at or below this is black. Players draw
/// their bars at true black; the margin covers compression noise. A dark
/// theme's title bar or taskbar is lighter than this.
const DARK: u8 = 12;
/// Bars thinner than this share of the frame are ignored: plenty of pictures
/// have a thin dark edge of their own.
const LEAST: f32 = 0.03;
/// The widest a bar may be, as a share of the frame on each side. A 4:3
/// picture on a 32:9 screen leaves 31%; anything darker than this is a dark
/// scene, which says nothing about the bars.
const MOST: f32 = 0.34;
/// Two measures closer than this share of the frame are the same bars.
const SAME: f32 = 0.015;
/// Seconds the frames must agree before bars are taken to appear or widen. A
/// dark scene rarely keeps both edges black that long.
const APPEAR: f32 = 2.0;
/// Seconds the frames must agree before bars are taken to narrow or go.
/// Picture on both sides where a bar was is never a dark scene, so this can
/// be quick: leaving a film for the desktop is not followed late.
const VANISH: f32 = 0.5;
/// How many samples a row or column is read at, whatever the frame's size.
const SAMPLES: u32 = 256;

/// The bars on one axis: above and below the picture, or beside it.
#[derive(Debug, Clone, Copy, Default)]
struct Axis {
    /// The width of each bar now, as a share of the frame.
    bar: f32,
    /// A different width the frames have shown lately, and for how long.
    candidate: Option<(f32, f32)>,
}

impl Axis {
    /// Take one frame's dark margins on both sides, `dt` seconds after the
    /// last frame. `None` on a side means it was dark beyond [`MOST`].
    fn see(&mut self, a: Option<f32>, b: Option<f32>, dt: f32) {
        // Too dark on a side to tell: no evidence either way.
        let (Some(a), Some(b)) = (a, b) else {
            return;
        };
        let (lo, hi) = (a.min(b), a.max(b));
        // Both sides darker than the bars: they may have widened. Either
        // side lit inside them: subtitles, or a dark scene's light edge,
        // until the other side is lit as well.
        let proposed = if lo > self.bar + SAME {
            lo
        } else if hi < self.bar - SAME {
            hi
        } else {
            self.candidate = None;
            return;
        };
        let proposed = if proposed < LEAST { 0.0 } else { proposed };
        let (value, held) = match self.candidate {
            // The narrower of two close measures, so the picture is never cut.
            Some((v, held)) if (v - proposed).abs() <= SAME => (v.min(proposed), held + dt),
            _ => (proposed, 0.0),
        };
        let needed = if value > self.bar { APPEAR } else { VANISH };
        if held >= needed {
            self.bar = value;
            self.candidate = None;
        } else {
            self.candidate = Some((value, held));
        }
    }
}

/// Follows the black bars of the frames it is shown.
#[derive(Debug, Clone, Copy, Default)]
pub struct Bars {
    /// Above and below the picture.
    rows: Axis,
    /// Left and right of it.
    columns: Axis,
}

impl Bars {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look at a frame, `dt` seconds after the last, and return the part of
    /// the frame the picture fills.
    pub fn see(&mut self, frame: &Frame, dt: f32) -> SampleZone {
        if let Some([top, bottom, left, right]) = margins(frame) {
            self.rows.see(top, bottom, dt);
            self.columns.see(left, right, dt);
        }
        self.picture()
    }

    /// The part of the frame the picture fills, bars left out.
    pub fn picture(&self) -> SampleZone {
        let (h, v) = (self.columns.bar, self.rows.bar);
        SampleZone::new(h, v, 1.0 - 2.0 * h, 1.0 - 2.0 * v)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The dark margin on each side of `frame` (top, bottom, left, right), as a
/// share of its height or width, `None` where it runs past [`MOST`]. `None`
/// for a frame with nothing lit at all.
fn margins(frame: &Frame) -> Option<[Option<f32>; 4]> {
    let (w, h) = (frame.width, frame.height);
    if w == 0 || h == 0 {
        return None;
    }
    let x_step = (w / SAMPLES).max(1);
    let y_step = (h / SAMPLES).max(1);
    let row_dark = |y: u32| {
        (0..w)
            .step_by(x_step as usize)
            .all(|x| frame.value(x, y) <= DARK)
    };
    let column_dark = |x: u32| {
        (0..h)
            .step_by(y_step as usize)
            .all(|y| frame.value(x, y) <= DARK)
    };
    // A frame dark through its middle row and column is a dark scene, or a
    // black screen: nothing to measure.
    if row_dark(h / 2) && column_dark(w / 2) {
        return None;
    }
    let run = |size: u32, step: u32, dark: &dyn Fn(u32) -> bool, from_end: bool| {
        let limit = (MOST * size as f32).ceil() as u32;
        let mut n = 0;
        while n < limit {
            let at = if from_end { size - 1 - n } else { n };
            if !dark(at) {
                return Some(n as f32 / size as f32);
            }
            n += step;
        }
        None
    };
    Some([
        run(h, y_step, &row_dark, false),
        run(h, y_step, &row_dark, true),
        run(w, x_step, &column_dark, false),
        run(w, x_step, &column_dark, true),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PixelFormat;

    /// A 200x100 frame of `fill`, black outside the picture rectangle given
    /// in pixels.
    fn framed(x0: u32, y0: u32, x1: u32, y1: u32, fill: u8) -> Frame {
        let (w, h) = (200u32, 100u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[fill, fill / 2, fill / 3, 255]);
            }
        }
        Frame::new(w, h, PixelFormat::Rgba, data)
    }

    /// Show `frame` for `secs` seconds at 45 frames a second.
    fn watch(bars: &mut Bars, frame: &Frame, secs: f32) -> SampleZone {
        let dt = 1.0 / 45.0;
        let mut picture = bars.picture();
        for _ in 0..(secs / dt).round() as u32 {
            picture = bars.see(frame, dt);
        }
        picture
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.011
    }

    #[test]
    fn a_full_picture_has_no_bars() {
        let mut bars = Bars::new();
        let p = watch(&mut bars, &framed(0, 0, 200, 100, 120), 5.0);
        assert_eq!(p, SampleZone::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn a_wide_film_loses_its_bands_once_they_have_held() {
        let mut bars = Bars::new();
        let film = framed(0, 13, 200, 87, 120);
        // One second is not enough to tell bars from a dark scene.
        assert_eq!(watch(&mut bars, &film, 1.0).y, 0.0);
        let p = watch(&mut bars, &film, 1.5);
        assert!(near(p.y, 0.13) && near(p.h, 0.74), "{p:?}");
        assert_eq!((p.x, p.w), (0.0, 1.0));
    }

    #[test]
    fn a_narrow_video_on_a_wide_screen_loses_its_sides() {
        let mut bars = Bars::new();
        let p = watch(&mut bars, &framed(25, 0, 175, 100, 120), 3.0);
        assert!(near(p.x, 0.125) && near(p.w, 0.75), "{p:?}");
        assert_eq!((p.y, p.h), (0.0, 1.0));
    }

    #[test]
    fn bars_on_both_axes_at_once_are_both_left_out() {
        // A 4:3 picture inside a 21:9 film's bands: black on all four sides.
        let mut bars = Bars::new();
        let p = watch(&mut bars, &framed(40, 13, 160, 87, 120), 3.0);
        assert!(near(p.x, 0.2) && near(p.w, 0.6), "{p:?}");
        assert!(near(p.y, 0.13) && near(p.h, 0.74), "{p:?}");
    }

    #[test]
    fn a_4_3_picture_on_the_widest_screens_is_still_found() {
        // On a 32:9 screen, 4:3 leaves 31% on each side.
        let mut bars = Bars::new();
        let p = watch(&mut bars, &framed(62, 0, 138, 100, 120), 3.0);
        assert!(near(p.x, 0.31) && near(p.w, 0.38), "{p:?}");
    }

    #[test]
    fn subtitles_in_the_lower_band_keep_the_bars() {
        let mut bars = Bars::new();
        watch(&mut bars, &framed(0, 13, 200, 87, 120), 3.0);
        // A line of white text in the lower band, for several seconds.
        let mut subtitled = framed(0, 13, 200, 87, 120);
        for y in 90..94 {
            for x in 60..140 {
                let i = ((y * 200 + x) * 4) as usize;
                subtitled.data[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let p = watch(&mut bars, &subtitled, 6.0);
        assert!(near(p.y, 0.13), "{p:?}");
    }

    #[test]
    fn a_dark_scene_keeps_the_bars_it_had() {
        let mut bars = Bars::new();
        watch(&mut bars, &framed(0, 13, 200, 87, 120), 3.0);
        // Black but for a small light in the middle.
        let p = watch(&mut bars, &framed(90, 45, 110, 55, 200), 5.0);
        assert!(near(p.y, 0.13), "{p:?}");
        let p = watch(&mut bars, &framed(0, 0, 0, 0, 0), 5.0);
        assert!(near(p.y, 0.13), "a black screen says nothing: {p:?}");
    }

    #[test]
    fn the_bars_go_soon_after_the_film_ends() {
        let mut bars = Bars::new();
        watch(&mut bars, &framed(0, 13, 200, 87, 120), 3.0);
        let p = watch(&mut bars, &framed(0, 0, 200, 100, 120), 0.6);
        assert_eq!(p, SampleZone::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn a_thin_dark_edge_is_not_a_bar() {
        let mut bars = Bars::new();
        let p = watch(&mut bars, &framed(0, 2, 200, 98, 120), 5.0);
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn a_dark_theme_s_grey_is_not_black() {
        let mut bars = Bars::new();
        let mut desktop = framed(0, 0, 200, 100, 120);
        for y in (0..12).chain(88..100) {
            for x in 0..200 {
                let i = ((y * 200 + x) * 4) as usize;
                desktop.data[i..i + 3].copy_from_slice(&[32, 32, 32]);
            }
        }
        assert_eq!(watch(&mut bars, &desktop, 5.0).y, 0.0);
    }
}
