//! Everyday light control, shared by the window and the tray: start and stop
//! the ambilight, switch lights, set them by hand, apply a scene.
//!
//! Everything here blocks on the network; callers run it off the UI thread.

use crate::devices;
use crate::error::{AppError, AppResult};
use crate::runner::{self, Mode};
use crate::state::AppState;
use lueur_core::config::DeviceBinding;
use lueur_core::device::Endpoint;
use lueur_core::scene::{self, LightState};
use std::collections::BTreeMap;

/// Outcome of a command sent to several lights. Individual bulbs are allowed
/// to fail (one may be unplugged); the UI reports the tally rather than
/// erroring out.
#[derive(serde::Serialize)]
pub struct PowerResult {
    pub total: usize,
    pub succeeded: usize,
    /// Names of the lights whose bulb did not answer.
    pub failures: Vec<String>,
}

pub fn poisoned() -> AppError {
    AppError::internal("lock poisoned")
}

pub fn unreachable(ip: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new("bulb_unreachable").subject(ip).detail(e)
}

/// Reject anything that isn't a bare IPv4/IPv6 address, so a command can never
/// be tricked into connecting somewhere unexpected via a hostname or URL.
pub fn validate_ip(ip: &str) -> AppResult<()> {
    ip.parse::<std::net::IpAddr>()
        .map(|_| ())
        .map_err(|_| AppError::new("invalid_ip").subject(ip))
}

/// Flip one bulb's power, whatever its family.
pub fn power_one(ep: &Endpoint, on: bool) -> AppResult<()> {
    validate_ip(&ep.ip)?;
    devices::set_power(ep, on).map_err(|e| unreachable(&ep.ip, e))
}

/// Start the ambilight, replacing whatever runs.
pub fn start_effect(state: &AppState) -> AppResult<usize> {
    start(state, Mode::Ambilight)
}

/// Start the ambilight or a show from the saved config, replacing whatever
/// runs. Lights held by hand are let go first: the effect needs their
/// streams. A show started this way becomes the one the tray offers.
pub fn start(state: &AppState, mode: Mode) -> AppResult<usize> {
    stop_effect(state, false);
    state.manual.release_all();
    let config = {
        let mut cfg = state.config.lock().map_err(|_| poisoned())?;
        if let Mode::Show(kind) = mode {
            cfg.show.kind = kind;
        }
        cfg.clone()
    };
    if matches!(mode, Mode::Show(_)) {
        state.persist()?;
    }
    let handle = match mode {
        Mode::Ambilight => runner::start(&config)?,
        Mode::Show(kind) => runner::start_show(&config, kind)?,
    };
    let connected = handle.connected_lights;
    *state.effect.lock().map_err(|_| poisoned())? = Some(handle);
    Ok(connected)
}

/// Stop the ambilight if it runs. Its bulbs settle on a warm glow; with
/// `restore`, those that had been set by hand go back to that instead, so
/// the room is as it was before the screen took over.
pub fn stop_effect(state: &AppState, restore: bool) {
    let handle = state.effect.lock().ok().and_then(|mut g| g.take());
    let Some(mut handle) = handle else {
        return;
    };
    handle.stop();
    if !restore {
        return;
    }
    let jobs: Vec<(DeviceBinding, LightState)> = match state.config.lock() {
        Ok(cfg) => handle
            .light_ids
            .iter()
            .filter_map(|id| Some((cfg.binding_for(id)?.clone(), *cfg.states.get(id)?)))
            .collect(),
        Err(_) => return,
    };
    std::thread::scope(|scope| {
        for (b, wish) in &jobs {
            scope.spawn(move || apply_one(state, b, *wish));
        }
    });
}

/// The light's binding, refusing a light the running ambilight drives: its
/// next frame would undo whatever was asked.
fn manual_binding(state: &AppState, light_id: &str) -> AppResult<DeviceBinding> {
    let driven = state
        .effect
        .lock()
        .map_err(|_| poisoned())?
        .as_ref()
        .is_some_and(|h| h.is_running() && h.light_ids.iter().any(|id| id == light_id));
    if driven {
        return Err(AppError::new("light_following"));
    }
    binding(state, light_id)
}

fn binding(state: &AppState, light_id: &str) -> AppResult<DeviceBinding> {
    let cfg = state.config.lock().map_err(|_| poisoned())?;
    let binding = cfg
        .binding_for(light_id)
        .cloned()
        .ok_or_else(|| AppError::new("light_unbound"))?;
    validate_ip(&binding.ip)?;
    Ok(binding)
}

