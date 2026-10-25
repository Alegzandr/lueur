//! Screen capture, abstracted behind [`FrameSource`] so the backend can evolve.
//!
//! On Windows the loop reads the desktop through DXGI Desktop Duplication,
//! shrunk on the GPU (see [`dxgi`]); where duplication refuses (a rotated
//! display, a remote session) it falls back to GDI, shrunk while it copies
//! (see [`gdi`]). On a Linux Wayland session it reads a ScreenCast portal
//! stream (see [`screencast`]) and falls back, without a portal, to
//! [`xcap`]'s one-shot capture, which is also what X11 and macOS use. Capture
//! is the one piece that must know about the OS, so it is kept deliberately
//! small: everything downstream consumes plain [`Frame`]s.
//!
//! ## Exclusive fullscreen
//! Desktop Duplication sees borderless *and* exclusive-fullscreen games on
//! Windows 10/11. If a title still refuses to be captured (a handful of
//! anti-cheat setups block it), switching that game to borderless-windowed
//! always works.
//!
//! ## Anti-cheat
//! Only whole monitors are captured, through the same OS APIs as OBS or
//! Discord screen share: the game process is never opened, read, hooked or
//! drawn over. `xcap::Window` is banned in `clippy.toml` because it opens the
//! owning process of every window it lists.

pub mod bench;
#[cfg(windows)]
mod dxgi;
#[cfg(windows)]
mod gdi;
#[cfg(any(windows, target_os = "linux"))]
mod scale;
#[cfg(target_os = "linux")]
mod screencast;

use crate::error::{AppError, AppResult};
use lueur_core::engine::{Frame, PixelFormat};
use std::sync::atomic::AtomicBool;
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;
use xcap::Monitor;

/// Something the ambilight loop can pull frames from.
///
/// Deliberately *not* `Send`: platform capture handles (a Windows `HMONITOR`, a
/// macOS display ref) are raw pointers and stay on the thread that opened them.
/// The loop builds its source inside the capture thread; see [`ScreenSource`].
pub trait FrameSource {
    /// Grab the current screen contents. A source that is told when the screen
    /// changes waits up to `timeout` for the next update and otherwise returns
    /// the previous image, which is still what is on screen.
    fn grab(&mut self, timeout: Duration) -> Result<&Frame, String>;
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
    monitor_at(index).map(drop)
}

fn all_monitors() -> AppResult<Vec<Monitor>> {
    Monitor::all().map_err(|e| AppError::new("monitors_unavailable").detail(e))
}

/// Desktop duplication failing this many grabs in a row (a display rotated
/// or unplugged mid-session) hands over to GDI rather than retrying forever.
#[cfg(windows)]
const DUPLICATION_FAILURES: u32 = 40;
/// How often GDI tries desktop duplication again, which comes back after a
/// UAC prompt or the lock screen and is what sees exclusive fullscreen.
#[cfg(windows)]
const DUPLICATION_RETRY: Duration = Duration::from_secs(5);

/// What a monitor is read through when nothing better works: GDI on
/// Windows, `xcap`'s one-shot capture elsewhere.
#[cfg(windows)]
type Fallback = gdi::GdiSource;
#[cfg(not(windows))]
type Fallback = XcapSource;

#[cfg(windows)]
fn fallback(monitor: Monitor) -> AppResult<Fallback> {
    gdi::GdiSource::new(monitor).map_err(|e| AppError::new("monitors_unavailable").detail(e))
}

#[cfg(not(windows))]
fn fallback(monitor: Monitor) -> AppResult<Fallback> {
    Ok(XcapSource {
        monitor,
        frame: None,
    })
}

#[cfg(windows)]
impl FrameSource for gdi::GdiSource {
    fn grab(&mut self, _timeout: Duration) -> Result<&Frame, String> {
        gdi::GdiSource::grab(self)
    }
}

/// The best [`FrameSource`] available for one monitor.
///
/// Build this on the thread that will pull frames from it: the underlying
/// handles are not thread-safe.
pub struct ScreenSource {
    #[cfg(windows)]
    duplication: Option<dxgi::DxgiSource>,
    /// Grabs in a row duplication has failed.
    #[cfg(windows)]
    failures: u32,
    /// When GDI next tries duplication again.
    #[cfg(windows)]
    retry_at: Instant,
    /// The ScreenCast portal stream, on a Wayland session that has one.
    #[cfg(target_os = "linux")]
    screencast: Option<screencast::ScreenCastSource>,
    fallback: Fallback,
}

impl ScreenSource {
    /// Build a source for the monitor at `index`.
    ///
    /// On Wayland this may show the desktop's screen sharing dialog, and
    /// waits for the user's answer, or for `cancel` to be set: the dialog is
    /// then closed and this returns at once.
    pub fn for_index(index: usize, cancel: &AtomicBool) -> AppResult<Self> {
        #[cfg(not(target_os = "linux"))]
        let _ = cancel;
        let monitor = monitor_at(index)?;
        Ok(ScreenSource {
            #[cfg(windows)]
            duplication: duplicate(&monitor, false),
            #[cfg(windows)]
            failures: 0,
            #[cfg(windows)]
            retry_at: Instant::now() + DUPLICATION_RETRY,
            #[cfg(target_os = "linux")]
            screencast: share(index, cancel)?,
            fallback: fallback(monitor)?,
        })
    }
}

