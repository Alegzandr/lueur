//! What the Lighting page does to the lights: moods, modes, power, brightness,
//! white and colour, saved scenes, the sleep timer, and the restart that
//! follows a change to the room.
//!
//! Each action runs the shell's commands in one job off the window's thread,
//! then takes back what the shell recorded of the lights.

use super::model::{Room, dimmed_only, scene_named};
use crate::i18n::{self, t};
use crate::rt;
use crate::state::Lueur;
use lueur_core::color::Rgb;
use lueur_core::config::{AppConfig, SavedScene, SceneLight};
use lueur_core::scene::{LightState, NATURAL};
use lueur_shell::lights::PowerResult;
use lueur_shell::service::{self, EffectStatus};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

thread_local! {
    static SENDING: RefCell<Sending> = RefCell::new(Sending::default());
    static STARTED: RefCell<Started> = RefCell::new(Started::default());
}

/// Whether a wish for this light is on its way or queued.
fn sending_has(id: &str) -> bool {
    SENDING.with_borrow(|s| s.has(id))
}

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

impl Sending {
    /// True while a wish for this light is on its way or queued, so the
    /// local value is newer than anything the shell has.
    fn has(&self, id: &str) -> bool {
        self.in_flight.contains(id) || self.waiting.contains_key(id)
    }
}

/// How long the room must stay as it is before the effects restart on it,
/// so typing a position or pressing an arrow key a few times restarts them
/// once, not at every step.
const RESTART_AFTER: Duration = Duration::from_millis(600);

/// The effects read the room (the plan, which bulb drives which light, the
/// capture) only when they start, so a change to it restarts them. This
/// keeps the room they play and the restart waiting to happen.
#[derive(Default)]
struct Started {
    /// The room the running effects read when they started.
    playing: Option<String>,
    /// The room a restart was last planned for. The window hears of other
    /// news (what plays, a toast) all the time; this keeps that news from
    /// putting the restart off again, and keeps a restart that failed from
    /// being tried again until the room changes once more.
    planned: Option<String>,
    /// The restart waiting for the room to rest. Dropping it cancels it.
    waiting: Option<slint::Timer>,
}

/// Follow what plays and the room as drawn: note the room when an effect is
/// first seen playing, forget it when nothing plays, and restart the
/// effects once a change to the room has rested. Never while a drag on the
/// plan lasts: its release brings the call that plans the restart.
pub fn follow_room(l: &Lueur) {
    let running = l.status.running;
    let arranging = l.arranging;
    let room = Room::of(l).topology();
    STARTED.with_borrow_mut(|started| {
        if !running {
            *started = Started::default();
            return;
        }
        let Some(playing) = &started.playing else {
            started.playing = Some(room);
            return;
        };
        if *playing == room || arranging {
            // Back as the effects play it, or still being dragged: nothing
            // to restart yet.
            started.planned = None;
            started.waiting = None;
            return;
        }
        if started.planned.as_ref() == Some(&room) {
            return;
        }
        started.planned = Some(room);
        let waiting = slint::Timer::default();
        waiting.start(slint::TimerMode::SingleShot, RESTART_AFTER, || {
            rt::with(Lueur::restart_on_room);
        });
        started.waiting = Some(waiting);
    });
}

impl Lueur {
    /// Take the lights' states the shell recorded. Lights being set by hand
    /// keep their value, which is newer.
    fn take_lights(&mut self, config: AppConfig) {
        let mut states = config.states;
        for (id, s) in &self.config.states {
            if sending_has(id) {
                states.insert(id.clone(), *s);
            }
        }
        let natural = config
            .natural
            .into_iter()
            .filter(|id| !sending_has(id))
            .collect();
        self.config.states = states;
        self.config.natural = natural;
        self.config.modes = config.modes;
    }

    /// Say which bulbs did not answer, when some did not.
    fn report_partial(&mut self, res: &PowerResult) {
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
        self.inform(text);
    }

