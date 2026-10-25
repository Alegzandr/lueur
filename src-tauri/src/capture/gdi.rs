//! Windows fallback capture through GDI, shrunk while it is copied.
//!
//! Used where Desktop Duplication refuses (a rotated display, a remote
//! session, the moments after a UAC prompt). `xcap`'s one-shot capture copies
//! the whole desktop at full resolution into a fresh bitmap, reads it back and
//! swaps every pixel's channels, every frame. Here one `StretchBlt` samples
//! the desktop straight into a small DIB section, whose pixels are read in
//! place: no full-resolution copy ever reaches system memory, and the device
//! contexts and bitmap live as long as the source.
//!
//! Reading the composed desktop back from the GPU is the floor GDI cannot go
//! under. The shrink itself is chosen by measurement (`capture_bench`, see
//! docs/DEVELOPMENT.md): `HALFTONE` averages every pixel but spends more CPU
//! than the full copy it replaces, while `COLORONCOLOR` picks one pixel in
//! each block at well under half the CPU. Sampling at twice the DXGI
//! readback's size (one pixel in 16 at 1440p) keeps that from flickering:
//! each light still averages thousands of samples.

use super::scale;
use lueur_core::engine::{Frame, PixelFormat};
use std::ffi::c_void;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, COLORONCOLOR, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC,
    SRCCOPY, SelectObject, SetStretchBltMode, StretchBlt,
};
use xcap::Monitor;

/// The size the desktop is sampled at: twice the DXGI readback's, so that a
/// point sample does not flicker, and never more than the desktop itself.
fn sample_size(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = scale::target_size(width, height);
    ((w * 2).min(width), (h * 2).min(height))
}

/// A small bitmap the desktop is shrunk into.
struct Target {
    bitmap: HBITMAP,
    /// What the memory DC held before the bitmap, put back before deleting it.
    previous: HGDIOBJ,
    /// The bitmap's pixels: `width * height` BGRA quads, top row first.
    bits: *const u8,
    width: u32,
    height: u32,
}

/// One monitor read through GDI.
pub struct GdiSource {
    monitor: Monitor,
    screen: HDC,
    memory: HDC,
    target: Option<Target>,
    frame: Option<Frame>,
}

impl GdiSource {
    pub fn new(monitor: Monitor) -> Result<Self, String> {
        // SAFETY: plain GDI calls; both DCs are released in `Drop`.
        let screen = unsafe { GetDC(None) };
        if screen.is_invalid() {
            return Err("no screen device context".into());
        }
        let memory = unsafe { CreateCompatibleDC(Some(screen)) };
        if memory.is_invalid() {
            unsafe { ReleaseDC(None, screen) };
            return Err("no memory device context".into());
        }
        // One source pixel per destination pixel, colours kept as they are
        // (see the module notes for why not HALFTONE).
        unsafe { SetStretchBltMode(memory, COLORONCOLOR) };
        Ok(Self {
            monitor,
            screen,
            memory,
            target: None,
            frame: None,
        })
    }

    /// The monitor this source reads.
    pub fn monitor(&self) -> &Monitor {
        &self.monitor
    }

