//! Everyday light control, shared by the window and the tray: start and stop
//! the ambilight, switch lights, set them by hand, apply a scene.
//!
//! Everything here blocks on the network; callers run it off the UI thread.

use crate::devices;
use crate::error::{AppError, AppResult};
use crate::runner::{self, Mode};
use crate::state::AppState;
use lueur_core::config::DeviceBinding;
use lueur_core::device::{Endpoint, common};
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

pub fn unreachable(ip: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new("bulb_unreachable").subject(ip).detail(e)
}

/// Reject anything that isn't a bulb's address: a bare private or link-local
/// IPv4 (see [`common::ipv4`]). A command, or a config file someone shared,
/// can never point Lueur at a hostname, the internet or this machine.
pub fn validate_ip(ip: &str) -> AppResult<()> {
    common::ipv4(ip)
        .map(|_| ())
        .map_err(|_| AppError::new("invalid_ip").subject(ip))
}

/// Flip one bulb's power, whatever its family.
pub fn power_one(ep: &Endpoint, on: bool) -> AppResult<()> {
    validate_ip(&ep.ip)?;
    devices::set_power(ep, on).map_err(|e| unreachable(&ep.ip, e))
}

/// Start the ambilight on the whole room, replacing whatever runs.
pub fn start_effect(state: &AppState) -> AppResult<usize> {
    start(state, Mode::Ambilight)
}

/// Start the ambilight or a show on every enabled light, from the saved
/// config, replacing whatever runs. Lights held by hand are let go first: the
/// effect needs their streams. A show started this way becomes the one the
/// tray offers.
pub fn start(state: &AppState, mode: Mode) -> AppResult<usize> {
    let _change = state.effects.change();
    stop_taken(state, state.effects.take(|_| true), false);
    state.manual.release_all();
    let config = {
        let mut cfg = state.lock_config()?;
        if let Mode::Show(kind) = mode {
            cfg.show.kind = kind;
        }
        cfg.clone()
    };
    if matches!(mode, Mode::Show(_)) {
        state.persist()?;
    }
    let ids = runner::enabled_ids(&config);
    let handle = runner::start(&config, mode, &ids, &runner::per_host(&config, &ids))?;
    let connected = handle.connected_lights;
    state.effects.push(handle);
    Ok(connected)
}

/// Stop every effect. Their bulbs settle on a warm glow; with `restore`,
/// those that had been set by hand go back to that instead, so the room is
/// as it was before the effects took over.
pub fn stop_effect(state: &AppState, restore: bool) {
    let _change = state.effects.change();
    stop_taken(state, state.effects.take(|_| true), restore);
}

/// Stop the effect playing `mode`, leaving the others to play on.
pub fn stop_mode(state: &AppState, mode: Mode, restore: bool) {
    let _change = state.effects.change();
    stop_taken(state, state.effects.take(|h| h.mode == mode), restore);
}

fn stop_taken(state: &AppState, mut handles: Vec<runner::Handle>, restore: bool) {
    // Every loop at once: each one's bulbs fade out together.
    std::thread::scope(|scope| {
        for h in &mut handles {
            scope.spawn(move || h.stop());
        }
    });
    if restore {
        let ids: Vec<String> = handles.iter().flat_map(|h| h.light_ids.clone()).collect();
        put_back(state, &ids, false);
    }
}

/// Show on each of `ids` what it was last set to by hand. A light never set
/// is left as it is, or, with `or_default`, given a warm white.
fn put_back(state: &AppState, ids: &[String], or_default: bool) {
    let Ok(cfg) = state.lock_config() else {
        return;
    };
    let jobs: Vec<(DeviceBinding, LightState)> = ids
        .iter()
        .filter_map(|id| {
            let saved = cfg.states.get(id).copied();
            let wish = if or_default {
                Some(saved.unwrap_or_default())
            } else {
                saved
            }?;
            Some((cfg.binding_for(id)?.clone(), wish))
        })
        .collect();
    drop(cfg);
    std::thread::scope(|scope| {
        for (b, wish) in &jobs {
            scope.spawn(move || apply_one(state, b, *wish));
        }
    });
}

/// Play `mode` on one light, or give it back to the hand with `None`.
///
/// Lights playing the same mode share one loop, so they move in step: the
/// light joins that loop, which restarts with it, and the one it leaves
/// restarts without it. Loops driving a light behind the same host restart
/// too, so a Hue bridge's budget is split anew. The bulbs go straight from
/// one loop to the next, without settling in between.
pub fn set_light_mode(state: &AppState, light_id: &str, mode: Option<Mode>) -> AppResult<()> {
    let _change = state.effects.change();
    let binding = binding(state, light_id)?;
    let from = state.effects.mode_of(light_id);
    if from == mode {
        return Ok(());
    }
    let config = state.lock_config()?.clone();
    let same_host = |id: &String| config.binding_for(id).is_some_and(|b| b.ip == binding.ip);
    let taken = state.effects.take(|h| {
        Some(h.mode) == from || Some(h.mode) == mode || h.light_ids.iter().any(same_host)
    });

    let mut plan: Vec<(Mode, Vec<String>)> = taken
        .iter()
        .map(|h| {
            let ids = h.light_ids.iter().filter(|id| *id != light_id).cloned();
            (h.mode, ids.collect())
        })
        .collect();
    if let Some(m) = mode {
        match plan.iter_mut().find(|(pm, _)| *pm == m) {
            Some((_, ids)) => ids.push(light_id.to_string()),
            None => plan.push((m, vec![light_id.to_string()])),
        }
        state.manual.release(light_id);
    }
    plan.retain(|(_, ids)| !ids.is_empty());
    hand_over(taken);

    let (mut idle, errors) = launch(state, &config, &plan);
    if mode.is_none() {
        idle.push(light_id.to_string());
    }
    put_back(state, &idle, true);
    if state.effects.mode_of(light_id) == mode {
        return Ok(());
    }
    // The light did not take its new mode: say why.
    Err(errors
        .into_iter()
        .find(|(m, _)| Some(*m) == mode)
        .map(|(_, e)| e)
        .unwrap_or_else(|| unreachable(&binding.ip, "no stream")))
}

