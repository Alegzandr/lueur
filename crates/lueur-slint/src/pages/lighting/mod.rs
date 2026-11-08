//! The Lighting page: the room's lights as cards, the scenes saved, and
//! beside them what to do with the lights picked (the modes, their
//! settings, brightness, white and colour), with the sleep timer and one
//! switch for the whole room in the page's head, and a getting-started card
//! until the room is set up.
//!
//! The page also holds the two backdrops the window draws under itself
//! (`backdrop.rs`), and restarts the running effects once the room they
//! play has changed and rested ([`actions::follow_room`]).

mod actions;
mod backdrop;
mod glow;
mod model;
mod view;

use crate::rt;
use crate::state::{Lueur, View};
use crate::{AppWindow, Backdrop, LightingState};
use lueur_core::scene::{MOODS, NATURAL, Tint};
use model::{MAX_KELVIN, MIN_KELVIN, PALETTE, Room, hue_rgb};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::time::Duration;

/// What the page keeps for itself.
pub struct Local {
    /// Whether the room was arranged at the last look, and whether the
    /// getting-started card stays for its last step, starting Screen,
    /// until the lights follow it.
    arranged: Cell<Option<bool>>,
    last_step_pending: Cell<bool>,
    /// The scene whose deletion is being asked about.
    confirming: Option<String>,
    /// The name being typed for a new scene.
    typed: String,
    /// What the lights were last given in the other tint, so switching
    /// between White and Colour comes back to it.
    last_kelvin: u16,
    last_colour: [u8; 3],
    /// The hue slider for a colour of one's own, open or not.
    custom_colour: bool,
    /// Counts the sleep timer down by the second while one runs.
    ticker: RefCell<Option<slint::Timer>>,
    pub(crate) backdrop: RefCell<backdrop::State>,
}

impl Default for Local {
    fn default() -> Self {
        Local {
            arranged: Cell::new(None),
            last_step_pending: Cell::new(false),
            confirming: None,
            typed: String::new(),
            last_kelvin: 2700,
            last_colour: PALETTE[1].0,
            custom_colour: false,
            ticker: RefCell::new(None),
            backdrop: RefCell::new(backdrop::State::default()),
        }
    }
}

/// The one light the settings go to, if there is one.
fn single(l: &Lueur) -> Option<String> {
    let room = Room::of(l);
    match room.picked() {
        Some(p) if p.len() == 1 => Some(p[0].clone()),
        Some(_) => None,
        None => {
            let pickable: Vec<&String> = l
                .config
                .room
                .lights
                .iter()
                .map(|x| &x.id)
                .filter(|id| room.bound(id))
                .collect();
            (pickable.len() == 1).then(|| pickable[0].clone())
        }
    }
}

/// The lights the tint goes to: those picked, or the room, set by hand.
fn by_hand(l: &Lueur) -> Vec<String> {
    let room = Room::of(l);
    room.mood_targets()
        .into_iter()
        .filter(|id| room.mode_of(id).is_none())
        .collect()
}

fn tint_all(l: &mut Lueur, tint: Tint) {
    for id in by_hand(l) {
        l.adjust_light(&id, |s| s.tint = tint);
    }
}

/// Switch the lights set by hand between a white and a colour, back to
/// what they were last given in the other.
fn set_tint_mode(l: &mut Lueur, to_colour: bool) {
    let ids = by_hand(l);
    let Some(first) = ids.first() else { return };
    let tint = Room::of(l).state(first).tint;
    let next = match (tint, to_colour) {
        (Tint::White { kelvin }, true) => {
            l.pages.lighting.last_kelvin = kelvin;
            let [r, g, b] = l.pages.lighting.last_colour;
            Tint::Color { r, g, b }
        }
        (Tint::Color { r, g, b }, false) => {
            l.pages.lighting.last_colour = [r, g, b];
            Tint::White {
                kelvin: l.pages.lighting.last_kelvin,
            }
        }
        _ => return,
    };
    tint_all(l, next);
}

