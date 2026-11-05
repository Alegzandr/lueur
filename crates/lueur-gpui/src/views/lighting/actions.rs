//! What the Lighting page does to the lights, after `src/lib/state/effect.ts`,
//! `lights.ts` and `scenes.svelte.ts`: moods, modes, power, brightness,
//! white and colour, saved scenes, the sleep timer and the restart banner.
//!
//! Each action runs the shell's commands in one job off the window's thread,
//! then takes back what the shell recorded of the lights.

use super::model::{Room, dimmed_only, scene_named};
use crate::i18n::{self, t};
use crate::state::{Lueur, model};
use gpui::{App, Context, Global, SharedString};
use lueur_core::color::Rgb;
use lueur_core::config::{AppConfig, SavedScene, SceneLight};
use lueur_core::scene::{LightState, NATURAL};
use lueur_shell::lights::PowerResult;
use lueur_shell::service::{self, EffectStatus};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Warm white: an identify flash should read as "this one", never as a
/// colour the effect could have picked.
const IDENTIFY: Rgb = Rgb::new(255, 190, 120);

/// One call per light in flight; while it runs, only the newest wish waits.
/// The first call reaches the bulb (seconds, for one that is unplugged), the
/// rest only move a slider's value along an open stream.
#[derive(Default)]
struct Sending {
    in_flight: HashSet<String>,
    waiting: HashMap<String, LightState>,
}

impl Global for Sending {}

impl Sending {
    /// True while a wish for this light is on its way or queued, so the
    /// local value is newer than anything the shell has.
    fn has(&self, id: &str) -> bool {
        self.in_flight.contains(id) || self.waiting.contains_key(id)
    }
}

/// The room as the running effect read it when it started: the effect reads
/// the plan only then, so a plan changed since needs a restart.
#[derive(Default)]
pub struct Started(pub Option<String>);

impl Global for Started {}

/// Follow what plays: forget the room when nothing does, note it when an
/// effect is first seen playing.
pub fn track_started(lueur: &gpui::Entity<Lueur>, cx: &mut App) {
    let l = lueur.read(cx);
    let running = l.status.running;
    let topology = Room::of(l).topology();
    let known = cx.default_global::<Started>().0.is_some();
    if !running {
        cx.global_mut::<Started>().0 = None;
    } else if !known {
        cx.global_mut::<Started>().0 = Some(topology);
    }
}

/// True when the running effect is out of date with the room as drawn.
pub fn needs_restart(l: &Lueur, cx: &App) -> bool {
    l.status.running
        && cx
            .try_global::<Started>()
            .and_then(|s| s.0.as_ref())
            .is_some_and(|s| *s != Room::of(l).topology())
}

impl Lueur {
    /// Take the lights' states the shell recorded. Lights being set by hand
    /// keep their value, which is newer.
    fn take_lights(&mut self, config: AppConfig, cx: &mut Context<Self>) {
        let sending = cx.default_global::<Sending>();
        let mut states = config.states;
        for (id, s) in &self.config.states {
            if sending.has(id) {
                states.insert(id.clone(), *s);
            }
        }
        let natural = config
            .natural
            .into_iter()
            .filter(|id| !sending.has(id))
            .collect();
        self.config.states = states;
        self.config.natural = natural;
        self.config.modes = config.modes;
        cx.notify();
    }

    /// Say which bulbs did not answer, when some did not.
    fn report_partial(&mut self, res: &PowerResult, cx: &mut Context<Self>) {
        if res.failures.is_empty() {
            return;
        }
        let text = i18n::fill(
            &self.t("partial_power"),
            &[
                ("ok", &res.succeeded.to_string()),
                ("total", &res.total.to_string()),
                ("names", &res.failures.join(", ")),
            ],
        );
        self.inform(text, cx);
    }

