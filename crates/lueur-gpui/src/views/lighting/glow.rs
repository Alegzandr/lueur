//! The glow behind Lighting, worked out as the WebGL shader of the Tauri
//! versions worked it out, pool for pool and pixel for pixel: each pool falls
//! off as a Gaussian bell that reaches zero at its edge, the pools add up as
//! light does, the sum is made more saturated, as coloured light on a wall
//! looks, and laid over the page with the theme's strength.
//!
//! It is drawn into a small image, a sixth of the page across, which the GPU
//! stretches over the page with bilinear filtering: a bell this wide changes
//! too slowly for the stretch to show, and working the pixels out costs a
//! fraction of a millisecond, even at 240 frames a second. A bell is the
//! product of a bell across and a bell down, so each pool only costs one
//! multiplication per pixel.
//!
//! A faint glow spans few of the 256 levels a screen shows, and rounded to
//! them it falls into bands. The shader dithered each pixel; here a fixed
//! noise, as fine as the screen's pixels and too faint to be seen as such,
//! is laid over the glow and scatters the steps so the eye reads the slope.

/// One pool as the frame draws it.
#[derive(Debug, Clone, Copy)]
pub struct Pool {
    /// Its centre, 0..1 across and down the page.
    pub x: f32,
    pub y: f32,
    /// Its colour, 0..255.
    pub rgb: [f32; 3],
    /// How bright its light is, 0..1.
    pub level: f32,
}

/// Where a pool's light reaches, as a square around it in page pixels
/// (left, top, side): the dither is laid there and nowhere else.
pub fn reach_square(p: &Pool, (w, h): (f32, f32)) -> (f32, f32, f32) {
    let reach = w.max(h) * (0.32 + 0.12 * p.level);
    (p.x * w - reach, p.y * h - reach, 2.0 * reach)
}

/// The pixels of the page's side the glow image has per pixel.
pub const SHRINK: f32 = 6.0;

/// Where the bell meets zero: exp(-4.5), as the shader cut it.
const EDGE: f32 = 0.011_109;

/// How far, in page pixels, the glow fades out toward the page's left
/// edge. The sidebar stands there, and light cut straight at its border
/// reads as a shadow the sidebar casts; faded, the page's ground meets the
/// sidebar as it does with the lights off.
const LEFT_FADE: f32 = 140.0;

/// How large the glow image is for a page of `w` × `h` pixels.
pub fn image_size(w: f32, h: f32) -> (u32, u32) {
    let side = |v: f32| ((v / SHRINK).ceil() as u32).clamp(2, 1024);
    (side(w), side(h))
}

/// The glow for a page of `w` × `h` pixels, as `tw` × `th` BGRA pixels with
/// straight alpha: drawn over the page, it gives what the shader drew.
/// `strength` is the theme's (and the glow's arrival), `saturation` how
/// much purer the light shows than it is sent.
pub fn render(
    pools: &[Pool],
    (w, h): (f32, f32),
    (tw, th): (u32, u32),
    strength: f32,
    saturation: f32,
) -> Vec<u8> {
    let (tw, th) = (tw as usize, th as usize);
    // The premultiplied sum of the pools, as the shader's `sum`.
    let mut sum = vec![[0.0f32; 4]; tw * th];
    let (sx, sy) = (w / tw as f32, h / th as f32);
    let mut across = vec![0.0f32; tw];
    let mut down = vec![0.0f32; th];
    for p in pools {
        if p.level <= 0.0 {
            continue;
        }
        // The pool's reach; the eye reads a dim bulb as brighter than its
        // share of full power.
        let reach = w.max(h) * (0.32 + 0.12 * p.level);
        let weight = 0.75 * p.level.powf(0.6);
        let (cx, cy) = (p.x * w, p.y * h);
        let bell = |d: f32| (-4.5 * (d / reach) * (d / reach)).exp();
        for (i, v) in across.iter_mut().enumerate() {
            *v = bell((i as f32 + 0.5) * sx - cx);
        }
        for (j, v) in down.iter_mut().enumerate() {
            *v = bell((j as f32 + 0.5) * sy - cy);
        }
        let colour = p.rgb.map(|c| c / 255.0);
        for (j, &dy) in down.iter().enumerate() {
            if dy <= EDGE {
                continue;
            }
            let row = &mut sum[j * tw..(j + 1) * tw];
            for (cell, &dx) in row.iter_mut().zip(&across) {
                let b = dx * dy;
                if b <= EDGE {
                    continue;
                }
                let a = weight * (b - EDGE) / (1.0 - EDGE);
                cell[0] += colour[0] * a;
                cell[1] += colour[1] * a;
                cell[2] += colour[2] * a;
                cell[3] += a;
            }
        }
    }
    let fade: Vec<f32> = (0..tw)
        .map(|i| {
            let t = ((i as f32 + 0.5) * sx / LEFT_FADE).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        })
        .collect();
    let mut out = Vec::with_capacity(tw * th * 4);
    for (k, s) in sum.iter().enumerate() {
        let a = s[3].min(1.0);
        if a <= 0.0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let rgb = [s[0].min(1.0), s[1].min(1.0), s[2].min(1.0)];
        let grey = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        // Saturated, then kept within what the pools' light can give.
        let lit = rgb.map(|c| (grey + (c - grey) * saturation).clamp(0.0, a));
        // From premultiplied to straight: the GPU lays it over the page by
        // its alpha, which gives the shader's `sum + background * (1 - a)`.
        let straight = lit.map(|c| ((c / a) * 255.0).round().clamp(0.0, 255.0) as u8);
        let alpha = ((a * strength * fade[k % tw]).clamp(0.0, 1.0) * 255.0).round() as u8;
        out.extend_from_slice(&[straight[2], straight[1], straight[0], alpha]);
    }
    out
}