    /// Every bound bulb on or off. Off also stops the effects (the shell
    /// does it).
    pub fn set_all_power(&mut self, on: bool) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.run_then(
            move |s| {
                let res = service::set_all_power(s, on)?;
                Ok((res, service::get_config(s)?, service::effect_status(s)))
            },
            |this, result| {
                this.busy = false;
                match result {
                    Ok((res, config, status)) => {
                        this.take_lights(config);
                        this.status = status;
                        this.report_partial(&res);
                    }
                    Err(e) => this.fail(&e),
                }
            },
        );
    }

    /// A built-in mood, or the natural light, on the lights picked or the
    /// whole room. Stops the effects playing on them.
    pub fn apply_mood(&mut self, id: &str) {
        if self.pending.is_some() {
            return;
        }
        self.pending = Some(id.to_string());
        let picked = Room::of(self).picked();
        let id = id.to_string();
        self.run_then(
            move |s| {
                let res = service::apply_scene(s, &id, picked.as_deref())?;
                Ok((res, service::get_config(s)?, service::effect_status(s)))
            },
            |this, result| {
                this.pending = None;
                match result {
                    Ok((res, config, status)) => {
                        this.take_lights(config);
                        this.status = status;
                        this.report_partial(&res);
                    }
                    Err(e) => this.fail(&e),
                }
            },
        );
    }

    /// The ambilight or a show on the lights picked, or on the whole room.
    /// Picking one they all play already stops it on them.
    pub fn choose_mode(&mut self, mode: &'static str) {
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
        self.run_then(
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
            |this, result| {
                this.busy = false;
                this.pending = None;
                match result {
                    Ok((silent, config, status)) => {
                        this.take_lights(config);
                        this.status = status;
                        if silent {
                            let text = this.t("started_silent");
                            this.inform(text);
                        }
                    }
                    Err(e) => {
                        this.fail(&e);
                        this.refresh_status();
                    }
                }
            },
        );
    }

    /// Take a light's bulb back from whatever else drives it, so the effect
    /// playing on it here drives it again.
    pub fn take_light(&mut self, id: String) {
        self.run_then(
            move |s| service::take_light(s, &id),
            |this, result: Result<EffectStatus, _>| match result {
                Ok(status) => {
                    this.status = status;
                    // A light set by hand shows its colour again.
                    this.reload_config();
                }
                Err(e) => {
                    this.fail(&e);
                    this.refresh_status();
                }
            },
        );
    }

    /// Save the room and restart the effects on it. A failure shows as an
    /// error toast, like any other.
    fn restart_on_room(&mut self) {
        if self.busy {
            // An effect is being started or stopped. Once it is, the window
            // hears of it and plans the restart again.
            STARTED.with_borrow_mut(|s| s.planned = None);
            return;
        }
        if !self.status.running {
            return;
        }
        let room = Room::of(self).topology();
        self.busy = true;
        let config = self.config.clone();
        self.run_then(
            move |s| {
                service::save_config(s, config)?;
                service::restart_effect(s)
            },
            move |this, result| {
                this.busy = false;
                match result {
                    Ok(status) => {
                        this.status = status;
                        STARTED.with_borrow_mut(|started| {
                            started.playing = Some(room);
                            started.planned = None;
                        });
                    }
                    Err(e) => {
                        this.fail(&e);
                        this.refresh_status();
                    }
                }
            },
        );
    }

    /// Set the sleep timer, or cancel it with None.
    pub fn set_sleep(&mut self, minutes: Option<u32>) {
        self.run(
            move |s| service::set_sleep_timer(s, minutes),
            |this, status| this.sleep = status.map(|s| (s, Instant::now())),
        );
    }

    /// Flash a light's bulb so the user can tell which one it is.
    pub fn identify_light(&mut self, id: &str) {
        let Some(endpoint) = self.config.binding_for(id).map(|b| b.endpoint()) else {
            let text = self.t("no_bulb_yet");
            self.inform(text);
            return;
        };
        self.run(
            move |s| service::test_device(s, endpoint, IDENTIFY),
            |_, ()| {},
        );
    }

    /// The light on the room's plan, picked there.
    pub fn show_on_plan(&mut self, id: &str) {
        self.selected_light = Some(id.to_string());
        self.show(crate::state::View::Room);
    }

    /// Pick a light for effects and moods, or put it back. Picking them all
    /// is the same as the whole room.
    pub fn toggle_target(&mut self, id: &str) {
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
    }

    /// Change part of a light's state (brightness, white, colour), show it
    /// and send it. Touching a light that is off turns it on.
    pub fn adjust_light(&mut self, id: &str, patch: impl FnOnce(&mut LightState)) {
        let now = Room::of(self).state(id);
        let mut next = now;
        next.on = true;
        patch(&mut next);
        let key = id.to_string();
        self.edit(|c| {
            c.states.insert(key.clone(), next);
            // The shell takes it off the natural light too, unless only its
            // brightness changed: then it keeps following, that much dimmer
            // or brighter.
            if !dimmed_only(&now, &next) {
                c.natural.remove(&key);
            }
        });
        self.send_state(id.to_string(), next);
    }

    fn send_state(&mut self, id: String, state: LightState) {
        let queued = SENDING.with_borrow_mut(|sending| {
            if sending.in_flight.contains(&id) {
                sending.waiting.insert(id.clone(), state);
                return true;
            }
            sending.in_flight.insert(id.clone());
            false
        });
        if queued {
            return;
        }
        let light = id.clone();
        self.run_then(
            move |s| service::set_light_state(s, &light, state),
            move |this, result| {
                let next = SENDING.with_borrow_mut(|sending| {
                    sending.in_flight.remove(&id);
                    sending.waiting.remove(&id)
                });
                match result {
                    Ok(()) => {
                        if let Some(next) = next {
                            this.send_state(id, next);
                        }
                    }
                    Err(e) => {
                        // One toast per failure, not one per slider step.
                        this.fail(&e);
                        this.revert(id);
                    }
                }
            },
        );
    }

    /// Show what the shell recorded for a light the bulb refused, rather
    /// than a value it never got.
    fn revert(&mut self, id: String) {
        self.run_then(
            |s| service::get_config(s),
            move |this, result| {
                let Ok(config) = result else { return };
                // Moved again meanwhile: that newer wish wins.
                if sending_has(&id) {
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
    pub fn set_brightness(&mut self, ids: &[String], bright: u8) {
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
            self.edit(|c| {
                c.engine.brightness = (f32::from(bright) / 100.0).max(0.05);
            });
        }
        if show {
            self.edit(|c| c.show.bright = bright.max(5));
        }
        for id in by_hand {
            self.adjust_light(&id, |s| s.bright = bright);
        }
    }

    /// Save what every light with a bulb does now as `name`, replacing the
    /// scene of that name if there is one.
    pub fn save_scene(&mut self, name: &str) {
        let room = Room::of(self);
        let lights: BTreeMap<String, SceneLight> = self
            .config
            .bindings
            .iter()
            .map(|b| (b.light_id.clone(), room.record_of(&b.light_id)))
            .collect();
        let same = scene_named(&self.config.scenes, name).map(|s| s.id.clone());
        let name = name.trim().to_string();
        self.edit(move |c| match same {
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
    pub fn delete_scene(&mut self, id: &str) {
        let Some(index) = self.config.scenes.iter().position(|s| s.id == id) else {
            return;
        };
        let scene = self.config.scenes[index].clone();
        self.edit(|c| c.scenes.retain(|s| s.id != scene.id));
        let text = i18n::fill(&self.t("scene_deleted"), &[("name", &scene.name)]);
        let label = t("undo");
        let undo = Rc::new(move || {
            let scene = scene.clone();
            rt::with(|l| {
                l.edit(|c| {
                    let at = index.min(c.scenes.len());
                    c.scenes.insert(at, scene);
                });
            });
        });
        self.inform_with(text, label, undo);
    }

    /// Bring a scene back: each light plays, follows or shows what it did
    /// when the scene was saved. Lights added since are left as they are.
    pub fn recall_scene(&mut self, scene: SavedScene) {
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
        self.run_then(
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
            |this, result| {
                this.busy = false;
                this.pending = None;
                match result {
                    Ok((reports, config, status)) => {
                        this.take_lights(config);
                        this.status = status;
                        for r in &reports {
                            this.report_partial(r);
                        }
                    }
                    Err(e) => {
                        this.fail(&e);
                        this.refresh_status();
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
