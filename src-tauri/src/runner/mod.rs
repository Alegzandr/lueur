//! The live loops: the ambilight, and the light shows.
//!
//! [`start`] snapshots the current config, opens a stream to every light it is
//! given (music mode for a Yeelight, whatever the family uses otherwise), then
//! spins a dedicated thread that computes and pushes colours every frame:
//! from the screen for the ambilight, from the clock or the sound for a show.
//! Engine and show settings are read from a shared cell each frame so the UI's
//! sliders take effect instantly without a restart; changing the *topology*
//! (which lights, where) requires a stop/start, which the UI does for you.
//!
//! Several loops may run at once, one per mode (see [`Effects`]): every light
//! playing the same mode is driven by the same loop, on the same clock, so
//! they move together.

mod ambilight;
mod connect;
mod effects;
mod glow;
mod show;
#[cfg(test)]
mod tests;

pub use effects::Effects;
pub use glow::{Glow, glows};

use crate::audio::{AudioError, AudioTap};
use crate::capture::{ScreenSource, check_monitor_index};
use crate::error::{AppError, AppResult};
use ambilight::run_ambilight;
use connect::connect;
use glow::{forget_glows, record_glow};
use lueur_core::color::{BulbOutput, Rgb};
use lueur_core::config::{AppConfig, AudioSource};
use lueur_core::device::lease::Leases;
use lueur_core::device::relink::{Link, LinkState, Relink};
use lueur_core::engine::EngineSettings;
use lueur_core::geometry::LightMapping;
use lueur_core::safety::SafetySettings;
use lueur_core::show::{ShowKind, ShowSettings};
use lueur_core::stage::Fixture;
use lueur_core::yeelight::Effect;
use show::run_show;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

/// Settings the loop re-reads every frame so UI changes apply live.
#[derive(Clone)]
pub struct LiveSettings {
    pub engine: EngineSettings,
    pub effect: Effect,
    pub duration_ms: u32,
    pub show: ShowSettings,
    /// The room's wall colour, which every output is corrected for.
    pub wall: Rgb,
    /// The photosensitivity protection every output goes through.
    pub safety: SafetySettings,
    /// The ambilight's updates per second.
    pub target_fps: u32,
    /// What the music show listens to.
    pub audio: AudioSource,
}

impl LiveSettings {
    pub fn of(config: &AppConfig) -> Self {
        LiveSettings {
            engine: config.engine,
            effect: config.transition.effect,
            duration_ms: config.transition.duration_ms,
            show: config.show,
            wall: config.room.wall,
            safety: config.safety,
            target_fps: config.capture.target_fps,
            audio: config.audio.clone(),
        }
    }

    /// The time between two ambilight updates.
    pub fn frame_budget(&self) -> Duration {
        frame_budget(self.target_fps)
    }
}

/// The time between two updates at `fps`, kept within 1 to 120 a second.
fn frame_budget(fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(fps.clamp(1, 120)))
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

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "ambilight" => Some(Mode::Ambilight),
            _ => ShowKind::from_id(id).map(Mode::Show),
        }
    }
}

/// A running effect. Dropping or calling [`Handle::stop`] tears it down cleanly.
pub struct Handle {
    stop: Arc<AtomicBool>,
    /// Whether the bulbs settle on a warm glow when the loop ends.
    settle: Arc<AtomicBool>,
    /// Whether, when a show ends, the bulbs that can play it by themselves
    /// do so rather than settle.
    keep: Arc<AtomicBool>,
    /// Set by the thread once it has wholly finished, settling included.
    done: Arc<AtomicBool>,
    live: Arc<Mutex<LiveSettings>>,
    thread: Option<JoinHandle<()>>,
    /// Human-readable summary of what started, for the UI/status.
    pub connected_lights: usize,
    /// The room lights the effect drives, which nothing else may touch.
    pub light_ids: Vec<String>,
    /// Each of those lights, and what its link to the bulb does right now.
    links: Vec<(String, Arc<LinkState>)>,
    pub mode: Mode,
    /// The music show's: whether it hears its source right now, rather than
    /// waits for it (an application closed, a device unplugged).
    hearing: Option<Arc<AtomicBool>>,
    /// The ambilight's: whether it reads the screen in HDR right now.
    hdr: Option<Arc<AtomicBool>>,
}

