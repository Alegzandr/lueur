//! The ambilight's loop: the screen, read and pushed to the lights.

use super::{ConnectedLight, LiveSettings, on_walls, push_zoned, spans, status_changed};
use crate::capture::{FrameSource, ScreenSource};
use lueur_core::engine::Ambilight;
use lueur_core::geometry::LightMapping;
use lueur_core::safety::FlashGuard;
use lueur_core::stage::Fixture;
use lueur_core::sync::{self, Lag};
use lueur_core::yeelight::Effect;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the screen may stay unreadable before the window says so. Longer
/// than a display mode change takes (a few seconds, see `DUPLICATION_PATIENCE`
/// in `capture.rs`), so switching HDR on does not flash a warning.
const LOST_AFTER: Duration = Duration::from_secs(10);

/// What the ambilight tells the window about the screen it reads.
pub(super) struct Flags {
    /// It reads the screen in HDR.
    pub hdr: Arc<AtomicBool>,
    /// It has not been able to read the screen for [`LOST_AFTER`].
    pub lost: Arc<AtomicBool>,
}

/// Fit a transition into the time available before the next frame replaces it.
///
/// The user's transition setting is an upper bound on *smoothness*, not a
/// promise the bulb can keep: at 60 fps there are only ~17 ms between commands.
/// Anything longer is cut short by the next write and leaves the bulb chasing
/// a target it never reaches, so we shorten it ourselves.
///
/// It is never shortened to nothing, though. A fade about as long as the gap
/// between two commands is what turns a stream of steps into a continuous
/// glide, so a budget below Yeelight's 30 ms floor gets the floor (overlapping
/// the next frame by a few ms is harmless), and families that pace themselves
/// further stretch it to their own interval (see `Throttle::glide`).
pub(super) fn pace(effect: Effect, duration_ms: u32, frame_budget: Duration) -> (Effect, u32) {
    if effect == Effect::Sudden {
        return (Effect::Sudden, 0);
    }
    let budget = frame_budget.as_millis().min(u32::MAX as u128) as u32;
    let fitted = duration_ms
        .min(budget)
        .max(lueur_core::yeelight::MIN_SMOOTH_DURATION_MS);
    (Effect::Smooth, fitted)
}

/// `sample_step` is tuned for a 1080p-wide capture. A wider frame takes a
/// proportionally longer stride (same cost, same detail), and one the GPU has
/// already shrunk is read in full.
pub(super) fn scaled_step(step: u32, width: u32) -> u32 {
    (u64::from(step) * u64::from(width) / 1920).max(1) as u32
}

pub(super) fn run_ambilight(
    mut source: ScreenSource,
    lights: &mut [ConnectedLight],
    live: &Mutex<LiveSettings>,
    stop: &AtomicBool,
    flags: &Flags,
) {
    let Ok(initial) = live.lock().map(|g| g.clone()) else {
        return;
    };
    // A light that colours several parts of itself mirrors a part of the
    // picture with each (see `map_zones`).
    let mappings: Vec<LightMapping> = lights
        .iter()
        .flat_map(|l| l.mappings.iter().copied())
        .collect();
    let spans = spans(lights);
    let mut engine = Ambilight::new(mappings.len(), initial.engine);
    let mut guard = FlashGuard::ambilight(mappings.len());
    // The quick lights wait for the slowest of them, so a cut on screen
    // reaches the whole room at once.
    let fixtures: Vec<Fixture> = lights
        .iter()
        .flat_map(|l| l.fixtures.iter().copied())
        .collect();
    let (_, waits) = sync::timing(&fixtures, 0.0);
    let mut lag = Lag::default();
    let mut unsharp = vec![false; mappings.len()];
    let mut last_tick: Option<Instant> = None;
    let mut last_frame: Option<Instant> = None;
    // Since when the screen has refused to be read, every grab.
    let mut failing_since: Option<Instant> = None;

    while !stop.load(Ordering::SeqCst) {
        // Read first: the rate itself is live, a new one paces this frame.
        let settings = live
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| initial.clone());
        let frame_budget = settings.frame_budget();

        // Cap bulb updates at the target rate. Whatever the screen presents
        // meanwhile piles up in the capture source, so the grab below returns
        // it at once; on a still screen it waits for the next present instead
        // of re-reading an unchanged desktop.
        if let Some(tick) = last_tick {
            let elapsed = tick.elapsed();
            if elapsed < frame_budget {
                std::thread::sleep(frame_budget - elapsed);
            }
        }
        last_tick = Some(Instant::now());

        // A bulb transition must finish before the next command replaces it.
        // Left unclamped, a 300 ms fade issued every 17 ms is overwritten 18
        // times before it lands, and the bulb ends up chasing a target it never
        // reaches, which reads as fast, erratic colour swings.
        let (effect, duration_ms) = pace(settings.effect, settings.duration_ms, frame_budget);

        let grabbed = source.grab(frame_budget);
        let lost = match &grabbed {
            Ok(_) => {
                failing_since = None;
                false
            }
            Err(e) => {
                let since = *failing_since.get_or_insert_with(|| {
                    log::warn!("the screen cannot be read: {e}");
                    Instant::now()
                });
                since.elapsed() >= LOST_AFTER
            }
        };
        if flags.lost.swap(lost, Ordering::Relaxed) != lost {
            if lost {
                log::error!(
                    "the screen has not been readable for {LOST_AFTER:?}; the lights hold their colour"
                );
            } else {
                log::info!("the screen can be read again");
            }
            status_changed();
        }
        match grabbed {
            Ok(frame) => {
                // The real gap, not the budget: a still screen delivers frames
                // late, and the smoothing should catch up by the time that
                // passed. Capped so a long stall is not one giant step.
                let now = Instant::now();
                let dt = last_frame.map_or(frame_budget, |t| now - t);
                last_frame = Some(now);
                let dt = dt.as_secs_f32().min(0.25);
                engine.set_frame_time(dt);
                let mut tuned = settings.engine;
                tuned.sample_step = scaled_step(tuned.sample_step, frame.width);
                engine.set_settings(tuned);
                let outputs = engine.process_outputs(frame, &mappings);
                let mut outputs = lag.run(dt, outputs, &mut unsharp, &waits);
                let asked = on_walls(&mut outputs, settings.wall);
                guard.apply(&mut outputs, dt, settings.safety.flash_guard);
                push_zoned(
                    lights,
                    &outputs,
                    &asked,
                    &spans,
                    |_| (effect, duration_ms),
                    |_| false,
                );
            }
            Err(_) => {
                // Transient capture hiccup (e.g. a display mode switch). Back off
                // briefly and try again rather than tearing everything down.
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        // HDR turned on or off in Windows reopens the capture in its new
        // format; the window says which one the colours come from.
        let reads_hdr = source.hdr();
        if flags.hdr.swap(reads_hdr, Ordering::Relaxed) != reads_hdr {
            status_changed();
        }
    }
}