/// Open the ScreenCast portal on a Wayland session. None on X11, or where
/// there is no portal: the one-shot capture then stands in, slowly.
#[cfg(target_os = "linux")]
fn share(index: usize, cancel: &AtomicBool) -> AppResult<Option<screencast::ScreenCastSource>> {
    if !screencast::is_wayland() {
        return Ok(None);
    }
    match screencast::ScreenCastSource::open(index, cancel) {
        Ok(source) => Ok(Some(source)),
        Err(screencast::Unavailable::Cancelled) => {
            Err(AppError::new("monitors_unavailable").detail("screen sharing was cancelled"))
        }
        Err(screencast::Unavailable::Refused) => {
            log::warn!("screen sharing was refused; the ambilight needs it on Wayland");
            Err(AppError::new("monitors_unavailable").detail("screen sharing was refused"))
        }
        Err(e) => {
            log::warn!(
                "the ScreenCast portal is unavailable ({e}); falling back to one \
                 screenshot per frame, which is slow and may ask for permission each time. \
                 Install xdg-desktop-portal and your desktop's portal backend to fix this."
            );
            Ok(None)
        }
    }
}

/// Desktop duplication of `monitor`. A failure is a warning the first time
/// (at start, or the retry after duplication was lost, which warned
/// already); the retries every few seconds after that only say so in debug,
/// or a remote session would log one every five seconds for as long as it
/// lasts.
#[cfg(windows)]
fn duplicate(monitor: &Monitor, retry: bool) -> Option<dxgi::DxgiSource> {
    let id = monitor.id().ok()?;
    dxgi::DxgiSource::open(id)
        .inspect_err(|e| {
            if retry {
                log::debug!("desktop duplication still unavailable: {e}");
            } else {
                log::warn!("desktop duplication unavailable, using GDI: {e}");
            }
        })
        .ok()
}

impl FrameSource for ScreenSource {
    fn grab(&mut self, timeout: Duration) -> Result<&Frame, String> {
        #[cfg(windows)]
        {
            if self.duplication.is_none() && Instant::now() >= self.retry_at {
                self.retry_at = Instant::now() + DUPLICATION_RETRY;
                self.duplication = duplicate(self.fallback.monitor(), true);
                self.failures = 0;
            }
            if let Some(d) = self.duplication.as_mut() {
                match d.grab(timeout) {
                    Ok(_) => self.failures = 0,
                    Err(e) => {
                        self.failures += 1;
                        if self.failures < DUPLICATION_FAILURES {
                            return Err(e);
                        }
                        log::warn!("desktop duplication lost, using GDI: {e}");
                        self.duplication = None;
                        self.retry_at = Instant::now() + DUPLICATION_RETRY;
                    }
                }
            }
            if let Some(frame) = self.duplication.as_ref().and_then(|d| d.frame()) {
                return Ok(frame);
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(s) = self.screencast.as_mut() {
            return s.grab(timeout);
        }
        FrameSource::grab(&mut self.fallback, timeout)
    }
}

fn monitor_at(index: usize) -> AppResult<Monitor> {
    let monitors = all_monitors()?;
    if monitors.is_empty() {
        return Err(AppError::new("no_monitors"));
    }
    monitors
        .into_iter()
        .nth(index)
        .ok_or_else(|| AppError::new("monitor_missing").subject((index + 1).to_string()))
}

/// A [`FrameSource`] backed by one `xcap` monitor: a fresh full-resolution
/// capture on every call. The fallback on X11 and macOS; on Windows only the
/// benchmark still measures it, to compare.
pub struct XcapSource {
    monitor: Monitor,
    frame: Option<Frame>,
}

impl FrameSource for XcapSource {
    fn grab(&mut self, _timeout: Duration) -> Result<&Frame, String> {
        // `capture_image` returns an RGBA image (top-left origin, tightly packed).
        let img = self
            .monitor
            .capture_image()
            .map_err(|e| format!("capture failed: {e}"))?;
        let width = img.width();
        let height = img.height();
        Ok(self
            .frame
            .insert(Frame::new(width, height, PixelFormat::Rgba, img.into_raw())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Time `xcap`'s one-shot capture on the real primary display. Needs a
    /// desktop session, so it only runs on request:
    /// `cargo test -p lueur capture_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn xcap_capture_cost() {
        let mut source = XcapSource {
            monitor: monitor_at(0).expect("monitor"),
            frame: None,
        };
        let runs = 30;
        let started = Instant::now();
        let mut size = (0, 0);
        for _ in 0..runs {
            let frame = source.grab(Duration::ZERO).expect("grab");
            size = (frame.width, frame.height);
        }
        let whole = lueur_core::geometry::SampleZone::new(0.0, 0.0, 1.0, 1.0);
        let average = source
            .grab(Duration::ZERO)
            .expect("grab")
            .average_zone(&whole, 2);
        println!("xcap: screen average {average:?}");
        println!(
            "xcap: {}x{} frame, {:.2} ms per capture",
            size.0,
            size.1,
            started.elapsed().as_secs_f64() * 1000.0 / runs as f64
        );
    }
}