impl Handle {
    /// Signal the loop to stop and wait for it to finish (best-effort join).
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    /// Stop without settling the bulbs: another loop, or the hand, takes
    /// them over at once, and a warm flash in between would only distract.
    pub fn hand_over(&mut self) {
        self.settle.store(false, Ordering::SeqCst);
        self.stop();
    }

    /// Stop, leaving the bulbs that can playing the show by themselves (see
    /// [`ShowKind::own_loop`]): Lueur quits, and the show goes on without
    /// it until Lueur plays it again. The others settle as ever.
    pub fn leave_playing(&mut self) {
        self.keep.store(true, Ordering::SeqCst);
        self.stop();
    }

    /// Update the live-tunable settings.
    pub fn update_live(&self, settings: LiveSettings) {
        if let Ok(mut guard) = self.live.lock() {
            *guard = settings;
        }
    }

    /// The loop plays, or settles the bulbs after ending by itself: they are
    /// still its own until it has finished.
    pub fn is_running(&self) -> bool {
        !self.stop.load(Ordering::SeqCst) && !self.done.load(Ordering::SeqCst)
    }

    /// A music show waiting for its source to come back.
    pub fn waiting_for_sound(&self) -> bool {
        self.hearing
            .as_ref()
            .is_some_and(|h| !h.load(Ordering::Relaxed))
    }

    /// An ambilight reading an HDR screen, its colours brought back to SDR.
    pub fn hdr(&self) -> bool {
        self.hdr.as_ref().is_some_and(|h| h.load(Ordering::Relaxed))
    }

