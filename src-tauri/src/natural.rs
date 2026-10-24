//! The natural light: while it is on, the lights follow the time of day,
//! cool and bright through the day, warm and low in the evening and at night
//! (see `lueur_core::scene::natural_at`).
//!
//! A worker checks every so often and re-sends only the lights that are on,
//! free (not driven by an effect) and behind what the hour calls for. The
//! steps are small enough that each change goes unnoticed.

use crate::lights;
use crate::state::AppState;
use crate::tray;
use chrono::Timelike;
use lueur_core::scene::{self, LightState};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// How often the lights are brought in line with the clock. The curve moves
/// at most a few kelvin a minute, so this is plenty.
const TICK: Duration = Duration::from_secs(30);

/// What the natural light shows at this moment, by the local clock.
pub fn now() -> LightState {
    let t = chrono::Local::now();
    scene::natural_at((t.hour() * 60 + t.minute()) as u16)
}

/// Keep the lights following the day for as long as the app runs.
pub fn spawn(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("lueur-natural".into())
        .spawn(move || {
            loop {
                std::thread::sleep(TICK);
                let state = app.state::<AppState>();
                // The sleep timer is dimming the room: leave it to that.
                if crate::sleep::fading(&state) {
                    continue;
                }
                if lights::follow_natural(&state, now()).unwrap_or(false) {
                    let _ = app.emit(tray::LIGHTS_CHANGED, ());
                }
            }
        });
}
