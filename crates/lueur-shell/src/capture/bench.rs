//! What Lueur costs the machine, measured: `examples/capture_bench.rs`.
//!
//! Runs the ambilight's per-frame work, capture and colour computation, at a
//! target rate on a real display, without any bulb, and reports the time
//! each part takes and the CPU the whole process used meanwhile. GPU time
//! (Desktop Duplication's shrink) is not counted: it shows up as the few
//! microseconds the readback waits.

use super::{FrameSource, ScreenSource, XcapSource, monitor_at};
use lueur_core::engine::{Ambilight, EngineSettings};
use lueur_core::geometry::{LightMapping, SampleZone};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// The capture paths that can be measured on this system: `auto` is what the
/// ambilight uses.
pub fn backends() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["auto", "gdi", "xcap"]
    }
    #[cfg(not(windows))]
    {
        &["auto", "xcap"]
    }
}

/// Mean and 95th percentile of a set of timings, in milliseconds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub mean_ms: f64,
    pub p95_ms: f64,
}

impl Stats {
    pub fn of(samples: &[Duration]) -> Self {
        if samples.is_empty() {
            return Stats::default();
        }
        let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
        ms.sort_by(f64::total_cmp);
        let mean_ms = ms.iter().sum::<f64>() / ms.len() as f64;
        // Nearest rank: the smallest sample at or above 95% of them.
        let rank = (ms.len() * 95).div_ceil(100).max(1);
        Stats {
            mean_ms,
            p95_ms: ms[rank - 1],
        }
    }
}

/// One measured run.
#[derive(Debug, Clone)]
pub struct Report {
    pub backend: &'static str,
    /// Frames measured, and grabs that failed (left out of the timings).
    pub frames: usize,
    pub errors: usize,
    pub width: u32,
    pub height: u32,
    /// Reading one frame from the screen.
    pub capture: Stats,
    /// Turning it into one colour per light.
    pub compute: Stats,
    /// Process CPU time over wall time, in percent of one core, at the
    /// target rate. None where the OS does not say.
    pub cpu_percent: Option<f64>,
}

/// How the run is set up.
#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub monitor: usize,
    pub frames: usize,
    pub fps: u32,
    pub lights: usize,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            monitor: 0,
            frames: 300,
            fps: 45,
            lights: 8,
        }
    }
}

/// Zones the way a room tends to place its lights: half along the top edge,
/// half along the bottom.
fn mappings(lights: usize) -> Vec<LightMapping> {
    let top = lights.div_ceil(2).max(1);
    let bottom = (lights - top.min(lights)).max(1);
    (0..lights)
        .map(|i| {
            let (row, n, y) = if i < top {
                (i, top, 0.0)
            } else {
                (i - top, bottom, 0.7)
            };
            let w = 1.0 / n as f32;
            LightMapping::direct(SampleZone::new(row as f32 * w, y, w, 0.3))
        })
        .collect()
}

/// Same rule as the loop's: `sample_step` is tuned for a 1080p-wide frame.
fn scaled_step(step: u32, width: u32) -> u32 {
    (u64::from(step) * u64::from(width) / 1920).max(1) as u32
}

fn open(backend: &str, monitor: usize) -> Result<Box<dyn FrameSource>, String> {
    match backend {
        "auto" => Ok(Box::new(
            ScreenSource::for_index(monitor, &AtomicBool::new(false)).map_err(|e| e.to_string())?,
        )),
        "xcap" => Ok(Box::new(XcapSource {
            monitor: monitor_at(monitor).map_err(|e| e.to_string())?,
            frame: None,
        })),
        #[cfg(windows)]
        "gdi" => Ok(Box::new(super::gdi::GdiSource::new(
            monitor_at(monitor).map_err(|e| e.to_string())?,
        )?)),
        other => Err(format!("unknown capture backend {other:?}")),
    }
}

/// Measure `backend` (one of [`backends`]).
pub fn run(backend: &'static str, setup: Setup) -> Result<Report, String> {
    let mut source = open(backend, setup.monitor)?;
    let mappings = mappings(setup.lights);
    let mut engine = Ambilight::new(mappings.len(), EngineSettings::default());
    let budget = Duration::from_secs_f64(1.0 / setup.fps.clamp(1, 240) as f64);

    // A few frames first: the first grab opens sessions and sizes buffers.
    for _ in 0..5 {
        let _ = source.grab(budget);
    }

    let mut capture = Vec::with_capacity(setup.frames);
    let mut compute = Vec::with_capacity(setup.frames);
    let (mut width, mut height, mut errors) = (0, 0, 0);
    let cpu_before = cpu_time();
    let started = Instant::now();
    let mut tick = Instant::now();
    for _ in 0..setup.frames {
        // Paced like the loop: one frame per budget, the rest asleep.
        let elapsed = tick.elapsed();
        if elapsed < budget {
            std::thread::sleep(budget - elapsed);
        }
        tick = Instant::now();

        let t = Instant::now();
        let frame = match source.grab(Duration::ZERO) {
            Ok(frame) => frame,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        capture.push(t.elapsed());
        (width, height) = (frame.width, frame.height);

        let t = Instant::now();
        let mut settings = *engine.settings();
        settings.sample_step = scaled_step(EngineSettings::default().sample_step, frame.width);
        engine.set_settings(settings);
        engine.set_frame_time(budget.as_secs_f32());
        std::hint::black_box(engine.process_outputs(frame, &mappings));
        compute.push(t.elapsed());
    }
    let wall = started.elapsed();
    let cpu_percent = match (cpu_before, cpu_time()) {
        (Some(a), Some(b)) => Some((b - a).as_secs_f64() / wall.as_secs_f64() * 100.0),
        _ => None,
    };

    Ok(Report {
        backend,
        frames: capture.len(),
        errors,
        width,
        height,
        capture: Stats::of(&capture),
        compute: Stats::of(&compute),
        cpu_percent,
    })
}

/// User plus kernel time this process has used so far.
#[cfg(windows)]
fn cpu_time() -> Option<Duration> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the pseudo handle of this very process, and four out-params.
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    }
    .ok()?;
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // FILETIME counts 100 ns intervals.
    Some(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
}

/// User plus kernel time this process has used so far, from `/proc`.
#[cfg(target_os = "linux")]
fn cpu_time() -> Option<Duration> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // The command name, in parentheses, may itself hold spaces: count the
    // fields after it. utime and stime are the 14th and 15th.
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut fields = rest.split_whitespace().skip(11);
    let utime: u64 = fields.next()?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;
    // In clock ticks, which are 100 a second on every mainstream kernel.
    Some(Duration::from_millis((utime + stime) * 10))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn cpu_time() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_take_the_mean_and_the_95th_percentile() {
        let samples: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let s = Stats::of(&samples);
        assert!((s.mean_ms - 50.5).abs() < 1e-9);
        assert!((s.p95_ms - 95.0).abs() < 1e-9);
        let one = Stats::of(&[Duration::from_millis(3)]);
        assert!((one.p95_ms - 3.0).abs() < 1e-9);
        assert_eq!(Stats::of(&[]).mean_ms, 0.0);
    }

    #[test]
    fn zones_cover_both_edges() {
        for n in [1, 2, 3, 8] {
            let m = mappings(n);
            assert_eq!(m.len(), n);
        }
    }

    #[test]
    fn this_process_has_a_cpu_time() {
        #[cfg(any(windows, target_os = "linux"))]
        assert!(cpu_time().is_some());
    }
}
