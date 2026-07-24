//! The live ambilight loop.
//!
//! [`start`] snapshots the current config, connects to every bound bulb in
//! music mode, then spins a dedicated thread that captures → computes → pushes
//! colours at the target frame rate. Engine settings (smoothing, brightness,
//! saturation…) are read from a shared cell each frame so the UI's sliders take
//! effect instantly without a restart; changing the *topology* (which lights,
//! where) requires a stop/start, which the UI does for you.

use crate::capture::{FrameSource, XcapSource};
use lueur_core::config::AppConfig;
use lueur_core::engine::{Ambilight, EngineSettings};
use lueur_core::geometry::zone_for_light;
use lueur_core::yeelight::{Effect, MusicConnection};
use lueur_core::{Rgb, SampleZone};
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
    zone: SampleZone,
    conn: MusicConnection,
}

/// Start the ambilight effect from `config`.
///
/// Returns an error if there are no bound+enabled lights, capture can't start,
/// or none of the bulbs accept a music-mode connection.
pub fn start(config: &AppConfig) -> Result<Handle, String> {
    // Resolve the enabled lights that have a device binding, in room order, and
    // compute each one's screen sample zone.
    let mut targets: Vec<(SampleZone, &lueur_core::config::DeviceBinding)> = Vec::new();
    for light in &config.room.lights {
        if !light.enabled {
            continue;
        }
        if let Some(binding) = config.binding_for(&light.id) {
            let zone = zone_for_light(&config.room.screen, light).0;
            targets.push((zone, binding));
        }
    }

    if targets.is_empty() {
        return Err("no lights are bound to a bulb — assign bulbs in the Devices tab".into());
    }

    // Bring up the capture source before touching the network, so a bad monitor
    // choice fails fast without leaving bulbs in music mode.
    let source =
        XcapSource::for_index(config.capture.monitor_index).map_err(|e| e.to_string())?;

    // Establish music mode for each bulb. We tolerate individual failures (a
    // bulb may be off) as long as at least one connects.
    let handshake = Duration::from_secs(3);
    let mut connected: Vec<ConnectedLight> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (zone, binding) in targets {
        match MusicConnection::establish(&binding.ip, binding.port, handshake) {
            Ok(conn) => connected.push(ConnectedLight { zone, conn }),
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
            .spawn(move || run_loop(source, connected, live, stop, frame_budget))
            .map_err(|e| format!("failed to spawn ambilight thread: {e}"))?
    };

    Ok(Handle {
        stop,
        live,
        thread: Some(thread),
        connected_lights,
    })
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

        match source.grab() {
            Ok(frame) => {
                let zones: Vec<SampleZone> = lights.iter().map(|l| l.zone).collect();
                let colors: Vec<Rgb> = engine.process(&frame, &zones);
                for (i, light) in lights.iter_mut().enumerate() {
                    if !alive[i] {
                        continue;
                    }
                    let color = colors.get(i).copied().unwrap_or(Rgb::BLACK);
                    if light
                        .conn
                        .set_rgb(color, settings.effect, settings.duration_ms)
                        .is_err()
                    {
                        // Bulb dropped; stop trying to write to it.
                        alive[i] = false;
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
    // room stays cosy rather than snapping to black.
    for light in lights.iter_mut() {
        let _ = light
            .conn
            .set_rgb(Rgb::new(255, 147, 41), Effect::Smooth, 400);
    }
}
