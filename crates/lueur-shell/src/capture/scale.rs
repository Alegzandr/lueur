//! Shrinking a screen image to the few hundred pixels the engine reads.
//!
//! The ambilight only needs a handful of zone averages, so every capture path
//! hands the loop a frame about as small as Desktop Duplication's readback
//! (see `dxgi.rs`): a few hundred pixels wide, a few hundred kilobytes. GDI
//! shrinks while it copies (see `gdi.rs`); a PipeWire stream arrives at full
//! resolution and is shrunk here, on the CPU.

use lueur_core::engine::{Frame, PixelFormat};
use std::cell::RefCell;

/// A shrunk frame is the smallest power-of-two reduction still at least this
/// wide, the same floor as the DXGI readback.
pub const MIN_WIDTH: u32 = 256;

/// At most this many source pixels are read along each axis of the block one
/// output pixel covers. A 4K desktop shrunk eightfold would otherwise read
/// every one of its 8×8 pixels; 4×4 evenly spread ones average the same.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const MAX_TAPS: usize = 4;

/// The size a `width`×`height` screen is shrunk to: halved until another
/// halving would drop below [`MIN_WIDTH`].
pub fn target_size(width: u32, height: u32) -> (u32, u32) {
    let mut level = 0;
    while (width >> (level + 1)) >= MIN_WIDTH {
        level += 1;
    }
    ((width >> level).max(1), (height >> level).max(1))
}

/// Shrink a 4-bytes-per-pixel image into `out` with a box filter.
///
/// `src` holds `height` rows of `stride` bytes, each starting with `width`
/// pixels in `format` order. Channels are averaged as squares, which is close
/// to averaging in linear light: a red and a blue half make a vivid magenta
/// rather than a muddy one, as with the engine's own accumulation and the
/// GPU's sRGB mips. The alpha byte is set opaque.
// Only the PipeWire stream is shrunk on the CPU; GDI shrinks as it copies.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn shrink_into(
    src: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    format: PixelFormat,
    out: &mut Frame,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("empty screen image".into());
    }
    let row = width as usize * 4;
    let needed = stride * (height as usize - 1) + row;
    if stride < row || src.len() < needed {
        return Err(format!(
            "screen image too small: {} bytes for {width}x{height} (stride {stride})",
            src.len()
        ));
    }
    let (out_w, out_h) = target_size(width, height);

    out.width = out_w;
    out.height = out_h;
    out.format = format;
    out.data.clear();
    out.data.reserve(out_w as usize * out_h as usize * 4);
    TAPS.with_borrow_mut(|cache| {
        let taps = match cache {
            Some(t) if t.size == (width, height) => t,
            _ => cache.insert(Taps {
                size: (width, height),
                columns: taps(width as usize, out_w as usize),
                rows: taps(height as usize, out_h as usize),
            }),
        };
        shrink_with(src, stride, &taps.columns, &taps.rows, out);
    });
    Ok(())
}

/// Which source pixels each output pixel averages, for one screen size.
struct Taps {
    size: (u32, u32),
    columns: Vec<Vec<usize>>,
    rows: Vec<Vec<usize>>,
}