fn apply_one(state: &AppState, binding: &DeviceBinding, wish: LightState) -> AppResult<()> {
    let ep = binding.endpoint();
    if wish.on {
        state
            .manual
            .set(&binding.light_id, &ep, wish.output())
            .map_err(|e| unreachable(&ep.ip, e))
    } else {
        state.manual.release(&binding.light_id);
        power_one(&ep, false)
    }
}

/// Show `wish` on one light.
pub fn set_light(state: &AppState, light_id: &str, wish: LightState) -> AppResult<()> {
    let binding = manual_binding(state, light_id)?;
    apply_one(state, &binding, wish.clamped())
}

/// Switch one light on or off, leaving its colour to the bulb. Allowed while
/// the ambilight drives it: a bulb switched off stays off, the stream only
/// sets colours.
pub fn set_light_power(state: &AppState, light_id: &str, on: bool) -> AppResult<()> {
    let binding = binding(state, light_id)?;
    state.manual.release(light_id);
    power_one(&binding.endpoint(), on)?;
    remember(state, |states| {
        states.entry(light_id.to_string()).or_default().on = on;
    })
}

/// Every bound bulb with what `wish` says it should show (lights it skips
/// are left alone), all at once.
fn apply_many(
    state: &AppState,
    wish: impl Fn(&DeviceBinding) -> Option<LightState>,
    power_only: bool,
) -> AppResult<PowerResult> {
    // Each bulb with the name of the light it drives, which is what the
    // user calls it.
    let jobs: Vec<(DeviceBinding, String, LightState)> = {
        let cfg = state.config.lock().map_err(|_| poisoned())?;
        cfg.bindings
            .iter()
            .filter_map(|b| {
                let name = cfg
                    .room
                    .lights
                    .iter()
                    .find(|l| l.id == b.light_id)
                    .map(|l| l.name.clone())
                    .unwrap_or_else(|| b.ip.clone());
                Some((b.clone(), name, wish(b)?))
            })
            .collect()
    };
    if jobs.is_empty() {
        return Err(AppError::new("no_bindings"));
    }

    // One thread per bulb: a bulb that is unplugged costs the full timeout, and
    // serialising those would make a handful of dead bulbs feel like a hang.
    let outcomes: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|(b, _, w)| {
                scope.spawn(move || {
                    if power_only {
                        state.manual.release(&b.light_id);
                        power_one(&b.endpoint(), w.on)
                    } else {
                        apply_one(state, b, *w)
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().is_ok_and(|r| r.is_ok()))
            .collect()
    });

    let mut failures = Vec::new();
    remember(state, |states| {
        for ((b, name, w), ok) in jobs.iter().zip(&outcomes) {
            if !ok {
                failures.push(name.clone());
            } else if power_only {
                states.entry(b.light_id.clone()).or_default().on = w.on;
            } else {
                states.insert(b.light_id.clone(), *w);
            }
        }
    })?;
    Ok(PowerResult {
        total: jobs.len(),
        succeeded: jobs.len() - failures.len(),
        failures,
    })
}

/// Turn every bound bulb on or off.
///
/// Turning everything off stops the effect first: the running loop would keep
/// streaming colours (and its shutdown fade would light the bulbs back up),
/// so an "all off" that left it running wouldn't stick.
pub fn set_all_power(state: &AppState, on: bool) -> AppResult<PowerResult> {
    if !on {
        crate::auto::user_took_over(state);
        stop_effect(state, false);
    }
    apply_many(
        state,
        |_| {
            Some(LightState {
                on,
                ..LightState::default()
            })
        },
        true,
    )
}

/// Apply a built-in mood (by its id) to every light. A mood is a decision
/// about the whole room, so whatever effect runs stops first.
pub fn apply_scene(state: &AppState, id: &str) -> AppResult<PowerResult> {
    let mood = scene::mood(id).ok_or_else(|| AppError::new("scene_missing"))?;
    crate::auto::user_took_over(state);
    stop_effect(state, false);
    apply_many(state, |_| Some(mood.state()), false)
}

/// Record what lights were set to, and persist it.
fn remember(state: &AppState, f: impl FnOnce(&mut BTreeMap<String, LightState>)) -> AppResult<()> {
    f(&mut state.config.lock().map_err(|_| poisoned())?.states);
    state.persist()
}
