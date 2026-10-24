//! The live loops: the ambilight, and the light shows.
//!
//! [`start`] and [`start_show`] snapshot the current config, open a stream to
//! every bound bulb that takes part (music mode for a Yeelight, whatever the
//! family uses otherwise), then spin a dedicated thread that computes and
//! pushes colours every frame — from the screen for the ambilight, from the
//! clock or the sound for a show. Engine and show settings are read from a
//! shared cell each frame so the UI's sliders take effect instantly without a
//! restart; changing the *topology* (which lights, where) requires a
//! stop/start, which the UI does for you.

use crate::audio::AudioTap;
use crate::capture::{FrameSource, ScreenSource, check_monitor_index};
use crate::devices;
use crate::error::{AppError, AppResult};
use lueur_core::color::BulbOutput;
use lueur_core::config::AppConfig;
use lueur_core::device::LightStream;
use lueur_core::engine::{Ambilight, EngineSettings};
use lueur_core::geometry::{LightMapping, map_light};
use lueur_core::show::{self, Analyzer, Candle, Dance, ShowKind, ShowSettings};
use lueur_core::yeelight::Effect;
use std::collections::HashMap;
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
    pub show: ShowSettings,
}

impl LiveSettings {
    pub fn of(config: &AppConfig) -> Self {
        LiveSettings {
            engine: config.engine,
            effect: config.transition.effect,
            duration_ms: config.transition.duration_ms,
            show: config.show,
        }
    }
}

/// What a running loop plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Ambilight,
    Show(ShowKind),
}

impl Mode {
    /// How the interface names it: `ambilight`, or the show's id.
    pub fn id(self) -> &'static str {
        match self {
            Mode::Ambilight => "ambilight",
            Mode::Show(kind) => kind.id(),
        }
    }
}

/// A running effect. Dropping or calling [`Handle::stop`] tears it down cleanly.
pub struct Handle {
    stop: Arc<AtomicBool>,
    live: Arc<Mutex<LiveSettings>>,
    thread: Option<JoinHandle<()>>,
    /// Human-readable summary of what started, for the UI/status.
    pub connected_lights: usize,
    /// The room lights the effect drives, which nothing else may touch.
    pub light_ids: Vec<String>,
    pub mode: Mode,
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
    conn: Box<dyn LightStream>,
}

/// Open a stream to every enabled light that has a bulb, in room order.
///
/// Returns an error if there are none, or if none of the bulbs accept a
/// stream; individual failures are tolerated.
fn connect(config: &AppConfig) -> AppResult<(Vec<ConnectedLight>, Vec<String>)> {
    // Resolve the enabled lights that have a device binding, in room order, and
    // compute each one's screen mapping (zone plus distance shaping).
    let targets: Vec<_> = config
        .room
        .lights
        .iter()
        .filter(|l| l.enabled)
        .filter_map(|l| {
            let binding = config.binding_for(&l.id)?;
            Some((map_light(&config.room.screen, l), binding, l))
        })
        .collect();
    if targets.is_empty() {
        return Err(AppError::new("no_active_lights"));
    }

    // Lights behind one host (a Hue bridge) share its command budget.
    let mut per_host: HashMap<&str, usize> = HashMap::new();
    for (_, binding, _) in &targets {
        *per_host.entry(binding.ip.as_str()).or_default() += 1;
    }

    // Open every bulb at once: an unplugged one costs its whole timeout, and
    // waiting on each in turn would make a few of them feel like a hang.
    let opened: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = targets
            .iter()
            .map(|(_, binding, _)| {
                let siblings = per_host[binding.ip.as_str()];
                scope.spawn(move || devices::open_stream(&binding.endpoint(), siblings))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(std::io::Error::other("bulb thread panicked")))
            })
            .collect()
    });
    let mut connected = Vec::new();
    let mut light_ids = Vec::new();
    let mut failed = Vec::new();
    let mut details = Vec::new();
    for ((mapping, binding, light), result) in targets.into_iter().zip(opened) {
        match result {
            Ok(conn) => {
                connected.push(ConnectedLight { mapping, conn });
                light_ids.push(light.id.clone());
            }
            Err(e) => {
                failed.push(light.name.clone());
                details.push(format!("{} ({}): {e}", light.name, binding.ip));
            }
        }
    }
    if connected.is_empty() {
        return Err(AppError::new("music_mode_failed")
            .subject(failed.join(", "))
            .detail(details.join("; ")));
    }
    Ok((connected, light_ids))
}

