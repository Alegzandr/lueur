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

use crate::audio::{AudioError, AudioTap};
use crate::capture::{FrameSource, ScreenSource, check_monitor_index};
use crate::devices;
use crate::error::{AppError, AppResult};
use lueur_core::color::{BulbOutput, Rgb};
use lueur_core::config::{AppConfig, AudioSource};
use lueur_core::device::relink::{OnChange, Opener, Relink};
use lueur_core::engine::{Ambilight, EngineSettings};
use lueur_core::geometry::{LightMapping, map_light};
use lueur_core::safety::{FlashGuard, SafetySettings};
use lueur_core::show::{Show, ShowKind, ShowSettings};
use lueur_core::stage::{Analyzer, Fixture, Levels};
use lueur_core::yeelight::Effect;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
            show: config.show.clone(),
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
    /// Set by the thread once it has wholly finished, settling included.
    done: Arc<AtomicBool>,
    live: Arc<Mutex<LiveSettings>>,
    thread: Option<JoinHandle<()>>,
    /// Human-readable summary of what started, for the UI/status.
    pub connected_lights: usize,
    /// The room lights the effect drives, which nothing else may touch.
    pub light_ids: Vec<String>,
    /// Each of those lights, and whether its bulb is reachable right now.
    links: Vec<(String, Arc<AtomicBool>)>,
    pub mode: Mode,
    /// The music show's: whether it hears its source right now, rather than
    /// waits for it (an application closed, a device unplugged).
    hearing: Option<Arc<AtomicBool>>,
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

    /// The lights it drives whose bulb dropped and is being reconnected.
    pub fn reconnecting(&self) -> impl Iterator<Item = &String> {
        self.links
            .iter()
            .filter(|(_, up)| !up.load(Ordering::SeqCst))
            .map(|(id, _)| id)
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
    /// Where it stands and how fast it answers, for the music show.
    fixture: Fixture,
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

/// Open a stream to each of `ids` that has a bulb, in room order. `hosts`
/// counts every light that will be driven once this starts (see
/// [`per_host`]).
///
/// Returns an error if there are none, or if none of the bulbs accept a
/// stream; individual failures are tolerated.
fn connect(
    config: &AppConfig,
    ids: &[String],
    hosts: &HashMap<String, usize>,
) -> AppResult<(Vec<ConnectedLight>, Vec<String>)> {
    // Resolve the lights that have a device binding, in room order, and
    // compute each one's screen mapping (zone plus distance shaping).
    let targets: Vec<_> = config
        .room
        .lights
        .iter()
        .filter(|l| ids.contains(&l.id))
        .filter_map(|l| {
            let binding = config.binding_for(&l.id)?;
            Some((map_light(&config.room.screen, l), binding, l))
        })
        .collect();
    if targets.is_empty() {
        return Err(AppError::new("no_active_lights"));
    }

    // How each bulb is opened, now and again whenever it drops.
    let openers: Vec<Opener> = targets
        .iter()
        .map(|(_, binding, _)| {
            let endpoint = binding.endpoint();
            let siblings = hosts.get(&binding.ip).copied().unwrap_or(1);
            Arc::new(move || devices::open_stream(&endpoint, siblings)) as Opener
        })
        .collect();
    // Open every bulb at once: an unplugged one costs its whole timeout, and
    // waiting on each in turn would make a few of them feel like a hang.
    let opened: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = openers
            .iter()
            .map(|open| scope.spawn(move || open()))
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
    let changed: OnChange = Arc::new(links_changed);
    for (((mapping, binding, light), result), open) in targets.into_iter().zip(opened).zip(openers)
    {
        match result {
            Ok(conn) => {
                let fixture = Fixture::place(&config.room.screen, light, conn.interval());
                connected.push(ConnectedLight {
                    id: light.id.clone(),
                    mapping,
                    fixture,
                    link: Relink::new(conn, open)
                        .named(light.id.as_str())
                        .on_change(Arc::clone(&changed)),
                });
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

/// Called when a loop ends by itself, set once at launch.
static ON_END: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// What to do when a loop ends without being stopped. It runs on the loop's
/// own thread, which must not wait on itself: anything that looks at the
/// running loops belongs on another thread.
pub fn on_end(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_END.set(Box::new(f));
}

/// Called when a light's bulb drops or comes back, set once at launch.
static ON_LINKS: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// What to do when a driven light's bulb drops or comes back (see
/// [`Handle::reconnecting`]). It runs on the frame loop, so it must return
/// at once: anything slower belongs on another thread.
pub fn on_links(f: impl Fn() + Send + Sync + 'static) {
    let _ = ON_LINKS.set(Box::new(f));
}

fn links_changed() {
    if let Some(changed) = ON_LINKS.get() {
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
    let done = Arc::new(AtomicBool::new(false));
    let connected_lights = lights.len();
    let links = lights
        .iter()
        .map(|l| (l.id.clone(), l.link.up_flag()))
        .collect();
    let thread = {
        let live = Arc::clone(&live);
        let stop = Arc::clone(&stop);
        let settle = Arc::clone(&settle);
        let done = Arc::clone(&done);
        std::thread::Builder::new()
            .name(format!("lueur-{}", mode.id()))
            .spawn(move || {
                let mut lights = lights;
                body(&mut lights, &live, &stop);
                if settle.load(Ordering::SeqCst) {
                    rest(&mut lights);
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
        done,
        live,
        thread: Some(thread),
        connected_lights,
        light_ids,
        links,
        mode,
        hearing: None,
    })
}

/// Start `mode` on the lights `ids` (see [`connect`] for `hosts`).
///
/// Returns an error if none of them has a bulb that accepts a stream, if
/// capture can't start (ambilight) or if there is nothing to listen to
/// (music).
pub fn start(
    config: &AppConfig,
    mode: Mode,
    ids: &[String],
    hosts: &HashMap<String, usize>,
) -> AppResult<Handle> {
    match mode {
        Mode::Ambilight => start_ambilight(config, ids, hosts),
        Mode::Show(kind) => start_show(config, kind, ids, hosts),
    }
}

fn start_ambilight(
    config: &AppConfig,
    ids: &[String],
    hosts: &HashMap<String, usize>,
) -> AppResult<Handle> {
    // Validate the monitor choice before touching the network, so a bad one
    // fails fast without leaving bulbs in music mode. The source itself is
    // opened on the capture thread: its handle is not `Send`.
    let monitor_index = config.capture.monitor_index;
    check_monitor_index(monitor_index)?;
    let (lights, ids) = connect(config, ids, hosts)?;
    spawn(
        config,
        Mode::Ambilight,
        lights,
        ids,
        move |lights, live, stop| {
            // Opened here, not in `start`: the capture handle stays on this
            // thread for its whole life. A stop asked meanwhile (the user
            // still looking at Wayland's screen sharing dialog) cancels it.
            match ScreenSource::for_index(monitor_index, stop) {
                Ok(source) => run_ambilight(source, lights, live, stop),
                Err(e) => log::error!("capture unavailable: {e}"),
            }
        },
    )
}

fn start_show(
    config: &AppConfig,
    kind: ShowKind,
    ids: &[String],
    hosts: &HashMap<String, usize>,
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
    let (lights, ids) = connect(config, ids, hosts)?;
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

/// Every loop running, at most one per mode.
#[derive(Default)]
pub struct Effects {
    running: Mutex<Vec<Handle>>,
    /// Held across a whole change (stop, connect, start), so two changes
    /// asked at once cannot both plan from the same lights.
    change: Mutex<()>,
}

impl Effects {
    fn lock(&self) -> MutexGuard<'_, Vec<Handle>> {
        // A loop that ended by itself (no capture, no sound) is forgotten here,
        // once it has settled the bulbs: dropping it then waits for nothing.
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        running.retain(Handle::is_running);
        running
    }

    /// Serialise a change to what runs. Not re-entrant: callers holding it
    /// use the functions that take it as given.
    pub fn change(&self) -> MutexGuard<'_, ()> {
        self.change.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_running(&self) -> bool {
        !self.lock().is_empty()
    }

    /// What runs: each mode, the lights it drives and how many answered.
    pub fn groups(&self) -> Vec<(Mode, Vec<String>, usize)> {
        self.lock()
            .iter()
            .map(|h| (h.mode, h.light_ids.clone(), h.connected_lights))
            .collect()
    }

    pub fn modes(&self) -> Vec<Mode> {
        self.lock().iter().map(|h| h.mode).collect()
    }

    /// The mode driving a light, if any.
    pub fn mode_of(&self, light_id: &str) -> Option<Mode> {
        self.lock()
            .iter()
            .find(|h| h.light_ids.iter().any(|id| id == light_id))
            .map(|h| h.mode)
    }

    /// Every room light some loop drives.
    pub fn driven_ids(&self) -> Vec<String> {
        self.lock()
            .iter()
            .flat_map(|h| h.light_ids.iter().cloned())
            .collect()
    }

    /// Whether the music show waits for its source to come back.
    pub fn waiting_for_sound(&self) -> bool {
        self.lock().iter().any(Handle::waiting_for_sound)
    }

    /// Every driven light whose bulb dropped and is being reconnected.
    pub fn reconnecting_ids(&self) -> Vec<String> {
        self.lock()
            .iter()
            .flat_map(|h| h.reconnecting().cloned())
            .collect()
    }

    /// Take the loops `which` picks out of the running set, still running.
    pub fn take(&self, which: impl Fn(&Handle) -> bool) -> Vec<Handle> {
        let mut running = self.lock();
        let (taken, kept) = running.drain(..).partition(|h| which(h));
        *running = kept;
        taken
    }

    /// Add a loop. One already playing its mode is replaced.
    pub fn push(&self, handle: Handle) {
        let mut running = self.lock();
        running.retain(|h| h.mode != handle.mode);
        running.push(handle);
    }

    pub fn update_live(&self, settings: LiveSettings) {
        for h in self.lock().iter() {
            h.update_live(settings.clone());
        }
    }
}

/// Push one frame to every light, corrected for the walls, each with the
/// transition `pace` gives it.
fn push_all(
    lights: &mut [ConnectedLight],
    outputs: &[BulbOutput],
    wall: Rgb,
    pace: impl Fn(usize) -> (Effect, u32),
) {
    for (i, light) in lights.iter_mut().enumerate() {
        let Some(&output) = outputs.get(i) else {
            continue;
        };
        // A bulb that drops is reconnected in the background, without
        // holding up the others, and picks up the frame of the moment.
        let (effect, duration_ms) = pace(i);
        light.link.push(output.on_wall(wall), effect, duration_ms);
    }
}

/// On stop, fade the bulbs to a gentle warm glow and leave them on, so the
/// room stays cosy rather than snapping to black. Brightness has to be set
/// explicitly: the effect may have ended on a dark frame, and the bulb would
/// otherwise hold that 1-2% brightness and look off. Every bulb at once, so
/// they settle together.
fn rest(lights: &mut [ConnectedLight]) {
    std::thread::scope(|scope| {
        for light in lights.iter_mut() {
            scope.spawn(move || light.link.rest());
        }
    });
}

fn run_show(
    kind: ShowKind,
    hearing: Option<Arc<AtomicBool>>,
    lights: &mut [ConnectedLight],
    live: &Mutex<LiveSettings>,
    stop: &AtomicBool,
) {
    let fixtures: Vec<Fixture> = lights.iter().map(|l| l.fixture).collect();
    let mut show = Show::new(kind, &fixtures);
    let mut guard = FlashGuard::show(fixtures.len());
    let mut ears = match hearing {
        Some(hearing) => {
            let Ok(source) = live.lock().map(|g| g.audio.clone()) else {
                return;
            };
            Some(Listener::new(source, hearing))
        }
        None => None,
    };
    // Most outputs run at 48 kHz; a tap that runs at another rate gets an
    // analyser of its own when it opens.
    let mut sample_rate = 48_000;
    let mut analyzer = Analyzer::new(sample_rate);
    let frame = Duration::from_millis(kind.frame_ms());
    // Each frame fades into the next over the time it has, so the bulbs glide
    // rather than step; a light a beat just struck is the exception, sent
    // sharp.
    let glide = pace(Effect::Smooth, frame.as_millis() as u32, frame);
    // Room for the tap's whole second, so the audio thread never grows it.
    let mut samples = Vec::with_capacity(if ears.is_some() {
        sample_rate as usize
    } else {
        0
    });

    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(frame.saturating_sub(last.elapsed()));
        // A stall (a sleeping laptop, a busy machine) must not fast-forward
        // the show.
        let dt = last.elapsed().as_secs_f32().min(0.25);
        last = Instant::now();
        let Ok((settings, wall, safety)) = live.lock().map(|g| {
            if let Some(ears) = &mut ears {
                ears.follow(&g.audio);
            }
            (g.show.clone(), g.wall, g.safety)
        }) else {
            break;
        };
        let levels = match &mut ears {
            Some(ears) => match ears.take_into(&mut samples) {
                Some(rate) => {
                    // A source opened again may run at another rate.
                    if rate != sample_rate {
                        sample_rate = rate;
                        analyzer = Analyzer::new(rate);
                    }
                    analyzer.feed(&samples)
                }
                None => Levels::default(),
            },
            None => Levels::default(),
        };
        let mut outputs = show.frame(&settings, dt, levels);
        guard.apply(&mut outputs, dt, safety.flash_guard);
        let sharp = show.sharp();
        // A hit the guard held back glides in like the rest.
        let pace = |i: usize| {
            if sharp.get(i).copied().unwrap_or(false) && !guard.held(i) {
                (Effect::Sudden, 0)
            } else {
                glide
            }
        };
        push_all(lights, &outputs, wall, pace);
    }
}

/// The music show's ears: a tap on the source the settings name, opened again
/// when they name another one, and when it stops hearing its own (a device
/// unplugged, the application closed), until that comes back.
struct Listener {
    source: AudioSource,
    tap: Option<AudioTap>,
    /// When to try opening the source again, while it has no tap.
    retry: Instant,
    /// Whether it has a tap, for the window to tell.
    hearing: Arc<AtomicBool>,
}

/// How long a source that could not be opened waits before the next try.
const LISTEN_RETRY: Duration = Duration::from_secs(2);

impl Listener {
    /// Opened on the first [`Listener::take_into`], on the show's thread.
    fn new(source: AudioSource, hearing: Arc<AtomicBool>) -> Self {
        Listener {
            source,
            tap: None,
            retry: Instant::now(),
            hearing,
        }
    }

    /// Listen to `source` from now on, if it is not already the one.
    fn follow(&mut self, source: &AudioSource) {
        if *source != self.source {
            self.source = source.clone();
            self.tap = None;
            self.retry = Instant::now();
        }
    }

    /// The sound captured since the last call, into `out`, and its sample
    /// rate. `None` while the source cannot be heard.
    fn take_into(&mut self, out: &mut Vec<f32>) -> Option<u32> {
        if self.tap.as_mut().is_some_and(AudioTap::lost) {
            log::info!("audio source lost, listening again");
            self.tap = None;
            self.retry = Instant::now() + LISTEN_RETRY;
        }
        if self.tap.is_none() && Instant::now() >= self.retry {
            match AudioTap::open(&self.source) {
                Ok(tap) => self.tap = Some(tap),
                Err(e) => {
                    log::debug!("audio unavailable: {e}");
                    self.retry = Instant::now() + LISTEN_RETRY;
                }
            }
        }
        self.hearing.store(self.tap.is_some(), Ordering::Relaxed);
        let tap = self.tap.as_mut()?;
        tap.take_into(out);
        Some(tap.sample_rate)
    }
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
fn pace(effect: Effect, duration_ms: u32, frame_budget: Duration) -> (Effect, u32) {
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
fn scaled_step(step: u32, width: u32) -> u32 {
    (u64::from(step) * u64::from(width) / 1920).max(1) as u32
}

fn run_ambilight(
    mut source: ScreenSource,
    lights: &mut [ConnectedLight],
    live: &Mutex<LiveSettings>,
    stop: &AtomicBool,
) {
    let Ok(initial) = live.lock().map(|g| g.clone()) else {
        return;
    };
    let mut engine = Ambilight::new(lights.len(), initial.engine);
    let mut guard = FlashGuard::ambilight(lights.len());
    let mappings: Vec<LightMapping> = lights.iter().map(|l| l.mapping).collect();
    let mut last_tick: Option<Instant> = None;
    let mut last_frame: Option<Instant> = None;

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

        match source.grab(frame_budget) {
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
                let mut outputs = engine.process_outputs(frame, &mappings);
                guard.apply(&mut outputs, dt, settings.safety.flash_guard);
                let wall = settings.wall;
                push_all(lights, &outputs, wall, |_| (effect, duration_ms));
            }
            Err(_) => {
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
    fn the_frame_rate_follows_the_live_settings() {
        let mut config = AppConfig::default();
        config.capture.target_fps = 60;
        assert_eq!(LiveSettings::of(&config).frame_budget(), fps(60));
        config.capture.target_fps = 30;
        assert_eq!(LiveSettings::of(&config).frame_budget(), fps(30));
        // Out-of-range rates from a hand-edited config are kept sane.
        assert_eq!(frame_budget(0), fps(1));
        assert_eq!(frame_budget(1000), fps(120));
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
        // The reported bug: 300 ms fades re-issued every frame. At 30 fps
        // each one gets the 33 ms until the next.
        assert_eq!(pace(Effect::Smooth, 300, fps(30)), (Effect::Smooth, 33));
    }

    #[test]
    fn a_fast_frame_rate_still_glides() {
        // Dropping to Sudden here is what made every family step at the
        // default 45 fps: the fade they should stretch never reached them.
        assert_eq!(pace(Effect::Smooth, 300, fps(60)), (Effect::Smooth, 30));
        assert_eq!(pace(Effect::Smooth, 80, fps(45)), (Effect::Smooth, 30));
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
            assert_eq!(effect, Effect::Smooth, "{f} fps");
            assert!(
                ms >= lueur_core::yeelight::MIN_SMOOTH_DURATION_MS,
                "{f} fps produced a {ms} ms smooth"
            );
        }
    }
}
