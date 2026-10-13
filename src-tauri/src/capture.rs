//! Screen capture, abstracted behind [`FrameSource`] so the backend can evolve.
//!
//! The default backend is [`xcap`], which uses the best native API on each
//! platform (Windows Graphics Capture / ScreenCaptureKit / X11 or PipeWire on
//! Linux). Capture is the one piece that must know about the OS, so it is kept
//! deliberately small: everything downstream consumes plain [`Frame`]s.
//!
//! ## Exclusive fullscreen
//! `xcap` uses Windows Graphics Capture, which captures borderless *and*
//! exclusive-fullscreen games on Windows 10/11. If a title still refuses to be
//! captured (a handful of anti-cheat setups block it), switching that game to
//! borderless-windowed always works.

use crate::error::{AppError, AppResult};
use lueur_core::engine::{Frame, PixelFormat};
use xcap::Monitor;

/// Something the ambilight loop can pull frames from.
///
/// Deliberately *not* `Send`: platform capture handles (a Windows `HMONITOR`, a
/// macOS display ref) are raw pointers and stay on the thread that opened them.
/// The loop builds its source inside the capture thread — see [`XcapSource`].
pub trait FrameSource {
    /// Grab the current screen contents as an RGBA frame.
    fn grab(&mut self) -> Result<Frame, String>;
}

/// Lightweight description of a monitor for the UI's monitor picker.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// Enumerate the connected monitors, in a stable order.
pub fn list_monitors() -> AppResult<Vec<MonitorInfo>> {
    let monitors = all_monitors()?;
    let mut out = Vec::with_capacity(monitors.len());
    for (index, m) in monitors.iter().enumerate() {
        out.push(MonitorInfo {
            index,
            name: m
                .name()
                .unwrap_or_else(|_| format!("Display {}", index + 1)),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            is_primary: m.is_primary().unwrap_or(index == 0),
        });
    }
    Ok(out)
}

/// Check that a monitor exists at `index` without holding onto it.
///
/// Lets the caller fail fast on a stale monitor choice from a thread that will
/// never own the capture handle (see [`FrameSource`]).
pub fn check_monitor_index(index: usize) -> AppResult<()> {
    let monitors = all_monitors()?;
    if monitors.is_empty() {
        return Err(AppError::new("no_monitors"));
    }
    if index >= monitors.len() {
        return Err(AppError::new("monitor_missing").subject((index + 1).to_string()));
    }
    Ok(())
}

fn all_monitors() -> AppResult<Vec<Monitor>> {
    Monitor::all().map_err(|e| AppError::new("monitors_unavailable").detail(e))
}

/// A [`FrameSource`] backed by one `xcap` monitor.
///
/// Build this on the thread that will pull frames from it — the underlying
/// handle is not thread-safe.
pub struct XcapSource {
    monitor: Monitor,
}

impl XcapSource {
    /// Build a source for the monitor at `index` (falling back to the first).
    pub fn for_index(index: usize) -> AppResult<Self> {
        let monitors = all_monitors()?;
        if monitors.is_empty() {
            return Err(AppError::new("no_monitors"));
        }
        let monitor = monitors
            .into_iter()
            .nth(index)
            .ok_or_else(|| AppError::new("monitor_missing").subject((index + 1).to_string()))?;
        Ok(Self { monitor })
    }
}

impl FrameSource for XcapSource {
    fn grab(&mut self) -> Result<Frame, String> {
        // `capture_image` returns an RGBA image (top-left origin, tightly packed).
        let img = self
            .monitor
            .capture_image()
            .map_err(|e| format!("capture failed: {e}"))?;
        let width = img.width();
        let height = img.height();
        Ok(Frame::new(width, height, PixelFormat::Rgba, img.into_raw()))
    }
}