thread_local! {
    /// The taps of the last screen size shrunk on this thread: the screen
    /// keeps its size, so a frame costs no allocation for them.
    static TAPS: RefCell<Option<Taps>> = const { RefCell::new(None) };
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn shrink_with(
    src: &[u8],
    stride: usize,
    columns: &[Vec<usize>],
    rows: &[Vec<usize>],
    out: &mut Frame,
) {
    for ys in rows {
        for xs in columns {
            let mut sum = [0u32; 3];
            for &y in ys {
                let line = &src[y * stride..];
                for &x in xs {
                    let px = &line[x * 4..x * 4 + 3];
                    for (s, &v) in sum.iter_mut().zip(px) {
                        *s += u32::from(v) * u32::from(v);
                    }
                }
            }
            let n = (xs.len() * ys.len()) as f32;
            for s in sum {
                out.data
                    .push(((s as f32 / n).sqrt() + 0.5).min(255.0) as u8);
            }
            out.data.push(255);
        }
    }
}

/// For each of `to` output pixels along an axis of `from` source pixels, the
/// source indices it averages: at most [`MAX_TAPS`], spread over its block.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn taps(from: usize, to: usize) -> Vec<Vec<usize>> {
    (0..to)
        .map(|i| {
            let start = i * from / to;
            let end = ((i + 1) * from / to).max(start + 1).min(from);
            let len = end - start;
            let count = len.min(MAX_TAPS);
            // Centred in `count` equal slices of the block.
            (0..count)
                .map(|k| start + (2 * k + 1) * len / (2 * count))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::geometry::SampleZone;

    fn solid(width: u32, height: u32, px: [u8; 4]) -> Vec<u8> {
        px.repeat((width * height) as usize)
    }

    fn empty() -> Frame {
        Frame::new(0, 0, PixelFormat::Rgba, Vec::new())
    }

    #[test]
    fn sizes_match_the_dxgi_readback() {
        assert_eq!(target_size(1920, 1080), (480, 270));
        assert_eq!(target_size(2560, 1440), (320, 180));
        assert_eq!(target_size(3840, 2160), (480, 270));
        assert_eq!(target_size(5120, 1440), (320, 90));
        // Never shrinks a small display below the floor.
        assert_eq!(target_size(300, 200), (300, 200));
        assert_eq!(target_size(1, 1), (1, 1));
    }

    #[test]
    fn a_solid_screen_stays_that_colour() {
        let src = solid(1920, 1080, [10, 200, 30, 0]);
        let mut out = empty();
        shrink_into(&src, 1920, 1080, 1920 * 4, PixelFormat::Bgra, &mut out).unwrap();
        assert_eq!((out.width, out.height), (480, 270));
        assert_eq!(out.data.len(), 480 * 270 * 4);
        assert_eq!(out.format, PixelFormat::Bgra);
        assert!(out.data.chunks(4).all(|px| px == [10, 200, 30, 255]));
    }

    #[test]
    fn halves_keep_their_side() {
        // Left half red, right half blue, 1024 wide: shrunk to 256.
        let (w, h) = (1024u32, 64u32);
        let mut src = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                src.extend_from_slice(if x < w / 2 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        let mut out = empty();
        shrink_into(&src, w, h, w as usize * 4, PixelFormat::Rgba, &mut out).unwrap();
        assert_eq!(out.width, 256);
        let left = out.average_zone(&SampleZone::new(0.0, 0.0, 0.4, 1.0), 1);
        let right = out.average_zone(&SampleZone::new(0.6, 0.0, 0.4, 1.0), 1);
        assert_eq!((left.r, left.g, left.b), (255, 0, 0));
        assert_eq!((right.r, right.g, right.b), (0, 0, 255));
    }

    #[test]
    fn mixes_in_linear_light() {
        // Alternate black and white columns: a plain mean would give 128, the
        // light-weighted mean is brighter.
        let (w, h) = (512u32, 2u32);
        let mut src = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if x % 2 == 0 { 0 } else { 255 };
                src.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let mut out = empty();
        shrink_into(&src, w, h, w as usize * 4, PixelFormat::Rgba, &mut out).unwrap();
        assert_eq!(out.width, 256);
        assert!(
            out.data.chunks(4).all(|px| px[0] == 180),
            "{:?}",
            &out.data[..8]
        );
    }

    #[test]
    fn row_padding_is_skipped() {
        // Each row carries 16 bytes of garbage past its pixels.
        let (w, h) = (512u32, 4u32);
        let stride = w as usize * 4 + 16;
        let mut src = Vec::new();
        for _ in 0..h {
            src.extend_from_slice(&solid(w, 1, [0, 255, 0, 255]));
            src.extend_from_slice(&[255; 16]);
        }
        let mut out = empty();
        shrink_into(&src, w, h, stride, PixelFormat::Rgba, &mut out).unwrap();
        assert!(out.data.chunks(4).all(|px| px == [0, 255, 0, 255]));
    }

    #[test]
    fn a_short_buffer_is_refused() {
        let mut out = empty();
        let src = solid(512, 4, [0; 4]);
        assert!(shrink_into(&src[..100], 512, 4, 512 * 4, PixelFormat::Rgba, &mut out).is_err());
        assert!(shrink_into(&src, 512, 4, 16, PixelFormat::Rgba, &mut out).is_err());
        assert!(shrink_into(&src, 0, 4, 0, PixelFormat::Rgba, &mut out).is_err());
    }

    #[test]
    fn a_new_screen_size_gets_its_own_taps() {
        // The cache must not serve one size's taps to another.
        let mut out = empty();
        let wide = solid(1024, 4, [9, 9, 9, 255]);
        shrink_into(&wide, 1024, 4, 1024 * 4, PixelFormat::Rgba, &mut out).unwrap();
        assert_eq!((out.width, out.height), (256, 1));
        let small = solid(300, 2, [7, 7, 7, 255]);
        shrink_into(&small, 300, 2, 300 * 4, PixelFormat::Rgba, &mut out).unwrap();
        assert_eq!((out.width, out.height), (300, 2));
        assert!(out.data.chunks(4).all(|px| px == [7, 7, 7, 255]));
        shrink_into(&wide, 1024, 4, 1024 * 4, PixelFormat::Rgba, &mut out).unwrap();
        assert_eq!(out.data.len(), 256 * 4);
    }

    #[test]
    fn taps_stay_inside_their_block() {
        for (from, to) in [
            (1920, 480),
            (2560, 320),
            (3840, 480),
            (300, 300),
            (1081, 270),
        ] {
            let t = taps(from, to);
            assert_eq!(t.len(), to);
            for (i, xs) in t.iter().enumerate() {
                let start = i * from / to;
                let end = ((i + 1) * from / to).max(start + 1);
                assert!(!xs.is_empty() && xs.len() <= MAX_TAPS);
                assert!(
                    xs.iter().all(|&x| x >= start && x < end && x < from),
                    "{from}->{to} #{i}"
                );
            }
        }
    }
}