/// Restart every effect, so it picks up a changed room, bulb or capture.
/// Each keeps the lights it drives; an effect playing on the whole room
/// takes every enabled light again, those added since included.
pub fn restart(state: &AppState) -> AppResult<usize> {
    let _change = state.effects.change();
    let config = state.lock_config()?.clone();
    let taken = state.effects.take(|_| true);
    let plan: Vec<(Mode, Vec<String>)> = if let [only] = taken.as_slice() {
        vec![(only.mode, runner::enabled_ids(&config))]
    } else {
        taken
            .iter()
            .map(|h| {
                let ids = h
                    .light_ids
                    .iter()
                    .filter(|id| config.binding_for(id).is_some());
                (h.mode, ids.cloned().collect())
            })
            .collect()
    };
    hand_over(taken);
    let (idle, errors) = launch(state, &config, &plan);
    put_back(state, &idle, true);
    match errors.into_iter().next() {
        Some((_, e)) if !state.effects.is_running() => Err(e),
        _ => Ok(state.effects.groups().iter().map(|g| g.2).sum()),
    }
}

/// Stop loops whose lights something else takes over at once.
fn hand_over(mut handles: Vec<runner::Handle>) {
    std::thread::scope(|scope| {
        for h in &mut handles {
            scope.spawn(move || h.hand_over());
        }
    });
}

/// Start every loop of `plan` at once, alongside those still running.
/// Returns the lights left without one (their bulb did not answer, or their
/// loop failed to start) and why each failed loop did.
fn launch(
    state: &AppState,
    config: &lueur_core::config::AppConfig,
    plan: &[(Mode, Vec<String>)],
) -> (Vec<String>, Vec<(Mode, AppError)>) {
    let still = state.effects.driven_ids();
    let hosts = runner::per_host(
        config,
        still.iter().chain(plan.iter().flat_map(|(_, ids)| ids)),
    );
    let started: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = plan
            .iter()
            .map(|(m, ids)| scope.spawn(|| runner::start(config, *m, ids, &hosts)))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(AppError::internal("effect thread panicked")))
            })
            .collect()
    });
    let mut idle = Vec::new();
    let mut errors = Vec::new();
    for ((m, ids), result) in plan.iter().zip(started) {
        match result {
            Ok(handle) => {
                idle.extend(
                    ids.iter()
                        .filter(|id| !handle.light_ids.contains(id))
                        .cloned(),
                );
                state.effects.push(handle);
            }
            Err(e) => {
                idle.extend(ids.iter().cloned());
                errors.push((*m, e));
            }
        }
    }
    (idle, errors)
}

/// The light's binding, refusing a light an effect drives: its next frame
/// would undo whatever was asked.
fn manual_binding(state: &AppState, light_id: &str) -> AppResult<DeviceBinding> {
    if state.effects.mode_of(light_id).is_some() {
        return Err(AppError::new("light_following"));
    }
    binding(state, light_id)
}

fn binding(state: &AppState, light_id: &str) -> AppResult<DeviceBinding> {
    let cfg = state.lock_config()?;
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
        let cfg = state.lock_config()?;
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
/// about the whole room, so whatever effect runs stops first. The natural
/// light shows what suits the time now, and keeps following the day (see
/// [`follow_natural`]).
pub fn apply_scene(state: &AppState, id: &str) -> AppResult<PowerResult> {
    let wish = if id == scene::NATURAL {
        crate::natural::now()
    } else {
        scene::mood(id)
            .ok_or_else(|| AppError::new("scene_missing"))?
            .state()
    };
    crate::auto::user_took_over(state);
    stop_effect(state, false);
    set_natural(state, id == scene::NATURAL)?;
    apply_many(state, |_| Some(wish), false)
}

/// Turn the natural light on or off, without touching the lights.
pub fn set_natural(state: &AppState, on: bool) -> AppResult<()> {
    let mut cfg = state.lock_config()?;
    if cfg.natural == on {
        return Ok(());
    }
    cfg.natural = on;
    drop(cfg);
    state.persist()
}

/// Bring every light that is on, and free, to what the natural light shows
/// now. Lights already there are left alone, so this is cheap to call
/// often. `true` if any light was changed.
pub fn follow_natural(state: &AppState, wish: LightState) -> AppResult<bool> {
    let driven = state.effects.driven_ids();
    let behind: Vec<String> = {
        let cfg = state.lock_config()?;
        if !cfg.natural {
            return Ok(false);
        }
        cfg.bindings
            .iter()
            .map(|b| &b.light_id)
            .filter(|id| {
                let now = cfg.states.get(*id).copied().unwrap_or_default();
                now.on && now != wish && !driven.contains(id)
            })
            .cloned()
            .collect()
    };
    if behind.is_empty() {
        return Ok(false);
    }
    apply_many(
        state,
        |b| behind.contains(&b.light_id).then_some(wish),
        false,
    )?;
    Ok(true)
}

/// Record what lights were set to, and persist it.
fn remember(state: &AppState, f: impl FnOnce(&mut BTreeMap<String, LightState>)) -> AppResult<()> {
    f(&mut state.lock_config()?.states);
    state.persist()
}