    /// The lights it drives whose bulb is not taking frames, and why: it
    /// dropped, or something else drives it.
    pub fn away(&self) -> impl Iterator<Item = (&String, Link)> {
        self.links
            .iter()
            .map(|(id, state)| (id, state.get()))
            .filter(|(_, link)| *link != Link::Up)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One connected, ready-to-drive light.
struct ConnectedLight {
    id: String,
    mapping: LightMapping,
    /// Where it stands and how fast it answers, for the shows: one fixture
    /// for each zone it plays as, left to right.
    fixtures: Vec<Fixture>,
    /// Its bulb, reconnected in the background whenever it drops.
    link: Relink,
}

/// How many of `ids` sit behind each host: lights behind one Hue bridge share
/// its command budget, whichever loop drives them.
pub fn per_host<'a>(
    config: &AppConfig,
    ids: impl IntoIterator<Item = &'a String>,
) -> HashMap<String, usize> {
    let mut hosts: HashMap<String, usize> = HashMap::new();
    for id in ids {
        if let Some(b) = config.binding_for(id) {
            *hosts.entry(b.ip.clone()).or_default() += 1;
        }
    }
    hosts
}

/// The lights an effect started for the whole room takes: every enabled
/// light that has a bulb, in room order.
pub fn enabled_ids(config: &AppConfig) -> Vec<String> {
    config
        .room
        .lights
        .iter()
        .filter(|l| l.enabled && config.binding_for(&l.id).is_some())
        .map(|l| l.id.clone())
        .collect()
}

/// Called when a loop ends by itself, set once at launch.
static ON_END: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// What to do when a loop ends without being stopped. It runs on the loop's
/// own thread, which must not wait on itself: anything that looks at the
/// running loops belongs on another thread.
pub fn on_end(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_END.set(Box::new(f));
}

/// Called when what a running loop shows changes, set once at launch.
static ON_STATUS: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// What to do when a driven light's bulb drops or comes back (see
/// [`Handle::away`]), or the ambilight starts or stops reading an HDR
/// screen (see [`Handle::hdr`]). It runs on the frame loop, so it must
/// return at once: anything slower belongs on another thread.
pub fn on_status(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_STATUS.set(Box::new(f));
}

fn status_changed() {
    if let Some(changed) = ON_STATUS.get() {
        changed();
    }
}

/// Run `body` on its own thread over the connected lights; when it returns
/// (stopped, or the capture or the sound gone) the bulbs settle, unless
/// handed over. A bulb that drops meanwhile does not end it: it is
/// reconnected while the others play on.
fn spawn(
    config: &AppConfig,
    mode: Mode,
    lights: Vec<ConnectedLight>,
    light_ids: Vec<String>,
    body: impl FnOnce(&mut [ConnectedLight], &Mutex<LiveSettings>, &AtomicBool) + Send + 'static,
) -> AppResult<Handle> {
    let live = Arc::new(Mutex::new(LiveSettings::of(config)));
    let stop = Arc::new(AtomicBool::new(false));
    let settle = Arc::new(AtomicBool::new(true));
    let keep = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let connected_lights = lights.len();
    let links = lights
        .iter()
        .map(|l| (l.id.clone(), l.link.state()))
        .collect();
    let thread = {
        let live = Arc::clone(&live);
        let stop = Arc::clone(&stop);
        let settle = Arc::clone(&settle);
        let keep = Arc::clone(&keep);
        let done = Arc::clone(&done);
        std::thread::Builder::new()
            .name(format!("lueur-{}", mode.id()))
            .spawn(move || {
                let mut lights = lights;
                body(&mut lights, &live, &stop);
                let ids: Vec<String> = lights.iter().map(|l| l.id.clone()).collect();
                forget_glows(&ids);
                if settle.load(Ordering::SeqCst) {
                    let own = match mode {
                        Mode::Show(kind) if keep.load(Ordering::SeqCst) => {
                            live.lock().ok().map(|l| Own(kind, l.show, l.wall))
                        }
                        _ => None,
                    };
                    rest(&mut lights, own);
                }
                done.store(true, Ordering::SeqCst);
                // Nobody asked it to stop: the capture or the sound could
                // not start. What shows it running must hear of it.
                if !stop.load(Ordering::SeqCst)
                    && let Some(ended) = ON_END.get()
                {
                    ended();
                }
            })
            .map_err(AppError::internal)?
    };
    Ok(Handle {
        stop,
        settle,
        keep,
        done,
        live,
        thread: Some(thread),
        connected_lights,
        light_ids,
        links,
        mode,
        hearing: None,
        hdr: None,
    })
}

/// Start `mode` on the lights `ids` (see [`connect`] for `hosts`), leaving
/// alone the bulbs other computers hold in `leases`.
///
/// Returns an error if none of them has a bulb that accepts a stream, if
/// capture can't start (ambilight) or if there is nothing to listen to
/// (music).
pub fn start(
    config: &AppConfig,
    mode: Mode,
    ids: &[String],
    hosts: &HashMap<String, usize>,
    leases: &Arc<Leases>,
) -> AppResult<Handle> {
    match mode {
        Mode::Ambilight => start_ambilight(config, ids, hosts, leases),
        Mode::Show(kind) => start_show(config, kind, ids, hosts, leases),
    }
}

fn start_ambilight(
    config: &AppConfig,
    ids: &[String],
    hosts: &HashMap<String, usize>,
    leases: &Arc<Leases>,
) -> AppResult<Handle> {
    // Validate the monitor choice before touching the network, so a bad one
    // fails fast without leaving bulbs in music mode. The source itself is
    // opened on the capture thread: its handle is not `Send`.
    let monitor_index = config.capture.monitor_index;
    check_monitor_index(monitor_index)?;
    let (lights, ids) = connect(config, ids, hosts, leases)?;
    let hdr = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&hdr);
    let mut handle = spawn(
        config,
        Mode::Ambilight,
        lights,
        ids,
        move |lights, live, stop| {
            // Opened here, not in `start`: the capture handle stays on this
            // thread for its whole life. A stop asked meanwhile (the user
            // still looking at Wayland's screen sharing dialog) cancels it.
            match ScreenSource::for_index(monitor_index, stop) {
                Ok(source) => run_ambilight(source, lights, live, stop, &seen),
                Err(e) => log::error!("capture unavailable: {e}"),
            }
        },
    )?;
    handle.hdr = Some(hdr);
    Ok(handle)
}