/// Run `body` on its own thread over the connected lights; when it returns
/// (stopped, or every bulb gone) the bulbs settle.
fn spawn(
    config: &AppConfig,
    mode: Mode,
    lights: Vec<ConnectedLight>,
    light_ids: Vec<String>,
    body: impl FnOnce(&mut [ConnectedLight], &Mutex<LiveSettings>, &AtomicBool) + Send + 'static,
) -> AppResult<Handle> {
    let live = Arc::new(Mutex::new(LiveSettings::of(config)));
    let stop = Arc::new(AtomicBool::new(false));
    let connected_lights = lights.len();
    let thread = {
        let live = Arc::clone(&live);
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name(format!("lueur-{}", mode.id()))
            .spawn(move || {
                let mut lights = lights;
                body(&mut lights, &live, &stop);
                stop.store(true, Ordering::SeqCst);
                rest(&mut lights);
            })
            .map_err(AppError::internal)?
    };
    Ok(Handle {
        stop,
        live,
        thread: Some(thread),
        connected_lights,
        light_ids,
        mode,
    })
}

/// Start the ambilight from `config`.
///
/// Returns an error if there are no bound+enabled lights, capture can't start,
/// or none of the bulbs accept a stream.
pub fn start(config: &AppConfig) -> AppResult<Handle> {
    // Validate the monitor choice before touching the network, so a bad one
    // fails fast without leaving bulbs in music mode. The source itself is
    // opened on the capture thread: its handle is not `Send`.
    let monitor_index = config.capture.monitor_index;
    if config.room.lights.iter().any(|l| l.enabled) {
        check_monitor_index(monitor_index)?;
    }
    let (lights, ids) = connect(config)?;
    let target_fps = config.capture.target_fps.clamp(1, 120);
    let frame_budget = Duration::from_secs_f64(1.0 / target_fps as f64);
    spawn(
        config,
        Mode::Ambilight,
        lights,
        ids,
        move |lights, live, stop| {
            // Opened here, not in `start`: the capture handle stays on this
            // thread for its whole life.
            match ScreenSource::for_index(monitor_index) {
                Ok(source) => run_ambilight(source, lights, live, stop, frame_budget),
                Err(e) => eprintln!("lueur: capture unavailable: {e}"),
            }
        },
    )
}

/// How often a show computes a frame. Shows move slowly next to a game: this
/// is smooth to the eye and well inside every family's command budget.
const SHOW_FRAME: Duration = Duration::from_millis(40);

/// Start a light show on the same lights the ambilight would drive.
pub fn start_show(config: &AppConfig, kind: ShowKind) -> AppResult<Handle> {
    // Make sure there is something to listen to before waking the bulbs.
    if kind == ShowKind::Music {
        AudioTap::open().map_err(|e| AppError::new("audio_unavailable").detail(e))?;
    }
    let (lights, ids) = connect(config)?;
    spawn(
        config,
        Mode::Show(kind),
        lights,
        ids,
        move |lights, live, stop| run_show(kind, lights, live, stop),
    )
}

/// Push one frame to every light still answering. False once none is.
fn push_all(
    lights: &mut [ConnectedLight],
    alive: &mut [bool],
    outputs: &[BulbOutput],
    effect: Effect,
    duration_ms: u32,
) -> bool {
    for (i, light) in lights.iter_mut().enumerate() {
        let Some(&output) = outputs.get(i) else {
            continue;
        };
        // A bulb that drops is retired rather than killing the whole effect.
        if alive[i] && light.conn.push(output, effect, duration_ms).is_err() {
            alive[i] = false;
        }
    }
    alive.iter().any(|&a| a)
}

/// On stop, fade the bulbs to a gentle warm glow and leave them on, so the
/// room stays cosy rather than snapping to black. Brightness has to be set
/// explicitly: the effect may have ended on a dark frame, and the bulb would
/// otherwise hold that 1-2% brightness and look off. Every bulb at once, so
/// they settle together.
fn rest(lights: &mut [ConnectedLight]) {
    std::thread::scope(|scope| {
        for light in lights.iter_mut() {
            scope.spawn(move || light.conn.rest());
        }
    });
}

