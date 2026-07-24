//! The live ambilight loop.
//!
//! [`start`] snapshots the current config, connects to every bound bulb in
//! music mode, then spins a dedicated thread that captures → computes → pushes
//! colours at the target frame rate. Engine settings (smoothing, brightness,
//! saturation…) are read from a shared cell each frame so the UI's sliders take
//! effect instantly without a restart; changing the *topology* (which lights,
//! where) requires a stop/start, which the UI does for you.

use crate::capture::{check_monitor_index, FrameSource, XcapSource};
use lueur_core::config::AppConfig;
use lueur_core::engine::{Ambilight, EngineSettings};
use lueur_core::geometry::{map_light, LightMapping};
use lueur_core::color::BulbOutput;
use lueur_core::yeelight::{Effect, MusicConnection};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Settings the loop re-reads every frame so UI changes apply live.
#[derive(Clone, Copy)]
pub struct LiveSettings {
    pub engine: EngineSettings,
    pub effect: Effect,
    pub duration_ms: u32,
}

/// A running effect. Dropping or calling [`Handle::stop`] tears it down cleanly.
pub struct Handle {
    stop: Arc<AtomicBool>,
    live: Arc<Mutex<LiveSettings>>,
    thread: Option<JoinHandle<()>>,
    /// Human-readable summary of what started, for the UI/status.
    pub connected_lights: usize,
}

impl Handle {
    /// Signal the loop to stop and wait for it to finish (best-effort join).
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    /// Update the live-tunable settings.
    pub fn update_live(&self, settings: LiveSettings) {
        if let Ok(mut guard) = self.live.lock() {
            *guard = settings;
        }
    }

    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::SeqCst)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One connected, ready-to-drive light.
struct ConnectedLight {
    mapping: LightMapping,
    conn: MusicConnection,
    /// What we last pushed, so brightness and colour temperature are only re-sent
    /// when they actually move.
    last: Option<BulbOutput>,
}

/// Start the ambilight effect from `config`.
///
/// Returns an error if there are no bound+enabled lights, capture can't start,
/// or none of the bulbs accept a music-mode connection.
pub fn start(config: &AppConfig) -> Result<Handle, String> {
    // Resolve the enabled lights that have a device binding, in room order, and
    // compute each one's screen mapping (zone plus distance shaping).
    let mut targets: Vec<(LightMapping, &lueur_core::config::DeviceBinding)> = Vec::new();
    for light in &config.room.lights {
        if !light.enabled {
            continue;
        }
        if let Some(binding) = config.binding_for(&light.id) {
            targets.push((map_light(&config.room.screen, light), binding));
        }
    }

    if targets.is_empty() {
        return Err("no lights are bound to a bulb — assign bulbs in the Devices tab".into());
    }

    // Validate the monitor choice before touching the network, so a bad one
    // fails fast without leaving bulbs in music mode. The source itself is
    // opened on the capture thread: its handle is not `Send`.
    let monitor_index = config.capture.monitor_index;
    check_monitor_index(monitor_index)?;

    // Establish music mode for each bulb. We tolerate individual failures (a
    // bulb may be off) as long as at least one connects.
    let handshake = Duration::from_secs(3);
    let mut connected: Vec<ConnectedLight> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (mapping, binding) in targets {
        match MusicConnection::establish(&binding.ip, binding.port, handshake) {
            Ok(conn) => connected.push(ConnectedLight {
                mapping,
                conn,
                last: None,
            }),
            Err(e) => errors.push(format!("{} ({}): {e}", binding.name, binding.ip)),
        }
    }

    if connected.is_empty() {
        return Err(format!(
            "could not connect to any bulb in music mode: {}",
            errors.join("; ")
        ));
    }

    let connected_lights = connected.len();
    let live = Arc::new(Mutex::new(LiveSettings {
        engine: config.engine,
        effect: config.transition.effect,
        duration_ms: config.transition.duration_ms,
    }));
    let stop = Arc::new(AtomicBool::new(false));

    let target_fps = config.capture.target_fps.clamp(1, 120);
    let frame_budget = Duration::from_secs_f64(1.0 / target_fps as f64);

    let thread = {
        let live = Arc::clone(&live);
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("lueur-ambilight".into())
            .spawn(move || {
                // Opened here, not in `start`: the capture handle stays on this
                // thread for its whole life.
                let source = match XcapSource::for_index(monitor_index) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("lueur: capture unavailable: {e}");
                        stop.store(true, Ordering::SeqCst);
                        return;
                    }
                };
                run_loop(source, connected, live, stop, frame_budget)
            })
            .map_err(|e| format!("failed to spawn ambilight thread: {e}"))?
    };

    Ok(Handle {
        stop,
        live,
        thread: Some(thread),
        connected_lights,
    })
}

