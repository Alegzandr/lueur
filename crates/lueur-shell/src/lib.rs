//! Lueur's desktop service.
//!
//! Everything the app does between the pure [`lueur_core`] engine and a
//! window: screen capture, the bulbs on the network, the live loops, the
//! secrets, the background workers (automatic mode, the natural light, the
//! sleep timer, the other computers running Lueur) and the commands a window
//! calls. No GUI framework is a dependency: a front end builds a [`Shell`],
//! hands it a [`Host`] that turns [`Event`]s into whatever its window and
//! tray need, and calls [`service`].

// The anti-cheat bans in `clippy.toml` are hard errors, not suggestions.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

pub mod audio;
pub mod auto;
pub mod autostart;
mod awake;
pub mod capture;
pub mod devices;
pub mod error;
pub mod identity;
#[cfg(windows)]
pub mod install;
pub mod lights;
pub mod logging;
pub mod manual;
pub mod natural;
pub mod paths;
pub mod peers;
pub mod runner;
pub mod secrets;
pub mod service;
pub mod sleep;
pub mod state;
pub mod tray_model;
pub mod tuya;
pub mod tuya_cloud;
pub mod vault;

/// The capture benchmark, for `examples/capture_bench.rs` only.
#[doc(hidden)]
pub use capture::bench as capture_bench;

use error::AppError;
use identity::Unverified;
use state::AppState;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// Launch argument the OS autostart passes, so a login launch opens hidden.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Something the window and the tray must hear about, raised by a command, a
/// background worker or the tray itself.
#[derive(Debug, Clone)]
pub enum Event {
    /// The effects changed (started, stopped, a bulb dropped or came back,
    /// another computer took a bulb), with the error the change ran into.
    EffectChanged(Option<AppError>),
    /// What the lights show changed outside the window (the tray, the
    /// natural light, the sleep timer).
    LightsChanged,
    /// Automatic mode started the ambilight for a fullscreen game or video.
    AutoStarted,
    /// The last search heard paired devices nothing vouches for.
    DevicesUnverified(Vec<Unverified>),
    /// The sleep timer was set, cancelled or ran out.
    SleepChanged(Option<sleep::Status>),
    /// A new version is installed and waits for a restart.
    UpdateReady(String),
    /// The tray menu may no longer match what the lights show. `always`
    /// rebuilds it even when nothing it shows changed (its language, say).
    TrayStale { always: bool },
    /// Bring the main window up: the tray asked, or a tray action failed
    /// with no window visible to say so.
    FocusMain,
}

impl Event {
    /// The name the Tauri window listens to (see `src/lib/ipc.ts`).
    pub fn name(&self) -> &'static str {
        match self {
            Event::EffectChanged(_) => "effect-changed",
            Event::LightsChanged => "lights-changed",
            Event::AutoStarted => "auto-started",
            Event::DevicesUnverified(_) => "device-unverified",
            Event::SleepChanged(_) => "sleep-changed",
            Event::UpdateReady(_) => "update-ready",
            Event::TrayStale { .. } => "tray-stale",
            Event::FocusMain => "focus-main",
        }
    }
}

/// What a front end does with the shell's events. Called from any thread.
pub trait Host: Send + Sync + 'static {
    fn emit(&self, event: Event);
}

/// The running app: its state, and the front end it reports to.
pub struct Shell {
    pub state: AppState,
    host: OnceLock<Arc<dyn Host>>,
    /// The version an update installed while the app runs, a restart away.
    update_ready: Mutex<Option<String>>,
}

impl Deref for Shell {
    type Target = AppState;
    fn deref(&self) -> &AppState {
        &self.state
    }
}

impl Shell {
    /// Load the state from `data_dir`. Events raised before [`Shell::attach`]
    /// are dropped: nothing listens yet.
    pub fn load(data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Shell {
            state: AppState::load(data_dir),
            host: OnceLock::new(),
            update_ready: Mutex::new(None),
        })
    }

    /// Plug the front end in. Only the first call counts.
    pub fn attach(&self, host: Arc<dyn Host>) {
        let _ = self.host.set(host);
    }

    pub fn emit(&self, event: Event) {
        if let Some(host) = self.host.get() {
            host.emit(event);
        }
    }

    /// Tell the window that the effects and the lights changed, with the
    /// error the change ran into if any, and bring the tray up to date.
    pub fn lights_changed(&self, error: Option<AppError>) {
        self.emit(Event::EffectChanged(error));
        self.emit(Event::LightsChanged);
        self.tray_refresh();
    }

    /// Rebuild the tray menu.
    pub fn tray_refresh(&self) {
        self.emit(Event::TrayStale { always: true });
    }

    /// Rebuild the tray menu if what it shows changed: cheap enough for a
    /// slider that sets a light many times a second.
    pub fn tray_sync(&self) {
        self.emit(Event::TrayStale { always: false });
    }

    pub fn focus_main(&self) {
        self.emit(Event::FocusMain);
    }

    /// The version installed and waiting for a restart, if any.
    pub fn update_ready(&self) -> Option<String> {
        self.update_ready.lock().ok().and_then(|r| r.clone())
    }

    /// An update was installed while the app runs.
    pub fn set_update_ready(&self, version: String) {
        if let Ok(mut r) = self.update_ready.lock() {
            *r = Some(version.clone());
        }
        self.emit(Event::UpdateReady(version));
        self.tray_refresh();
    }

    /// Start the background workers: the other computers running Lueur, the
    /// effect ending or changing by itself, automatic mode and the natural
    /// light. Call once, after [`Shell::attach`].
    pub fn spawn_workers(self: &Arc<Self>) {
        // Listening for the other computers running Lueur from the first
        // moment, so the effect resumed at launch knows which bulbs they
        // hold.
        peers::spawn(self);
        // An effect that ends by itself (no capture, no sound) still shows
        // as playing until the window and the tray are told.
        let shell = Arc::clone(self);
        runner::on_end(move || {
            let shell = Arc::clone(&shell);
            std::thread::spawn(move || {
                shell.emit(Event::EffectChanged(None));
                shell.tray_refresh();
            });
        });
        // A bulb that dropped or came back, or the ambilight's screen going
        // in or out of HDR: the window shows which lights are being
        // reconnected, and whether the colours come from HDR.
        let shell = Arc::clone(self);
        runner::on_status(move || {
            let shell = Arc::clone(&shell);
            std::thread::spawn(move || shell.emit(Event::EffectChanged(None)));
        });
    }

    /// Once the window is up: automatic mode, the natural light, and each
    /// light picking up the mode it was left playing.
    pub fn start(self: &Arc<Self>) {
        // An install moved the executable: point the login item at the new
        // path.
        autostart::refresh();
        auto::spawn(self);
        natural::spawn(self);
        let shell = Arc::clone(self);
        std::thread::spawn(move || {
            if let Err(e) = lights::resume(&shell) {
                log::warn!("cannot resume the modes the lights were left playing: {e}");
            }
            // The window may have read the state before the bulbs answered.
            shell.lights_changed(None);
        });
    }

    /// However Lueur quits, the bulbs leave music mode rather than freeze on
    /// the last frame, and the other computers hear this one let go of its
    /// bulbs. A no-op when the tray's Quit has done it already.
    pub fn quit(&self) {
        lights::quit(self);
        peers::leave(self);
    }
}
