//! Everyday light control, shared by the window and the tray: start and stop
//! the ambilight, switch lights, set them by hand, apply a scene.
//!
//! Everything here blocks on the network; callers run it off the UI thread.

use crate::devices;
use crate::error::{AppError, AppResult};
use crate::runner::{self, Mode};
use crate::state::AppState;
use lueur_core::color::Rgb;
use lueur_core::config::{AppConfig, DeviceBinding};
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

/// The bulb at `ip` did not answer. Its address is the subject until
/// [`named`] gives it the name of its light; the detail keeps the address
/// for whoever digs further.
pub fn unreachable(ip: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new("bulb_unreachable")
        .subject(ip)
        .detail(format!("{ip}: {e}"))
}

/// Say which bulb did not answer by the name of the light it drives, which
/// is what the user calls it, rather than by its address. Without a name
/// (a bulb in no light, or a light left unnamed) the address stays.
pub fn named(mut e: AppError, light_name: Option<&str>) -> AppError {
    if e.code == "bulb_unreachable"
        && let Some(name) = light_name.map(str::trim).filter(|n| !n.is_empty())
    {
        e.subject = Some(name.to_string());
    }
    e
}

/// The name of the light `light_id`.
pub fn light_name<'a>(config: &'a AppConfig, light_id: &str) -> Option<&'a str> {
    config
        .room
        .lights
        .iter()
        .find(|l| l.id == light_id)
        .map(|l| l.name.as_str())
}

/// The name of the light the bulb `device_id` drives, if it drives one.
pub fn device_light_name<'a>(config: &'a AppConfig, device_id: &str) -> Option<&'a str> {
    let b = config.bindings.iter().find(|b| b.device_id == device_id)?;
    light_name(config, &b.light_id)
}