/// Fit a transition into the time available before the next frame replaces it.
///
/// The user's transition setting is an upper bound on *smoothness*, not a
/// promise the bulb can keep: at 60 fps there are only ~17 ms between commands.
/// Anything longer is cut short by the next write, so we shorten it ourselves —
/// and when even Yeelight's 30 ms floor doesn't fit, we drop to `Sudden`. The
/// engine's own temporal smoothing is what makes the result look gradual;
/// asking the bulb to also interpolate just makes the two fight.
fn pace(effect: Effect, duration_ms: u32, frame_budget: Duration) -> (Effect, u32) {
    if effect == Effect::Sudden {
        return (Effect::Sudden, 0);
    }
    let budget = frame_budget.as_millis().min(u32::MAX as u128) as u32;
    let fitted = duration_ms.min(budget);
    if fitted < lueur_core::yeelight::MIN_SMOOTH_DURATION_MS {
        (Effect::Sudden, 0)
    } else {
        (Effect::Smooth, fitted)
    }
}

fn run_loop(
    mut source: XcapSource,
    mut lights: Vec<ConnectedLight>,
    live: Arc<Mutex<LiveSettings>>,
    stop: Arc<AtomicBool>,
    frame_budget: Duration,
) {
    let initial = live.lock().map(|g| *g).unwrap_or(LiveSettings {
        engine: EngineSettings::default(),
        effect: Effect::Smooth,
        duration_ms: 80,
    });
    let mut engine = Ambilight::new(lights.len(), initial.engine);

    // Track which lights are still healthy; a socket error retires a light
    // rather than killing the whole effect.
    let mut alive = vec![true; lights.len()];

    while !stop.load(Ordering::SeqCst) {
        let frame_start = Instant::now();

        let settings = live.lock().map(|g| *g).unwrap_or(initial);
        engine.set_settings(settings.engine);

        // A bulb transition must finish before the next command replaces it.
        // Left unclamped, a 300 ms fade issued every 17 ms is overwritten 18
        // times before it lands, and the bulb ends up chasing a target it never
        // reaches — which reads as fast, erratic colour swings.
        let (effect, duration_ms) = pace(settings.effect, settings.duration_ms, frame_budget);

        match source.grab() {
            Ok(frame) => {
                let mappings: Vec<LightMapping> = lights.iter().map(|l| l.mapping).collect();
                let outputs = engine.process_outputs(&frame, &mappings);
                for (i, light) in lights.iter_mut().enumerate() {
                    if !alive[i] {
                        continue;
                    }
                    let Some(&output) = outputs.get(i) else { continue };
                    // Chromaticity and brightness must both be pushed: `set_rgb`
                    // alone only carries hue, so a dark scene would otherwise
                    // stay at whatever brightness the bulb happened to have.
                    if light
                        .conn
                        .push(output, light.last, effect, duration_ms)
                        .is_err()
                    {
                        // Bulb dropped; stop trying to write to it.
                        alive[i] = false;
                        light.last = None;
                    } else {
                        light.last = Some(output);
                    }
                }
                if alive.iter().all(|&a| !a) {
                    break; // every bulb is gone; nothing left to do
                }
            }
            Err(_e) => {
                // Transient capture hiccup (e.g. a display mode switch). Back off
                // briefly and try again rather than tearing everything down.
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        // Frame pacing: sleep only the remaining budget.
        let elapsed = frame_start.elapsed();
        if elapsed < frame_budget {
            std::thread::sleep(frame_budget - elapsed);
        }
    }

    // On stop, fade the bulbs to a gentle warm glow and leave them on, so the
    // room stays cosy rather than snapping to black. Brightness has to be set
    // explicitly: the effect may have ended on a dark frame, and the bulb would
    // otherwise hold that 1-2% brightness and look off.
    for light in lights.iter_mut() {
        let _ = light.conn.set_bright(40, Effect::Smooth, 400);
        let _ = light.conn.set_ct(2700, Effect::Smooth, 400);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fps(n: u64) -> Duration {
        Duration::from_secs_f64(1.0 / n as f64)
    }

    #[test]
    fn sudden_stays_sudden() {
        assert_eq!(pace(Effect::Sudden, 300, fps(30)), (Effect::Sudden, 0));
    }

    #[test]
    fn transition_is_cut_to_the_frame_interval() {
        // The reported bug: 300 ms fades issued every ~17 ms at 60 fps.
        assert_eq!(pace(Effect::Smooth, 300, fps(60)), (Effect::Sudden, 0));
        // 30 fps leaves 33 ms, which clears Yeelight's 30 ms floor.
        assert_eq!(pace(Effect::Smooth, 300, fps(30)), (Effect::Smooth, 33));
    }

    #[test]
    fn a_short_request_is_left_alone() {
        // Well inside a 10 fps budget (100 ms): nothing to clamp.
        assert_eq!(pace(Effect::Smooth, 80, fps(10)), (Effect::Smooth, 80));
    }

    #[test]
    fn never_emits_a_smooth_below_the_protocol_floor() {
        for f in [15u64, 24, 30, 45, 60, 90, 120] {
            let (effect, ms) = pace(Effect::Smooth, 300, fps(f));
            if effect == Effect::Smooth {
                assert!(
                    ms >= lueur_core::yeelight::MIN_SMOOTH_DURATION_MS,
                    "{f} fps produced a {ms} ms smooth"
                );
            }
        }
    }
}
