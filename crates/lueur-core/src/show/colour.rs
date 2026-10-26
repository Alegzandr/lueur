//! Colour helpers the shows share: a bulb output at a level, an even
//! brightness round the wheel, and blends that go round it.

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT, Rgb, full_chroma};

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
pub(super) fn hsv(c: Rgb) -> (f32, f32, f32) {
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
pub(super) fn clean_blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
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
pub(super) fn palette_at(colors: &[Rgb], pos: f32) -> Rgb {
    let len = colors.len();
    let pos = pos.rem_euclid(len as f32);
    let i = (pos.floor() as usize).min(len - 1);
    blend(colors[i], colors[(i + 1) % len], pos - pos.floor())
}

/// The colour at `x` (0..=1) along a palette that does not loop.
pub(super) fn ramp(colors: &[Rgb], x: f32) -> Rgb {
    let pos = x.clamp(0.0, 1.0) * (colors.len() - 1) as f32;
    let i = (pos.floor() as usize).min(colors.len() - 2);
    blend(colors[i], colors[i + 1], pos - i as f32)
}
