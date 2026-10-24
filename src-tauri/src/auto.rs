//! Automatic mode: the ambilight starts by itself when a game or a video goes
//! fullscreen, and stops when it ends, handing the lights back as they were.
//!
//! Fullscreen is read from the shell's notification state, the same signal
//! Windows uses to hold notifications during a game or a presentation. It
//! names no window and opens no process, so it keeps to the anti-cheat rules
//! in `clippy.toml`. Other systems have no equivalent: there the mode is off.

use crate::lights;
use crate::state::AppState;
use crate::tray;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// How often fullscreen is checked. A start takes a moment anyway (the bulbs
/// have to be reached), so polling faster would not be noticed.
const POLL: Duration = Duration::from_millis(1000);
/// How long fullscreen must be gone before the effect stops, so a quick
/// alt-tab or a loading screen does not flicker the room.
const GRACE: Duration = Duration::from_secs(4);

/// Whether this system can tell when something is fullscreen.
pub const SUPPORTED: bool = cfg!(windows);

#[cfg(windows)]
fn fullscreen() -> bool {
    use windows::Win32::UI::Shell::{
        QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
        SHQueryUserNotificationState,
    };
    // SAFETY: no arguments; the call only reads the shell's state.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_BUSY || s == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn fullscreen() -> bool {
    false
}

/// Who started the running effect, so automatic mode only ever stops what it
/// started itself.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    #[default]
    Nobody,
    /// Started by automatic mode for the current fullscreen session.
    Auto,
    /// The user started or stopped it during this fullscreen session: leave
    /// it alone until the session ends.
    User,
}

/// Watch for fullscreen for as long as the app runs.
pub fn spawn(app: &AppHandle) {
    if !SUPPORTED {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("lueur-auto".into())
        .spawn(move || watch(&app));
}

fn watch(app: &AppHandle) {
    let mut last_seen: Option<Instant> = None;
    loop {
        std::thread::sleep(POLL);
        let state = app.state::<AppState>();
        let enabled = state
            .lock_config()
            .is_ok_and(|c| c.behavior.auto_fullscreen && !c.bindings.is_empty());

        if fullscreen() {
            last_seen = Some(Instant::now());
        }
        let in_session = last_seen.is_some_and(|t| t.elapsed() < GRACE);
        let owner = *state.auto_owner.lock().unwrap_or_else(|p| p.into_inner());

        if !in_session {
            last_seen = None;
            if owner == Owner::Auto && state.is_running() {
                lights::stop_effect(&state, true);
                changed(app);
            }
            set_owner(&state, Owner::Nobody);
            continue;
        }

        if enabled && owner == Owner::Nobody && !state.is_running() {
            set_owner(&state, Owner::Auto);
            let result = lights::start_effect(&state);
            if result.is_err() {
                // Nothing to light (no light follows the screen, say): stay
                // out of the way until the next fullscreen session.
                set_owner(&state, Owner::User);
            }
            changed(app);
        }
    }
}

fn set_owner(state: &AppState, owner: Owner) {
    *state.auto_owner.lock().unwrap_or_else(|p| p.into_inner()) = owner;
}

/// The user started or stopped the effect: automatic mode leaves it alone
/// until the current fullscreen session (if any) ends.
pub fn user_took_over(state: &AppState) {
    if fullscreen() || state.is_running() {
        set_owner(state, Owner::User);
    }
}

fn changed(app: &AppHandle) {
    let _ = app.emit(tray::EFFECT_CHANGED, None::<crate::error::AppError>);
    let _ = app.emit(tray::LIGHTS_CHANGED, ());
    tray::refresh(app);
}