/// [`named`] after the light `light_id`, from the config as it is now.
fn named_for(state: &AppState, light_id: &str, e: AppError) -> AppError {
    match state.lock_config() {
        Ok(cfg) => named(e, light_name(&cfg, light_id)),
        Err(_) => e,
    }
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

/// Start the ambilight on the whole room for the app itself (at launch, or
/// for a fullscreen game), replacing whatever runs. The modes the user chose
/// stay recorded, so they come back at the next launch.
pub fn start_effect(state: &AppState) -> AppResult<usize> {
    start_room(state, Mode::Ambilight)
}

/// Start the ambilight or a show on every enabled light, as the user asked,
/// replacing whatever runs, and record it for the next launch.
pub fn start(state: &AppState, mode: Mode) -> AppResult<usize> {
    let connected = start_room(state, mode)?;
    record_modes(state, |modes, cfg| {
        *modes = runner::enabled_ids(cfg)
            .into_iter()
            .map(|id| (id, mode.id().to_string()))
            .collect();
    })?;
    Ok(connected)
}

/// Start `mode` on every enabled light, from the saved config, replacing
/// whatever runs. Lights held by hand are let go first: the effect needs
/// their streams. A show started this way becomes the one the tray offers.
fn start_room(state: &AppState, mode: Mode) -> AppResult<usize> {
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
    let hosts = runner::per_host(&config, &ids);
    let handle = runner::start(&config, mode, &ids, &hosts, &state.leases)?;
    let connected = handle.connected_lights;
    state.effects.push(handle);
    Ok(connected)
}

/// Stop every effect, as the user asked: the next launch starts none. Their
/// bulbs settle on a warm glow; with `restore`, those that had been set by
/// hand go back to that instead, so the room is as it was before the effects
/// took over.
pub fn stop_effect(state: &AppState, restore: bool) {
    suspend(state, restore);
    if let Err(e) = record_modes(state, |modes, _| modes.clear()) {
        log::warn!("cannot save that no light plays an effect: {e}");
    }
}

/// Stop every effect for the app itself (quitting, updating, a fullscreen
/// game that ended), leaving the modes the user chose recorded.
pub fn suspend(state: &AppState, restore: bool) {
    let _change = state.effects.change();
    stop_taken(state, state.effects.take(|_| true), restore);
}

/// Stop every effect because Lueur quits, leaving the modes the user chose
/// recorded: they play again at the next launch. Meanwhile a show goes on
/// on the bulbs that can play it by themselves; the others settle on a warm
/// glow, as with [`suspend`].
pub fn quit(state: &AppState) {
    let _change = state.effects.change();
    let mut handles = state.effects.take(|_| true);
    // Every loop at once, as in `stop_taken`.
    std::thread::scope(|scope| {
        for h in &mut handles {
            scope.spawn(move || h.leave_playing());
        }
    });
}

/// Stop the effect playing `mode`, leaving the others to play on.
pub fn stop_mode(state: &AppState, mode: Mode, restore: bool) {
    {
        let _change = state.effects.change();
        stop_taken(state, state.effects.take(|h| h.mode == mode), restore);
    }
    if let Err(e) = record_modes(state, |modes, _| modes.retain(|_, m| m != mode.id())) {
        log::warn!("cannot save that no light plays {}: {e}", mode.id());
    }
}

/// Change what is recorded of each light's mode, and persist it.
fn record_modes(
    state: &AppState,
    f: impl FnOnce(&mut BTreeMap<String, String>, &lueur_core::config::AppConfig),
) -> AppResult<()> {
    {
        let mut cfg = state.lock_config()?;
        let mut modes = std::mem::take(&mut cfg.modes);
        f(&mut modes, &cfg);
        cfg.modes = modes;
    }
    state.persist()
}

/// Play again what each light played when the user last chose, as recorded
/// in the config: at launch, the room picks up where it was left. Lights
/// that are gone or whose bulb does not answer are skipped, but stay
/// recorded. `Ok(0)` when nothing is recorded.
pub fn resume(state: &AppState) -> AppResult<usize> {
    let _change = state.effects.change();
    let config = state.lock_config()?.clone();
    let mut plan: Vec<(Mode, Vec<String>)> = Vec::new();
    for light in &config.room.lights {
        let Some(mode) = config.modes.get(&light.id).and_then(|m| Mode::from_id(m)) else {
            continue;
        };
        if config.binding_for(&light.id).is_none() {
            continue;
        }
        match plan.iter_mut().find(|(m, _)| *m == mode) {
            Some((_, ids)) => ids.push(light.id.clone()),
            None => plan.push((mode, vec![light.id.clone()])),
        }
    }
    if plan.is_empty() {
        return Ok(0);
    }
    stop_taken(state, state.effects.take(|_| true), false);
    for (_, ids) in &plan {
        for id in ids {
            state.manual.release(id);
        }
    }
    let (_, errors) = launch(state, &config, &plan);
    match errors.into_iter().next() {
        Some((_, e)) if !state.effects.is_running() => Err(e),
        _ => Ok(state.effects.groups().iter().map(|g| g.2).sum()),
    }
}

/// What plays now: each mode and its lights, as [`give_back`] takes them.
pub fn playing(state: &AppState) -> Vec<(Mode, Vec<String>)> {
    state
        .effects
        .groups()
        .into_iter()
        .map(|(mode, ids, _)| (mode, ids))
        .collect()
}

/// Stop what the app started by itself and play `before` again, which is
/// what played when it took over (see [`playing`]). The lights `before`
/// leaves out go back to what they were last set to by hand. Nothing
/// recorded changes: what the app starts by itself is never recorded.
///
/// `still_ours` is asked once no other change can start: if the user chose
/// something in the meantime, nothing is touched.
pub fn give_back(
    state: &AppState,
    before: &[(Mode, Vec<String>)],
    still_ours: impl FnOnce() -> bool,
) -> AppResult<usize> {
    let _change = state.effects.change();
    if !still_ours() {
        return Ok(0);
    }
    if before.is_empty() {
        stop_taken(state, state.effects.take(|_| true), true);
        return Ok(0);
    }
    let config = state.lock_config()?.clone();
    // Lights removed or unbound since are left out.
    let plan: Vec<(Mode, Vec<String>)> = before
        .iter()
        .map(|(mode, ids)| {
            let ids = ids.iter().filter(|id| config.binding_for(id).is_some());
            (*mode, ids.cloned().collect::<Vec<_>>())
        })
        .filter(|(_, ids)| !ids.is_empty())
        .collect();
    let planned = |id: &String| plan.iter().any(|(_, ids)| ids.contains(id));
    let taken = state.effects.take(|_| true);
    let mut idle: Vec<String> = taken
        .iter()
        .flat_map(|h| h.light_ids.iter())
        .filter(|id| !planned(id))
        .cloned()
        .collect();
    hand_over(taken);
    for id in plan.iter().flat_map(|(_, ids)| ids) {
        state.manual.release(id);
    }
    let (missed, errors) = launch(state, &config, &plan);
    idle.extend(missed);
    put_back(state, &idle, true);
    match errors.into_iter().next() {
        Some((_, e)) if !state.effects.is_running() => Err(e),
        _ => Ok(state.effects.groups().iter().map(|g| g.2).sum()),
    }
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
/// is left as it is, or, with `or_default`, given a warm white. So is one
/// whose bulb another computer drives: it shows that computer's effect.
fn put_back(state: &AppState, ids: &[String], or_default: bool) {
    let Ok(cfg) = state.lock_config() else {
        return;
    };
    let wall = cfg.room.wall;
    let jobs: Vec<(DeviceBinding, LightState)> = ids
        .iter()
        .filter_map(|id| {
            let saved = cfg.states.get(id).copied();
            let wish = if or_default {
                Some(saved.unwrap_or_default())
            } else {
                saved
            }?;
            let binding = cfg.binding_for(id)?;
            if state.leases.holder(&binding.device_id).is_some() {
                return None;
            }
            Some((binding.clone(), wish))
        })
        .collect();
    drop(cfg);
    std::thread::scope(|scope| {
        for (b, wish) in &jobs {
            scope.spawn(move || apply_one(state, b, *wish, wall));
        }
    });
}

/// Play `mode` on `ids`, or give them back to the hand with `None`.
///
/// Lights playing the same mode share one loop, so they move in step: the
/// lights join that loop, which restarts with them, and the ones they leave
/// restart without them. Loops driving a light behind the same host restart
/// too, so a Hue bridge's budget is split anew. The bulbs go straight from
/// one loop to the next, without settling in between.
pub fn set_lights_mode(state: &AppState, ids: &[String], mode: Option<Mode>) -> AppResult<()> {
    move_lights(state, ids, mode, true)
}

/// [`set_lights_mode`]. Lights given back to the hand show what they were
/// last set to, unless `settle` is false: something else is about to be
/// shown on them.
fn move_lights(
    state: &AppState,
    ids: &[String],
    mode: Option<Mode>,
    settle: bool,
) -> AppResult<()> {
    let _change = state.effects.change();
    let bindings = ids
        .iter()
        .map(|id| binding(state, id))
        .collect::<AppResult<Vec<_>>>()?;
    let moving: Vec<&String> = ids
        .iter()
        .filter(|id| state.effects.mode_of(id) != mode)
        .collect();
    if moving.is_empty() {
        return Ok(());
    }
    let from: Vec<Option<Mode>> = moving.iter().map(|id| state.effects.mode_of(id)).collect();
    let config = state.lock_config()?.clone();
    let same_host = |id: &String| {
        config
            .binding_for(id)
            .is_some_and(|b| bindings.iter().any(|ours| ours.ip == b.ip))
    };
    let taken = state.effects.take(|h| {
        from.contains(&Some(h.mode)) || Some(h.mode) == mode || h.light_ids.iter().any(same_host)
    });

    let mut plan: Vec<(Mode, Vec<String>)> = taken
        .iter()
        .map(|h| {
            let rest = h
                .light_ids
                .iter()
                .filter(|id| !moving.contains(id))
                .cloned();
            (h.mode, rest.collect())
        })
        .collect();
    if let Some(m) = mode {
        let joining = moving.iter().map(|id| id.to_string());
        match plan.iter_mut().find(|(pm, _)| *pm == m) {
            Some((_, ids)) => ids.extend(joining),
            None => plan.push((m, joining.collect())),
        }
        for id in &moving {
            state.manual.release(id);
        }
    }
    plan.retain(|(_, ids)| !ids.is_empty());
    hand_over(taken);

    let (mut idle, errors) = launch(state, &config, &plan);
    if mode.is_none() && settle {
        idle.extend(moving.iter().map(|id| id.to_string()));
    }
    put_back(state, &idle, true);
    let took: Vec<&String> = moving
        .iter()
        .copied()
        .filter(|id| state.effects.mode_of(id) == mode)
        .collect();
    record_modes(state, |modes, _| {
        for id in &took {
            match mode {
                Some(m) => {
                    modes.insert(id.to_string(), m.id().to_string());
                }
                None => {
                    modes.remove(*id);
                }
            }
        }
    })?;
    if took.len() == moving.len() {
        return Ok(());
    }
    // Some light did not take its new mode: say why.
    let missed = moving.iter().find(|id| !took.contains(id));
    let ip = missed
        .and_then(|id| config.binding_for(id))
        .map_or("", |b| b.ip.as_str());
    let name = missed.and_then(|id| light_name(&config, id));
    Err(errors
        .into_iter()
        .find(|(m, _)| Some(*m) == mode)
        .map(|(_, e)| e)
        .unwrap_or_else(|| named(unreachable(ip, "no stream"), name)))
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

/// Take the bulb of `light_id` from whatever drives it, another computer or
/// another app, as the user asked: the effect playing on the light here
/// drives it again at once, and the other one waits instead. Only a light
/// an effect plays on can be taken, or nothing would drive the bulb it
/// took.
pub fn take(state: &AppState, light_id: &str) -> AppResult<()> {
    if state.effects.mode_of(light_id).is_none() {
        return Err(AppError::new("light_not_playing"));
    }
    let device = binding(state, light_id)?.device_id;
    state.leases.take(&device);
    Ok(())
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
            .map(|(m, ids)| scope.spawn(|| runner::start(config, *m, ids, &hosts, &state.leases)))
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

/// Show `wish` on one light, corrected for the room's `wall`.
fn apply_one(
    state: &AppState,
    binding: &DeviceBinding,
    wish: LightState,
    wall: Rgb,
) -> AppResult<()> {
    let ep = binding.endpoint();
    if wish.on {
        state
            .manual
            .set(&binding.light_id, &ep, wish.output().on_wall(wall))
            .map_err(|e| unreachable(&ep.ip, e))
    } else {
        state.manual.release(&binding.light_id);
        power_one(&ep, false)
    }
}

/// Show `wish` on one light, and record it. A slider drag calls this many
/// times a second, so the record waits for the interface's next save to
/// reach the disk.
///
/// Holds the effects' change lock, so an effect starting meanwhile either
/// sees the light taken by hand or takes it after: never both at once.
pub fn set_light(state: &AppState, light_id: &str, wish: LightState) -> AppResult<()> {
    let _change = state.effects.change();
    let binding = manual_binding(state, light_id)?;
    let wish = wish.clamped();
    let wall = state.lock_config()?.room.wall;
    apply_one(state, &binding, wish, wall).map_err(|e| named_for(state, light_id, e))?;
    state
        .lock_config()?
        .states
        .insert(light_id.to_string(), wish);
    Ok(())
}

/// Show each wish on its light, all at once, without recording them: a fade
/// passes through states nobody chose. Lights an effect drives are skipped,
/// and a light that does not answer is left as it is.
pub fn show_lights(state: &AppState, wishes: &[(String, LightState)]) {
    let _change = state.effects.change();
    let Ok(wall) = state.lock_config().map(|c| c.room.wall) else {
        return;
    };
    std::thread::scope(|scope| {
        for (id, wish) in wishes {
            if let Ok(binding) = manual_binding(state, id) {
                let wish = wish.clamped();
                scope.spawn(move || apply_one(state, &binding, wish, wall));
            }
        }
    });
}

/// Switch one light on or off, leaving its colour to the bulb. Off takes the
/// light out of the effect playing on it first: a stream does not leave a
/// bulb off (a Yeelight leaves music mode and is opened again, a Tuya bulb
/// is switched on with the next frame), so switching it off under the loop
/// only made the bulb drop out and come back.
pub fn set_light_power(state: &AppState, light_id: &str, on: bool) -> AppResult<()> {
    let binding = binding(state, light_id)?;
    if !on && state.effects.mode_of(light_id).is_some() {
        crate::auto::user_took_over(state);
        move_lights(state, &[light_id.to_string()], None, false)?;
    }
    state.manual.release(light_id);
    power_one(&binding.endpoint(), on).map_err(|e| named_for(state, light_id, e))?;
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
    let (wall, jobs): (Rgb, Vec<(DeviceBinding, String, LightState)>) = {
        let cfg = state.lock_config()?;
        let jobs = cfg
            .bindings
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
            .collect();
        (cfg.room.wall, jobs)
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
                        apply_one(state, b, *w, wall)
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

/// Switch off every light, for the sleep timer, but those whose bulb
/// another computer drives: the timer puts this computer's room to bed,
/// not the effect someone plays on the other one.
pub fn sleep_all(state: &AppState) -> AppResult<PowerResult> {
    crate::auto::user_took_over(state);
    stop_effect(state, false);
    apply_many(
        state,
        |b| {
            state
                .leases
                .holder(&b.device_id)
                .is_none()
                .then(|| LightState {
                    on: false,
                    ..LightState::default()
                })
        },
        true,
    )
}

/// Apply a built-in mood (by its id) to `ids`, or to every light with
/// `None`. A mood is a decision about those lights, so whatever effect plays
/// on them stops first; on the whole room, every effect does. The natural
/// light shows what suits the time now, and keeps following the day (see
/// [`follow_natural`]).
pub fn apply_scene(state: &AppState, id: &str, ids: Option<&[String]>) -> AppResult<PowerResult> {
    let wish = if id == scene::NATURAL {
        crate::natural::now()
    } else {
        scene::mood(id)
            .ok_or_else(|| AppError::new("scene_missing"))?
            .state()
    };
    crate::auto::user_took_over(state);
    let ids: Vec<String> = match ids {
        Some(ids) => {
            move_lights(state, ids, None, false)?;
            ids.to_vec()
        }
        None => {
            stop_effect(state, false);
            let cfg = state.lock_config()?;
            cfg.bindings.iter().map(|b| b.light_id.clone()).collect()
        }
    };
    set_natural(state, &ids, id == scene::NATURAL)?;
    apply_many(state, |b| ids.contains(&b.light_id).then_some(wish), false)
}

/// Have `ids` follow the natural light or stop following it, without
/// touching the lights. Either way they take the curve's brightness again.
pub fn set_natural(state: &AppState, ids: &[String], on: bool) -> AppResult<()> {
    let mut cfg = state.lock_config()?;
    let before = (cfg.natural.clone(), cfg.natural_scale.clone());
    for id in ids {
        cfg.natural_scale.remove(id);
        if on {
            cfg.natural.insert(id.clone());
        } else {
            cfg.natural.remove(id);
        }
    }
    if (&cfg.natural, &cfg.natural_scale) == (&before.0, &before.1) {
        return Ok(());
    }
    drop(cfg);
    state.persist()
}

/// A light is about to be set to `wish` by hand. One that follows the
/// natural light and only has its brightness changed keeps following it,
/// as much brighter or dimmer than the curve as it now is; any other change
/// takes it off. A slider drag calls this many times a second, so the
/// record waits for the interface's next save to reach the disk.
pub fn set_by_hand(state: &AppState, light_id: &str, wish: LightState) -> AppResult<()> {
    let mut cfg = state.lock_config()?;
    let now = cfg.states.get(light_id).copied().unwrap_or_default();
    let dimmed = cfg.natural.contains(light_id) && wish.on && now.on && wish.tint == now.tint;
    if !dimmed {
        drop(cfg);
        return set_natural(state, &[light_id.to_string()], false);
    }
    let curve = crate::natural::now().bright.max(1);
    let scale = f32::from(wish.clamped().bright) / f32::from(curve);
    cfg.natural_scale.insert(light_id.to_string(), scale);
    Ok(())
}

/// The natural light's `wish` for a light set `scale` times as bright as the
/// curve, if it was set at all.
fn scaled(wish: LightState, scale: Option<f32>) -> LightState {
    match scale {
        Some(scale) => LightState {
            bright: (f32::from(wish.bright) * scale).round().min(100.0) as u8,
            ..wish
        }
        .clamped(),
        None => wish,
    }
}

/// Bring every light that follows the natural light, is on, and free, to
/// what it shows now, as much brighter or dimmer as it was set by hand. Lights already there are left alone, so this is cheap to call
/// often. `true` if any light was changed.
pub fn follow_natural(state: &AppState, wish: LightState) -> AppResult<bool> {
    let driven = state.effects.driven_ids();
    let behind: BTreeMap<String, LightState> = {
        let cfg = state.lock_config()?;
        cfg.bindings
            .iter()
            .map(|b| &b.light_id)
            .filter(|id| cfg.natural.contains(*id) && !driven.contains(id))
            .filter_map(|id| {
                let wish = scaled(wish, cfg.natural_scale.get(id).copied());
                let now = cfg.states.get(id).copied().unwrap_or_default();
                (now.on && now != wish).then(|| (id.clone(), wish))
            })
            .collect()
    };
    if behind.is_empty() {
        return Ok(false);
    }
    apply_many(state, |b| behind.get(&b.light_id).copied(), false)?;
    Ok(true)
}

/// The walls were repainted: show again, corrected for the new colour, what
/// every light that is on and free was set to. Effects pick it up on their
/// own (see [`runner::LiveSettings`]).
pub fn rewall(state: &AppState) {
    let driven = state.effects.driven_ids();
    let ids: Vec<String> = match state.lock_config() {
        Ok(cfg) => cfg
            .states
            .iter()
            .filter(|(id, s)| s.on && !driven.contains(id))
            .map(|(id, _)| id.clone())
            .collect(),
        Err(_) => return,
    };
    put_back(state, &ids, false);
}

/// Record what lights were set to, and persist it.
fn remember(state: &AppState, f: impl FnOnce(&mut BTreeMap<String, LightState>)) -> AppResult<()> {
    f(&mut state.lock_config()?.states);
    state.persist()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.room.lights = serde_json::from_value(serde_json::json!([
            { "id": "a", "name": "Desk lamp", "pos": { "x": 1.0, "y": 1.0 } },
            { "id": "b", "name": "  ", "pos": { "x": 2.0, "y": 1.0 } },
        ]))
        .unwrap();
        cfg.bindings = serde_json::from_value(serde_json::json!([
            { "light_id": "a", "device_id": "dev-a", "ip": "192.168.1.5" },
            { "light_id": "b", "device_id": "dev-b", "ip": "192.168.1.6" },
        ]))
        .unwrap();
        cfg
    }

    #[test]
    fn a_dimmed_natural_light_keeps_its_share_of_the_curve() {
        let evening = scene::natural_at(20 * 60);
        assert_eq!(scaled(evening, None), evening);
        let half = scaled(evening, Some(0.5));
        assert_eq!(half.tint, evening.tint);
        assert_eq!(half.bright, (f32::from(evening.bright) / 2.0).round() as u8);
        // Set brighter than the curve at noon: never past full brightness.
        assert_eq!(scaled(scene::natural_at(12 * 60), Some(3.0)).bright, 100);
    }

    #[test]
    fn an_unreachable_bulb_goes_by_its_light() {
        let cfg = config();
        let e = named(
            unreachable("192.168.1.5", "timed out"),
            device_light_name(&cfg, "dev-a"),
        );
        assert_eq!(e.subject.as_deref(), Some("Desk lamp"));
        // The address is not lost: the detail keeps it.
        assert_eq!(e.detail.as_deref(), Some("192.168.1.5: timed out"));
    }

    #[test]
    fn without_a_name_the_address_stays() {
        let cfg = config();
        // A light left blank, a bulb in no light, a light that is gone.
        for name in [
            device_light_name(&cfg, "dev-b"),
            device_light_name(&cfg, "dev-new"),
            light_name(&cfg, "gone"),
        ] {
            let e = named(unreachable("192.168.1.6", "timed out"), name);
            assert_eq!(e.subject.as_deref(), Some("192.168.1.6"));
        }
    }

    #[test]
    fn only_an_unreachable_bulb_is_renamed() {
        let e = named(
            AppError::new("invalid_ip").subject("8.8.8.8"),
            Some("Desk lamp"),
        );
        assert_eq!(e.subject.as_deref(), Some("8.8.8.8"));
    }
}