/// The side of the dither tile, in screen pixels.
pub const NOISE_SIDE: u32 = 128;

/// The dither: half its pixels white, half black, at random, each a hair
/// opaque. Laid over the glow, it moves each screen pixel a level or so up or
/// down, which breaks the bands into a grain too fine to see.
pub fn noise() -> Vec<u8> {
    // A small fixed generator: the same grain every launch, no dependency.
    let mut state: u32 = 0x9e37_79b9;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let n = (NOISE_SIDE * NOISE_SIDE) as usize;
    let mut out = Vec::with_capacity(n * 4);
    for _ in 0..n {
        let r = next();
        let white = r & 1 == 1;
        // One or two levels of 255, as the shader's own dither: enough to
        // scatter a step, too little to see.
        let alpha = 1 + ((r >> 8) % 2) as u8;
        let v = if white { 255 } else { 0 };
        out.extend_from_slice(&[v, v, v, alpha]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], tw: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * tw + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn a_pool_is_brightest_at_its_centre_and_gone_past_its_reach() {
        let pool = Pool {
            x: 0.25,
            y: 0.5,
            rgb: [255.0, 160.0, 60.0],
            level: 1.0,
        };
        let (w, h) = (1200.0, 800.0);
        let size = image_size(w, h);
        let img = render(&[pool], (w, h), size, 1.0, 1.0);
        let centre = pixel(&img, size.0, size.0 / 4, size.1 / 2);
        let near = pixel(&img, size.0, size.0 / 4 + 20, size.1 / 2);
        let far = pixel(&img, size.0, size.0 - 1, size.1 / 2);
        assert!(centre[3] > near[3] && near[3] > 0, "{centre:?} {near:?}");
        assert_eq!(far[3], 0, "past the reach the page shows plain");
        // BGRA, the colour itself where the pool is.
        assert!(centre[2] > centre[1] && centre[1] > centre[0]);
    }

    #[test]
    fn two_pools_add_up_as_light_does() {
        let at = |x| Pool {
            x,
            y: 0.5,
            rgb: [255.0, 255.0, 255.0],
            level: 0.3,
        };
        let (w, h) = (1200.0, 800.0);
        let size = image_size(w, h);
        let one = render(&[at(0.45)], (w, h), size, 1.0, 1.0);
        let two = render(&[at(0.45), at(0.55)], (w, h), size, 1.0, 1.0);
        let mid = |img: &[u8]| pixel(img, size.0, size.0 / 2, size.1 / 2)[3];
        assert!(mid(&two) > mid(&one));
    }

    #[test]
    fn a_pool_by_the_sidebar_fades_out_before_it() {
        let pool = Pool {
            x: 0.06,
            y: 0.5,
            rgb: [255.0, 255.0, 255.0],
            level: 1.0,
        };
        let (w, h) = (960.0, 800.0);
        let size = image_size(w, h);
        let img = render(&[pool], (w, h), size, 1.0, 1.0);
        let edge = pixel(&img, size.0, 0, size.1 / 2)[3];
        let inside = pixel(&img, size.0, (LEFT_FADE / SHRINK) as u32 + 2, size.1 / 2)[3];
        assert!(edge < 5, "the glow meets the sidebar at nothing: {edge}");
        assert!(inside > 100, "past the fade the pool shows whole: {inside}");
    }

    #[test]
    fn the_dither_is_faint_and_both_ways() {
        let n = noise();
        let alphas: Vec<u8> = n.chunks(4).map(|p| p[3]).collect();
        assert!(alphas.iter().all(|&a| (1..=2).contains(&a)));
        let white = n.chunks(4).filter(|p| p[0] == 255).count();
        let all = n.len() / 4;
        assert!(white > all * 4 / 10 && white < all * 6 / 10);
    }
}