    /// Every bound bulb on or off. Off also stops the effects (the shell
    /// does it).
    pub fn set_all_power(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        self.run_then(
            cx,
            move |s| {
                let res = service::set_all_power(s, on)?;
                Ok((res, service::get_config(s)?, service::effect_status(s)))
            },
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok((res, config, status)) => {
                        this.take_lights(config, cx);
                        this.status = status;
                        this.report_partial(&res, cx);
                    }
                    Err(e) => this.fail(&e, cx),
                }
            },
        );
    }

    /// A built-in mood, or the natural light, on the lights picked or the
    /// whole room. Stops the effects playing on them.
    pub fn apply_mood(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(id.to_string());
        cx.notify();
        let picked = Room::of(self).picked();
        let id = id.to_string();
        self.run_then(
            cx,
            move |s| {
                let res = service::apply_scene(s, &id, picked.as_deref())?;
                Ok((res, service::get_config(s)?, service::effect_status(s)))
            },
            |this, result, cx| {
                this.pending = None;
                match result {
                    Ok((res, config, status)) => {
                        this.take_lights(config, cx);
                        this.status = status;
                        this.report_partial(&res, cx);
                    }
                    Err(e) => this.fail(&e, cx),
                }
            },
        );
    }

    /// The ambilight or a show on the lights picked, or on the whole room.
    /// Picking one they all play already stops it on them.
    pub fn choose_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        if self.busy || self.pending.is_some() {
            return;
        }
        let room = Room::of(self);
        let targets = room.effect_targets();
        let picked = room.picked();
        let all_play =
            !targets.is_empty() && targets.iter().all(|id| room.mode_of(id) == Some(mode));
        self.pending = Some(mode.to_string());
        self.busy = true;
        cx.notify();
        self.run_then(
            cx,
            move |s| {
                let mut silent = false;
                if all_play {
                    match &picked {
                        Some(ids) => {
                            service::set_lights_mode(s, ids, None)?;
                        }
                        None => service::stop_effect(s, Some(mode))?,
                    }
                } else if let Some(ids) = &picked {
                    service::set_lights_mode(s, ids, Some(mode))?;
                } else {
                    silent = service::start_effect(s, Some(mode))?.connected_lights == 0;
                }
                Ok((silent, service::get_config(s)?, service::effect_status(s)))
            },
            |this, result, cx| {
                this.busy = false;
                this.pending = None;
                match result {
                    Ok((silent, config, status)) => {
                        this.take_lights(config, cx);
                        this.status = status;
                        if silent {
                            let text = this.t("started_silent");
                            this.inform(text, cx);
                        }
                    }
                    Err(e) => {
                        this.fail(&e, cx);
                        this.refresh_status(cx);
                    }
                }
            },
        );
    }

    /// Take a light's bulb back from whatever else drives it, so the effect
    /// playing on it here drives it again.
    pub fn take_light(&mut self, id: String, cx: &mut Context<Self>) {
        self.run_then(
            cx,
            move |s| service::take_light(s, &id),
            |this, result: Result<EffectStatus, _>, cx| match result {
                Ok(status) => this.status = status,
                Err(e) => {
                    this.fail(&e, cx);
                    this.refresh_status(cx);
                }
            },
        );
    }

    /// Stop and start the effects again so they pick up the room as drawn.
    pub fn restart_effect(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        self.run_then(
            cx,
            |s| service::restart_effect(s),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(status) => {
                        this.status = status;
                        let topology = Room::of(this).topology();
                        cx.set_global(Started(Some(topology)));
                    }
                    Err(e) => {
                        this.fail(&e, cx);
                        this.refresh_status(cx);
                    }
                }
            },
        );
    }

    /// Set the sleep timer, or cancel it with None.
    pub fn set_sleep(&mut self, minutes: Option<u32>, cx: &mut Context<Self>) {
        self.run(
            cx,
            move |s| service::set_sleep_timer(s, minutes),
            |this, status, _| this.sleep = status.map(|s| (s, Instant::now())),
        );
    }

    /// Flash a light's bulb so the user can tell which one it is.
    pub fn identify_light(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(endpoint) = self.config.binding_for(id).map(|b| b.endpoint()) else {
            let text = self.t("no_bulb_yet");
            self.inform(text, cx);
            return;
        };
        self.run(
            cx,
            move |s| service::test_device(s, endpoint, IDENTIFY),
            |_, (), _| {},
        );
    }

    /// The light on the room's plan, picked there.
    pub fn show_on_plan(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected_light = Some(id.to_string());
        self.show(crate::state::View::Room, cx);
    }

    /// Pick a light for effects and moods, or put it back. Picking them all
    /// is the same as the whole room.
    pub fn toggle_target(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut ids = self.target.clone();
        if let Some(i) = ids.iter().position(|t| t == id) {
            ids.remove(i);
        } else {
            ids.push(id.to_string());
        }
        let all = self
            .config
            .bindings
            .iter()
            .all(|b| ids.contains(&b.light_id));
        self.target = if all { Vec::new() } else { ids };
        cx.notify();
    }

    /// Change part of a light's state (brightness, white, colour), show it
    /// and send it. Touching a light that is off turns it on.
    pub fn adjust_light(
        &mut self,
        id: &str,
        patch: impl FnOnce(&mut LightState),
        cx: &mut Context<Self>,
    ) {
        let now = Room::of(self).state(id);
        let mut next = now;
        next.on = true;
        patch(&mut next);
        let key = id.to_string();
        self.edit(cx, |c| {
            c.states.insert(key.clone(), next);
            // The shell takes it off the natural light too, unless only its
            // brightness changed: then it keeps following, that much dimmer
            // or brighter.
            if !dimmed_only(&now, &next) {
                c.natural.remove(&key);
            }
        });
        self.send_state(id.to_string(), next, cx);
    }

    fn send_state(&mut self, id: String, state: LightState, cx: &mut Context<Self>) {
        let sending = cx.default_global::<Sending>();
        if sending.in_flight.contains(&id) {
            sending.waiting.insert(id, state);
            return;
        }
        sending.in_flight.insert(id.clone());
        let light = id.clone();
        self.run_then(
            cx,
            move |s| service::set_light_state(s, &light, state),
            move |this, result, cx| {
                let sending = cx.global_mut::<Sending>();
                sending.in_flight.remove(&id);
                match result {
                    Ok(()) => {
                        if let Some(next) = sending.waiting.remove(&id) {
                            this.send_state(id, next, cx);
                        }
                    }
                    Err(e) => {
                        // One toast per failure, not one per slider step.
                        sending.waiting.remove(&id);
                        this.fail(&e, cx);
                        this.revert(id, cx);
                    }
                }
            },
        );
    }

    /// Show what the shell recorded for a light the bulb refused, rather
    /// than a value it never got.
    fn revert(&mut self, id: String, cx: &mut Context<Self>) {
        self.run_then(
            cx,
            |s| service::get_config(s),
            move |this, result, cx| {
                let Ok(config) = result else { return };
                // Moved again meanwhile: that newer wish wins.
                if cx.default_global::<Sending>().has(&id) {
                    return;
                }
                match config.states.get(&id) {
                    Some(s) => this.config.states.insert(id.clone(), *s),
                    None => this.config.states.remove(&id),
                };
                if config.natural.contains(&id) {
                    this.config.natural.insert(id);
                } else {
                    this.config.natural.remove(&id);
                }
            },
        );
    }

    /// Set the brightness of `ids`, 1..=100: of each light set by hand, and
    /// of each effect one of them plays (an effect has one brightness, for
    /// every light playing it). Lights off are turned on.
    pub fn set_brightness(&mut self, ids: &[String], bright: u8, cx: &mut Context<Self>) {
        let room = Room::of(self);
        let modes: Vec<Option<&str>> = ids.iter().map(|id| room.mode_of(id)).collect();
        let by_hand: Vec<String> = ids
            .iter()
            .zip(&modes)
            .filter(|(_, m)| m.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        let screen = modes.contains(&Some("ambilight"));
        let show = modes.iter().any(|m| m.is_some_and(|m| m != "ambilight"));
        if screen {
            self.edit(cx, |c| {
                c.engine.brightness = (f32::from(bright) / 100.0).max(0.05);
            });
        }
        if show {
            self.edit(cx, |c| c.show.bright = bright.max(5));
        }
        for id in by_hand {
            self.adjust_light(&id, |s| s.bright = bright, cx);
        }
    }

    /// Save what every light with a bulb does now as `name`, replacing the
    /// scene of that name if there is one.
    pub fn save_scene(&mut self, name: &str, cx: &mut Context<Self>) {
        let room = Room::of(self);
        let lights: BTreeMap<String, SceneLight> = self
            .config
            .bindings
            .iter()
            .map(|b| (b.light_id.clone(), room.record_of(&b.light_id)))
            .collect();
        let same = scene_named(&self.config.scenes, name).map(|s| s.id.clone());
        let name = name.trim().to_string();
        self.edit(cx, move |c| match same {
            Some(id) => {
                if let Some(s) = c.scenes.iter_mut().find(|s| s.id == id) {
                    s.lights = lights;
                }
            }
            None => {
                let millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis());
                c.scenes.push(SavedScene {
                    id: format!("scene-{}", base36(millis)),
                    name,
                    lights,
                });
            }
        });
    }

    /// Delete a scene, with an undo in the toast that says so.
    pub fn delete_scene(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(index) = self.config.scenes.iter().position(|s| s.id == id) else {
            return;
        };
        let scene = self.config.scenes[index].clone();
        self.edit(cx, |c| c.scenes.retain(|s| s.id != scene.id));
        let text = i18n::fill(&self.t("scene_deleted"), &[("name", &scene.name)]);
        let label: SharedString = t(cx, "undo");
        let undo = Rc::new(move |_: &mut gpui::Window, cx: &mut App| {
            let scene = scene.clone();
            model(cx).update(cx, |l, cx| {
                l.edit(cx, |c| {
                    let at = index.min(c.scenes.len());
                    c.scenes.insert(at, scene);
                });
            });
        });
        self.inform_with(text, label, undo, cx);
    }

    /// Bring a scene back: each light plays, follows or shows what it did
    /// when the scene was saved. Lights added since are left as they are.
    pub fn recall_scene(&mut self, scene: SavedScene, cx: &mut Context<Self>) {
        if self.busy || self.pending.is_some() {
            return;
        }
        let room = Room::of(self);
        let lights: Vec<(String, SceneLight)> = scene
            .lights
            .into_iter()
            .filter(|(id, _)| room.bound(id))
            .collect();
        // A bulb an effect drives refuses a state: the lights set by hand
        // leave their effect first.
        let freed: Vec<String> = lights
            .iter()
            .filter(|(id, s)| !matches!(s, SceneLight::Mode { .. }) && room.mode_of(id).is_some())
            .map(|(id, _)| id.clone())
            .collect();
        let mut playing: Vec<(String, Vec<String>)> = Vec::new();
        for (id, s) in &lights {
            if let SceneLight::Mode { mode } = s
                && room.mode_of(id) != Some(mode.as_str())
            {
                match playing.iter_mut().find(|(m, _)| m == mode) {
                    Some((_, ids)) => ids.push(id.clone()),
                    None => playing.push((mode.clone(), vec![id.clone()])),
                }
            }
        }
        let natural: Vec<String> = lights
            .iter()
            .filter(|(_, s)| matches!(s, SceneLight::Natural))
            .map(|(id, _)| id.clone())
            .collect();
        let by_hand: Vec<(String, String, LightState)> = lights
            .iter()
            .filter_map(|(id, s)| match s {
                SceneLight::Hand { state } => Some((id.clone(), room.name_of(id), *state)),
                _ => None,
            })
            .collect();
        self.pending = Some(scene.id);
        self.busy = true;
        cx.notify();
        self.run_then(
            cx,
            move |s| {
                if !freed.is_empty() {
                    service::set_lights_mode(s, &freed, None)?;
                }
                for (mode, ids) in &playing {
                    service::set_lights_mode(s, ids, Some(mode))?;
                }
                let mut reports = Vec::new();
                if !natural.is_empty() {
                    reports.push(service::apply_scene(s, NATURAL, Some(&natural))?);
                }
                // Each bulb on its own thread: one that is unplugged keeps
                // the others waiting no longer than itself.
                let failures: Vec<String> = std::thread::scope(|scope| {
                    let calls: Vec<_> = by_hand
                        .iter()
                        .map(|(id, name, state)| {
                            scope.spawn(move || {
                                service::set_light_state(s, id, *state)
                                    .err()
                                    .map(|_| name.clone())
                            })
                        })
                        .collect();
                    calls
                        .into_iter()
                        .filter_map(|c| c.join().ok().flatten())
                        .collect()
                });
                reports.push(PowerResult {
                    total: by_hand.len(),
                    succeeded: by_hand.len() - failures.len(),
                    failures,
                });
                Ok((reports, service::get_config(s)?, service::effect_status(s)))
            },
            |this, result, cx| {
                this.busy = false;
                this.pending = None;
                match result {
                    Ok((reports, config, status)) => {
                        this.take_lights(config, cx);
                        this.status = status;
                        for r in &reports {
                            this.report_partial(r, cx);
                        }
                    }
                    Err(e) => {
                        this.fail(&e, cx);
                        this.refresh_status(cx);
                    }
                }
            },
        );
    }
}

/// A number in base 36, as JavaScript's `toString(36)` writes it.
fn base36(mut n: u128) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_are_written_as_javascript_writes_them() {
        assert_eq!(super::base36(0), "0");
        assert_eq!(super::base36(36 * 36 + 35), "10z");
    }
}