pub fn bind(ui: &AppWindow) {
    let b = ui.global::<Backdrop>();
    b.on_frame(backdrop::frame);
    b.on_resized(backdrop::resized);

    let g = ui.global::<LightingState>();
    g.on_set_all_power(|| {
        rt::with(|l| {
            if l.busy {
                return;
            }
            let room = Room::of(l);
            let any_on = l.config.bindings.iter().any(|b| room.on(&b.light_id));
            l.set_all_power(!any_on);
        })
    });
    g.on_stop_effects(|| rt::with(Lueur::toggle_effect));
    g.on_sleep_start(|m| rt::with(|l| l.set_sleep(u32::try_from(m).ok())));
    g.on_sleep_cancel(|| rt::with(|l| l.set_sleep(None)));
    g.on_step(|i| {
        rt::with(|l| match i {
            0 => l.show(View::Bulbs),
            1 => l.show(View::Room),
            _ => l.choose_mode("ambilight"),
        })
    });
    g.on_toggle_target(|id| rt::with(|l| l.toggle_target(&id)));
    g.on_select_all(|| rt::with(|l| l.target.clear()));
    g.on_light_action(|id| {
        rt::with(|l| {
            let room = Room::of(l);
            if !room.bound(&id) {
                l.show(View::Bulbs);
            } else if room.elsewhere(&id).is_some() {
                l.take_light(id.to_string());
            } else {
                let on = room.on(&id);
                l.set_light_power(id.to_string(), !on);
            }
        })
    });
    g.on_choose_tile(|id| {
        rt::with(|l| {
            if id == NATURAL {
                l.apply_mood(NATURAL);
            } else if let Some(m) = MOODS.iter().find(|m| m.id == id.as_str()) {
                l.apply_mood(m.id);
            } else if let Some(mode) = model::ANIMATED.iter().find(|m| **m == id.as_str()) {
                l.choose_mode(mode);
            }
        })
    });
    g.on_recall_scene(|id| {
        rt::with(|l| {
            if let Some(scene) = l.config.scenes.iter().find(|s| s.id == id.as_str()) {
                let scene = scene.clone();
                l.recall_scene(scene);
            }
        })
    });
    g.on_ask_delete(|id| rt::with(|l| l.pages.lighting.confirming = Some(id.to_string())));
    g.on_keep_scene(|_| rt::with(|l| l.pages.lighting.confirming = None));
    g.on_delete_scene(|id| {
        rt::with(|l| {
            l.pages.lighting.confirming = None;
            l.delete_scene(&id);
        })
    });
    g.on_name_edited(|text| rt::with(|l| l.pages.lighting.typed = text.to_string()));
    g.on_save_scene(|name| {
        rt::with(|l| {
            let name = name.trim().to_string();
            if !name.is_empty() {
                l.save_scene(&name);
            }
            l.pages.lighting.typed.clear();
        })
    });
    g.on_zone_plan(|id| rt::with(|l| l.show_on_plan(&id)));
    g.on_feel_chosen(|i| {
        rt::with(|l| {
            if let Some((feel, _)) = usize::try_from(i).ok().and_then(|i| model::FEELS.get(i)) {
                l.edit(|c| model::apply_feel(c, feel));
            }
        })
    });
    g.on_speed_changed(|v| rt::with(|l| l.edit(|c| c.show.speed = v.round() as u8)));
    g.on_flashes_toggled(|| rt::with(|l| l.edit(|c| c.show.flashes = !c.show.flashes)));
    g.on_listen_change(|| rt::with(|l| l.show(View::Settings)));
    g.on_bright_changed(|v| {
        rt::with(|l| {
            let ids = Room::of(l).mood_targets();
            let v = v.round().clamp(1.0, 100.0) as u8;
            l.set_brightness(&ids, v);
        })
    });
    g.on_tint_chosen(|i| rt::with(|l| set_tint_mode(l, i == 1)));
    g.on_kelvin_changed(|v| {
        rt::with(|l| {
            let kelvin = (v.round() as u16).clamp(MIN_KELVIN, MAX_KELVIN);
            tint_all(l, Tint::White { kelvin });
        })
    });
    g.on_swatch_chosen(|i| {
        rt::with(|l| {
            let Some(([r, g, b], _)) = usize::try_from(i).ok().and_then(|i| PALETTE.get(i)) else {
                return;
            };
            l.pages.lighting.custom_colour = false;
            tint_all(
                l,
                Tint::Color {
                    r: *r,
                    g: *g,
                    b: *b,
                },
            );
        })
    });
    g.on_custom_toggled(|| {
        rt::with(|l| l.pages.lighting.custom_colour = !l.pages.lighting.custom_colour)
    });
    g.on_hue_changed(|v| {
        rt::with(|l| {
            let [r, g, b] = hue_rgb(v);
            tint_all(l, Tint::Color { r, g, b });
        })
    });
    g.on_identify(|| {
        rt::with(|l| {
            if let Some(id) = single(l) {
                l.identify_light(&id);
            }
        })
    });
    g.on_show_on_plan(|| {
        rt::with(|l| {
            if let Some(id) = single(l) {
                l.show_on_plan(&id);
            }
        })
    });
}

/// Follow what plays and the room, whatever page shows: the getting-started
/// card's last step, the restart a changed room needs, and the backdrops.
pub fn project_backdrop(l: &Lueur, ui: &AppWindow) {
    let local = &l.pages.lighting;
    let arranged = l.config.interface.room_arranged;
    if local.arranged.get() == Some(false) && arranged {
        local.last_step_pending.set(true);
    }
    local.arranged.set(Some(arranged));
    // Once the lights have followed the screen, the guide has done its
    // work, even if they stop.
    if l.status.running {
        local.last_step_pending.set(false);
    }
    actions::follow_room(l);
    backdrop::project(l, ui);

    // The sleep timer counts down by the second while one runs and the
    // page shows it.
    let wanted = l.sleep.is_some() && l.view == View::Lighting && l.window_shown;
    let mut ticker = local.ticker.borrow_mut();
    if wanted && ticker.is_none() {
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_secs(1),
            rt::refresh_soon,
        );
        *ticker = Some(timer);
    } else if !wanted {
        *ticker = None;
    }
}

pub fn project(l: &Lueur, ui: &AppWindow) {
    view::project(l, ui);
}