fn start_show(
    config: &AppConfig,
    kind: ShowKind,
    ids: &[String],
    hosts: &HashMap<String, usize>,
    leases: &Arc<Leases>,
) -> AppResult<Handle> {
    // Make sure this computer has sound to listen to before waking the
    // bulbs. The source picked being away (the application closed, the
    // device unplugged) is no reason not to start: the show waits for it,
    // and the window says so.
    let hearing = (kind == ShowKind::Music).then(|| Arc::new(AtomicBool::new(true)));
    if let Some(hearing) = &hearing {
        match AudioTap::open(&config.audio) {
            Ok(_) => {}
            Err(AudioError::AppClosed(_) | AudioError::DeviceMissing(_)) => {
                hearing.store(false, Ordering::Relaxed);
            }
            Err(e) => return Err(e.into()),
        }
    }
    let (lights, ids) = connect(config, ids, hosts, leases)?;
    let ears = hearing.clone();
    let mut handle = spawn(
        config,
        Mode::Show(kind),
        lights,
        ids,
        move |lights, live, stop| run_show(kind, ears, lights, live, stop),
    )?;
    handle.hearing = hearing;
    Ok(handle)
}

/// Which of a frame's outputs are each light's: one for each zone it plays
/// as, light after light.
fn spans(lights: &[ConnectedLight]) -> Vec<Range<usize>> {
    lights
        .iter()
        .scan(0, |at, l| {
            let span = *at..*at + l.fixtures.len();
            *at = span.end;
            Some(span)
        })
        .collect()
}

/// On stop, fade the bulbs to a gentle warm glow and leave them on, so the
/// room stays cosy rather than snapping to black. Brightness has to be set
/// explicitly: the effect may have ended on a dark frame, and the bulb would
/// otherwise hold that 1-2% brightness and look off. Every bulb at once, so
/// they settle together.
///
/// With `own`, each bulb that can plays the show by itself instead (see
/// [`ShowKind::own_loop`]), placed across the room as the show placed it.
fn rest(lights: &mut [ConnectedLight], own: Option<Own>) {
    let n = lights.len();
    std::thread::scope(|scope| {
        for (i, light) in lights.iter_mut().enumerate() {
            let place = if n > 1 {
                i as f32 / (n - 1) as f32
            } else {
                0.0
            };
            let own = own
                .and_then(|Own(kind, show, wall)| Some(kind.own_loop(&show, place)?.on_wall(wall)));
            scope.spawn(move || match own {
                Some(own) => light.link.rest_looping(&own),
                None => light.link.rest(),
            });
        }
    });
}

/// The show a loop played, how, and the walls it played on: what its bulbs
/// need to play it by themselves.
#[derive(Clone, Copy)]
struct Own(ShowKind, ShowSettings, Rgb);

/// Correct a frame for the walls, in place, and return it as it was asked
/// for, for the window to draw.
///
/// This comes before the flash guard: the guard must measure the light the
/// bulbs give, and a wall's correction can change it. Against a green wall,
/// an orange on screen becomes a saturated red on the bulb, which the guard
/// counts on its own.
fn on_walls(outputs: &mut [BulbOutput], wall: Rgb) -> Vec<BulbOutput> {
    let asked = outputs.to_vec();
    for o in outputs.iter_mut() {
        *o = o.on_wall(wall);
    }
    asked
}

/// Push one frame, an output for each zone of each light (`spans` says
/// which are whose), already corrected for the walls and guarded; `asked`
/// is the same frame before the walls' correction (see [`on_walls`]).
/// `pace` gives the transition of light `i`'s frame; `hold` says to keep
/// its next command for something sharp due on it.
fn push_zoned(
    lights: &mut [ConnectedLight],
    outputs: &[BulbOutput],
    asked: &[BulbOutput],
    spans: &[Range<usize>],
    pace: impl Fn(usize) -> (Effect, u32),
    hold: impl Fn(usize) -> bool,
) {
    for (i, (light, span)) in lights.iter_mut().zip(spans).enumerate() {
        let (Some(zones), Some(seen)) = (outputs.get(span.clone()), asked.get(span.clone())) else {
            continue;
        };
        if hold(i) {
            continue;
        }
        record_glow(&light.id, seen, zones);
        // A bulb that drops is reconnected in the background, without
        // holding up the others, and picks up the frame of the moment.
        let (effect, duration_ms) = pace(i);
        light.link.push_zones(zones, effect, duration_ms);
    }
}