    /// Shrink the monitor's current contents into the frame.
    pub fn grab(&mut self) -> Result<&Frame, String> {
        // Asked every frame, as `xcap` did: the resolution or the layout can
        // change under a running loop.
        let m = &self.monitor;
        let geometry = (|| Some((m.x().ok()?, m.y().ok()?, m.width().ok()?, m.height().ok()?)))();
        let Some((x, y, width, height)) = geometry else {
            return Err("monitor geometry unavailable".into());
        };
        if width == 0 || height == 0 {
            return Err("monitor has no size".into());
        }
        let (small_w, small_h) = sample_size(width, height);
        if self
            .target
            .as_ref()
            .is_none_or(|t| (t.width, t.height) != (small_w, small_h))
        {
            self.release_target();
            self.target = Some(self.make_target(small_w, small_h)?);
        }
        let Some(t) = self.target.as_ref() else {
            return Err("no capture bitmap".into());
        };

        // SAFETY: both DCs and the selected bitmap are alive; GdiFlush makes
        // sure the blit has landed in the DIB before its bits are read.
        let copied = unsafe {
            let ok = StretchBlt(
                self.memory,
                0,
                0,
                t.width as i32,
                t.height as i32,
                Some(self.screen),
                x,
                y,
                width as i32,
                height as i32,
                SRCCOPY,
            );
            let _ = GdiFlush();
            ok.as_bool()
        };
        if !copied {
            return Err("StretchBlt failed".into());
        }

        let len = t.width as usize * t.height as usize * 4;
        // SAFETY: a 32-bit top-down DIB section of `width * height` pixels has
        // no row padding, and stays allocated until `release_target`.
        let pixels = unsafe { std::slice::from_raw_parts(t.bits, len) };
        let out = self
            .frame
            .get_or_insert_with(|| Frame::new(0, 0, PixelFormat::Bgra, Vec::new()));
        out.width = t.width;
        out.height = t.height;
        out.format = PixelFormat::Bgra;
        out.data.clear();
        out.data.extend_from_slice(pixels);
        Ok(out)
    }

    fn make_target(&self, width: u32, height: u32) -> Result<Target, String> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negative: rows top to bottom, as the engine reads them.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        // SAFETY: `info` describes a plain 32-bit DIB; `bits` receives its
        // pixel memory, owned by the bitmap.
        let bitmap = unsafe {
            CreateDIBSection(Some(self.memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        }
        .map_err(|e| format!("capture bitmap: {e}"))?;
        if bits.is_null() {
            unsafe {
                let _ = DeleteObject(bitmap.into());
            }
            return Err("capture bitmap has no pixels".into());
        }
        let previous = unsafe { SelectObject(self.memory, bitmap.into()) };
        Ok(Target {
            bitmap,
            previous,
            bits: bits as *const u8,
            width,
            height,
        })
    }

    fn release_target(&mut self) {
        if let Some(t) = self.target.take() {
            // SAFETY: the bitmap is deselected before it is deleted.
            unsafe {
                SelectObject(self.memory, t.previous);
                let _ = DeleteObject(t.bitmap.into());
            }
        }
    }
}

impl Drop for GdiSource {
    fn drop(&mut self) {
        self.release_target();
        // SAFETY: created in `new` and used by nothing else.
        unsafe {
            let _ = DeleteDC(self.memory);
            ReleaseDC(None, self.screen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Time the shrinking GDI copy on the real primary display. Needs a
    /// desktop session, so it only runs on request:
    /// `cargo test -p lueur capture_cost -- --ignored --nocapture`
    #[test]
    fn samples_at_twice_the_readback() {
        assert_eq!(sample_size(2560, 1440), (640, 360));
        assert_eq!(sample_size(1920, 1080), (960, 540));
        assert_eq!(sample_size(3840, 2160), (960, 540));
        // A small display is read whole, never stretched.
        assert_eq!(sample_size(300, 200), (300, 200));
    }

    #[test]
    #[ignore]
    fn gdi_capture_cost() {
        let monitor = Monitor::all().expect("monitors").remove(0);
        let mut source = GdiSource::new(monitor).expect("gdi");
        let runs = 60;
        let started = Instant::now();
        for _ in 0..runs {
            source.grab().expect("grab");
        }
        let elapsed = started.elapsed();
        let f = source.grab().expect("grab");
        let whole = lueur_core::geometry::SampleZone::new(0.0, 0.0, 1.0, 1.0);
        println!("gdi: screen average {:?}", f.average_zone(&whole, 1));
        println!(
            "gdi: {}x{} frame, {:.2} ms per capture",
            f.width,
            f.height,
            elapsed.as_secs_f64() * 1000.0 / runs as f64
        );
    }
}