fn run_show(
    kind: ShowKind,
    lights: &mut [ConnectedLight],
    live: &Mutex<LiveSettings>,
    stop: &AtomicBool,
) {
    let n = lights.len();
    let mut alive = vec![true; n];
    let mut candle = Candle::new(n);
    let mut dance = Dance::default();
    let tap = if kind == ShowKind::Music {
        match AudioTap::open() {
            Ok(tap) => Some(tap),
            Err(e) => {
                eprintln!("lueur: audio unavailable: {e}");
                return;
            }
        }
    } else {
        None
    };
    let mut analyzer = Analyzer::new(tap.as_ref().map_or(48_000, |t| t.sample_rate));
    let (effect, duration_ms) = pace(Effect::Smooth, SHOW_FRAME.as_millis() as u32, SHOW_FRAME);

    let started = Instant::now();
    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(SHOW_FRAME.saturating_sub(last.elapsed()));
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        let Ok(settings) = live.lock().map(|g| g.show) else {
            break;
        };
        let outputs = match kind {
            ShowKind::Candle => candle.frame(&settings, dt),
            ShowKind::Music => {
                let samples = tap.as_ref().map(AudioTap::take).unwrap_or_default();
                let levels = analyzer.feed(&samples, dt);
                dance.frame(&settings, levels, dt, n)
            }
            ShowKind::Rainbow | ShowKind::Sunset | ShowKind::Ocean => show::fade(
                &ShowSettings { kind, ..settings },
                started.elapsed().as_secs_f32(),
                n,
            ),
        };
        if !push_all(lights, &mut alive, &outputs, effect, duration_ms) {
            break;
        }
    }
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

/// `sample_step` is tuned for a 1080p-wide capture. A wider frame takes a
/// proportionally longer stride (same cost, same detail), and one the GPU has
/// already shrunk is read in full.
fn scaled_step(step: u32, width: u32) -> u32 {
    (u64::from(step) * u64::from(width) / 1920).max(1) as u32
}

fn run_ambilight(
    mut source: ScreenSource,
    lights: &mut [ConnectedLight],
    live: &Mutex<LiveSettings>,
    stop: &AtomicBool,
    frame_budget: Duration,
) {
    let Ok(initial) = live.lock().map(|g| *g) else {
        return;
    };
    let mut engine = Ambilight::new(lights.len(), initial.engine);
    let mappings: Vec<LightMapping> = lights.iter().map(|l| l.mapping).collect();
    let mut alive = vec![true; lights.len()];
    let mut last_tick: Option<Instant> = None;

    while !stop.load(Ordering::SeqCst) {
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

        let settings = live.lock().map(|g| *g).unwrap_or(initial);

        // A bulb transition must finish before the next command replaces it.
        // Left unclamped, a 300 ms fade issued every 17 ms is overwritten 18
        // times before it lands, and the bulb ends up chasing a target it never
        // reaches — which reads as fast, erratic colour swings.
        let (effect, duration_ms) = pace(settings.effect, settings.duration_ms, frame_budget);

        match source.grab(frame_budget) {
            Ok(frame) => {
                engine.set_settings(EngineSettings {
                    sample_step: scaled_step(settings.engine.sample_step, frame.width),
                    ..settings.engine
                });
                let outputs = engine.process_outputs(frame, &mappings);
                if !push_all(lights, &mut alive, &outputs, effect, duration_ms) {
                    break; // every bulb is gone; nothing left to do
                }
            }
            Err(_e) => {
                // Transient capture hiccup (e.g. a display mode switch). Back off
                // briefly and try again rather than tearing everything down.
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fps(n: u64) -> Duration {
        Duration::from_secs_f64(1.0 / n as f64)
    }

    #[test]
    fn sample_step_follows_capture_width() {
        assert_eq!(scaled_step(4, 1920), 4);
        assert_eq!(scaled_step(4, 3840), 8);
        // A GPU-shrunk frame is read pixel by pixel.
        assert_eq!(scaled_step(4, 480), 1);
        assert_eq!(scaled_step(1, 0), 1);
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
