//! The sleep timer: after a set time, the room fades out and every light goes
//! off, whatever was running.
//!
//! The fade is the timer's last minute: each light free to be set by hand
//! dims from what it shows down to its lowest, then everything is switched
//! off. Cancelling during the fade puts the lights back as they were.

use crate::error::{AppError, AppResult};
use crate::lights;
use crate::state::AppState;
use crate::tray;
use lueur_core::scene::LightState;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Event telling the frontend the timer was set, cancelled or ran out. The
/// payload is its [`Status`], or null.
pub const SLEEP_CHANGED: &str = "sleep-changed";

/// The longest timer the interface may ask for, in minutes.
pub const MAX_MINUTES: u32 = 240;
/// How long the lights take to fade out, at the end of the timer.
const FADE: Duration = Duration::from_secs(60);
/// How many brightness steps the fade takes.
const FADE_STEPS: u32 = 30;
/// How often the waiting worker checks whether it was cancelled.
const POLL: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Inner {
    /// Bumped on every set or cancel, so a worker can tell it is stale.
    generation: u64,
    deadline: Option<Instant>,
    /// What the timer was set to, for the tray to tick.
    minutes: u32,
    fading: bool,
}

/// The timer, if one is set.
#[derive(Default)]
pub struct Timer(Mutex<Inner>);

impl Timer {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn current(&self, generation: u64) -> bool {
        self.lock().generation == generation
    }
}

/// A running timer, as the interface shows it.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Status {
    /// What it was set to.
    pub minutes: u32,
    /// Seconds until the lights go off.
    pub left: u64,
}

pub fn status(state: &AppState) -> Option<Status> {
    Some(Status {
        minutes: minutes(state)?,
        left: remaining(state)?,
    })
}

/// Seconds until the lights go off, if a timer is set.
fn remaining(state: &AppState) -> Option<u64> {
    let t = state.sleep.lock();
    let left = t.deadline?.saturating_duration_since(Instant::now());
    Some(left.as_secs() + u64::from(left.subsec_nanos() > 0))
}

/// What the running timer was set to, in minutes.
pub fn minutes(state: &AppState) -> Option<u32> {
    let t = state.sleep.lock();
    t.deadline.map(|_| t.minutes)
}

/// Whether the lights are fading out right now.
pub fn fading(state: &AppState) -> bool {
    state.sleep.lock().fading
}

/// Set the timer to go off in `minutes` (replacing any other), or cancel it
/// with `None`.
pub fn set(app: &AppHandle, minutes: Option<u32>) -> AppResult<Option<Status>> {
    if minutes.is_some_and(|m| m == 0 || m > MAX_MINUTES) {
        return Err(AppError::new("invalid_timer"));
    }
    let state = app.state::<AppState>();
    let generation = {
        let mut t = state.sleep.lock();
        t.generation += 1;
        t.deadline = minutes.map(|m| Instant::now() + Duration::from_secs(u64::from(m) * 60));
        t.minutes = minutes.unwrap_or(0);
        t.fading = false;
        t.generation
    };
    if minutes.is_some() {
        let handle = app.clone();
        std::thread::Builder::new()
            .name("lueur-sleep".into())
            .spawn(move || run(&handle, generation))
            .map_err(AppError::internal)?;
    }
    changed(app);
    Ok(status(&state))
}

fn run(app: &AppHandle, generation: u64) {
    let state = app.state::<AppState>();
    loop {
        if !state.sleep.current(generation) {
            return;
        }
        if remaining(&state).is_none_or(|left| left <= FADE.as_secs()) {
            break;
        }
        std::thread::sleep(POLL);
    }

    state.sleep.lock().fading = true;
    crate::auto::user_took_over(&state);
    lights::stop_effect(&state, false);
    changed(app);

    if fade(&state, generation) {
        let _ = lights::set_all_power(&state, false);
        let mut t = state.sleep.lock();
        if t.generation == generation {
            *t = Inner {
                generation,
                ..Inner::default()
            };
        }
    }
    let _ = app.emit(tray::EFFECT_CHANGED, None::<AppError>);
    let _ = app.emit(tray::LIGHTS_CHANGED, ());
    changed(app);
}

/// Dim every light that is on down to its lowest. `false` if the timer was
/// cancelled on the way, in which case the lights are put back.
fn fade(state: &AppState, generation: u64) -> bool {
    let lit: Vec<(String, LightState)> = match state.lock_config() {
        Ok(cfg) => cfg
            .bindings
            .iter()
            .map(|b| {
                let s = cfg.states.get(&b.light_id).copied().unwrap_or_default();
                (b.light_id.clone(), s)
            })
            .filter(|(_, s)| s.on)
            .collect(),
        Err(_) => return true,
    };
    let show = |dim: &dyn Fn(LightState) -> LightState| {
        std::thread::scope(|scope| {
            for (id, s) in &lit {
                let wish = dim(*s);
                // A light that does not answer is switched off with the rest.
                scope.spawn(move || lights::set_light(state, id, wish));
            }
        });
    };
    for step in 1..=FADE_STEPS {
        if !state.sleep.current(generation) {
            show(&|s| s);
            return false;
        }
        let left = 1.0 - step as f32 / FADE_STEPS as f32;
        show(&|s| LightState {
            bright: (f32::from(s.bright) * left).round().max(1.0) as u8,
            ..s
        });
        std::thread::sleep(FADE / FADE_STEPS);
    }
    if state.sleep.current(generation) {
        return true;
    }
    show(&|s| s);
    false
}

fn changed(app: &AppHandle) {
    let _ = app.emit(SLEEP_CHANGED, status(&app.state::<AppState>()));
    tray::refresh(app);
}
