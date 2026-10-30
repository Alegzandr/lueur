//! Automatic mode: the ambilight starts by itself when a game or a video goes
//! fullscreen, taking over from whatever effect played, and stops when it
//! ends, handing the lights back as they were: the effects that played play
//! again, the lights set by hand show what they were set to.
//!
//! Fullscreen is read from the shell's notification state, the same signal
//! Windows uses to hold notifications during a game or a presentation. That
//! state does not say which screen is fullscreen, so the window in front is
//! located, only to learn which monitor it is on: it is never opened, read or
//! named, and its process is never touched, which keeps to the anti-cheat
//! rules in `clippy.toml`. Other systems have no equivalent: there the mode
//! is off.

use crate::lights;
use crate::runner::Mode;
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

/// Sent to the window when automatic mode started the ambilight.
pub const AUTO_STARTED: &str = "auto-started";

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

/// The monitor the window in front is on, as `xcap` numbers monitors (see
/// [`crate::capture::monitor_id`]).
#[cfg(windows)]
fn front_monitor() -> Option<u32> {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromWindow};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    // SAFETY: both calls only read the window manager's state; the handle is
    // not kept, and nothing is asked of the window itself.
    let monitor = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONULL) };
    (!monitor.is_invalid()).then_some(monitor.0 as usize as u32)
}

#[cfg(not(windows))]
fn front_monitor() -> Option<u32> {
    None
}

/// Whether something is fullscreen on the monitor the ambilight reads, the
/// one at `index`. A video fullscreen on another screen is no reason to light
/// the room after this one. Where the monitors cannot be told apart, any
/// fullscreen counts.
fn fullscreen_on(index: usize) -> bool {
    if !fullscreen() {
        return false;
    }
    match (crate::capture::monitor_id(index), front_monitor()) {
        (Some(ours), Some(front)) => ours == front,
        _ => true,
    }
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

/// What the watcher does after one look at the screen.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Wait,
    /// Start the ambilight, replacing what plays.
    TakeOver,
    /// The session it started for is over: stop it, and play again what
    /// played before.
    GiveBack,
}

/// The step for a poll: whether a fullscreen session is on, automatic mode
/// enabled, who owns what runs, and the modes playing.
fn step(in_session: bool, enabled: bool, owner: Owner, playing: &[Mode]) -> Step {
    if !in_session {
        return if owner == Owner::Auto {
            Step::GiveBack
        } else {
            Step::Wait
        };
    }
    // The user has the ambilight going already: nothing to take over.
    let ambilight_only = !playing.is_empty() && playing.iter().all(|m| *m == Mode::Ambilight);
    if enabled && owner == Owner::Nobody && !ambilight_only {
        Step::TakeOver
    } else {
        Step::Wait
    }
}

fn watch(app: &AppHandle) {
    let mut last_seen: Option<Instant> = None;
    // What played when automatic mode took over, to play again after.
    let mut before: Vec<(Mode, Vec<String>)> = Vec::new();
    loop {
        std::thread::sleep(POLL);
        let state = app.state::<AppState>();
        let (enabled, monitor_index) = state
            .lock_config()
            .map(|c| {
                (
                    c.behavior.auto_fullscreen && !c.bindings.is_empty(),
                    c.capture.monitor_index,
                )
            })
            .unwrap_or((false, 0));

        if fullscreen_on(monitor_index) {
            last_seen = Some(Instant::now());
        }
        let in_session = last_seen.is_some_and(|t| t.elapsed() < GRACE);
        let owner = *state.auto_owner.lock().unwrap_or_else(|p| p.into_inner());

        match step(in_session, enabled, owner, &state.effects.modes()) {
            Step::Wait => {}
            Step::TakeOver => {
                set_owner(&state, Owner::Auto);
                before = lights::playing(&state);
                if lights::start_effect(&state).is_ok() {
                    let _ = app.emit(AUTO_STARTED, ());
                } else {
                    // Nothing to light (no light follows the screen, say):
                    // put back what played, and stay out of the way until
                    // the next fullscreen session.
                    if !before.is_empty() {
                        let before = std::mem::take(&mut before);
                        let _ = lights::give_back(&state, &before, || owns(&state));
                    }
                    set_owner(&state, Owner::User);
                }
                changed(app);
            }
            // Had the user chosen anything meanwhile, they would own it:
            // what they chose is never undone here. Asked again under the
            // change lock, so a choice made just now wins too.
            Step::GiveBack => {
                let _ = lights::give_back(&state, &before, || owns(&state));
                changed(app);
            }
        }
        if !in_session {
            last_seen = None;
            before.clear();
            set_owner(&state, Owner::Nobody);
        }
    }
}

/// Whether automatic mode still owns what runs.
fn owns(state: &AppState) -> bool {
    *state.auto_owner.lock().unwrap_or_else(|p| p.into_inner()) == Owner::Auto
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

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::show::ShowKind;

    const RAINBOW: Mode = Mode::Show(ShowKind::Rainbow);

    #[test]
    fn fullscreen_takes_over_from_an_effect() {
        // The reported bug: Rainbow playing kept the ambilight from starting.
        assert_eq!(step(true, true, Owner::Nobody, &[RAINBOW]), Step::TakeOver);
        assert_eq!(step(true, true, Owner::Nobody, &[]), Step::TakeOver);
        assert_eq!(
            step(true, true, Owner::Nobody, &[Mode::Ambilight, RAINBOW]),
            Step::TakeOver
        );
    }

    #[test]
    fn an_ambilight_already_playing_is_left_alone() {
        assert_eq!(
            step(true, true, Owner::Nobody, &[Mode::Ambilight]),
            Step::Wait
        );
    }

    #[test]
    fn nothing_starts_when_disabled_or_once_owned() {
        assert_eq!(step(true, false, Owner::Nobody, &[RAINBOW]), Step::Wait);
        assert_eq!(
            step(true, true, Owner::Auto, &[Mode::Ambilight]),
            Step::Wait
        );
        // The user chose during the session: theirs until it ends.
        assert_eq!(step(true, true, Owner::User, &[RAINBOW]), Step::Wait);
        assert_eq!(step(true, true, Owner::User, &[]), Step::Wait);
    }

    #[test]
    fn the_end_of_a_session_gives_back_only_what_auto_took() {
        assert_eq!(
            step(false, true, Owner::Auto, &[Mode::Ambilight]),
            Step::GiveBack
        );
        // Even if its ambilight ended early, what played before comes back.
        assert_eq!(step(false, true, Owner::Auto, &[]), Step::GiveBack);
        assert_eq!(step(false, true, Owner::User, &[RAINBOW]), Step::Wait);
        assert_eq!(step(false, true, Owner::Nobody, &[]), Step::Wait);
    }
}
