//! The Room page: the plan of the room on the left, and beside it the list
//! of what it holds or, once something is picked, that thing's inspector.
//!
//! The plan is worked out in [`draw`], the list and the inspector in
//! [`inspector`]; the arithmetic both use is in [`plan`] and the undo
//! history in [`history`]. What is picked lives in two places: a light in
//! the shared [`Lueur::selected_light`] (the Bulbs page links to it), the
//! screen or a corner only here.

mod colors;
mod draw;
pub mod history;
mod inspector;
pub mod plan;

use crate::i18n::{fill, t};
use crate::rt;
use crate::state::{Lueur, ToastAction, View};
use crate::{AppWindow, RoomState};
use history::{History, Snap};
use lueur_core::color::Rgb;
use lueur_core::geometry::{Light, Room, Vec2, map_light};
use lueur_shell::service;
use plan::{Other, Target, ViewBox};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// What a drag on the plan moves, and where it was grabbed.
#[derive(Debug, Clone)]
enum Drag {
    Light {
        id: String,
        off: Vec2,
    },
    Screen {
        off: Vec2,
    },
    Corner {
        index: usize,
        off: Vec2,
    },
    /// The seat, which turns the screen.
    Seat,
    /// An end of the screen, which sets its width.
    Width,
}

/// One axis of a typed position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// What the page keeps for itself. The history is fed while the window is
/// brought up to date, which only reads the state: it sits in cells.
#[derive(Default)]
pub struct Local {
    other: Option<Other>,
    drag: Option<Drag>,
    hover: Option<Target>,
    /// The view held still while dragging.
    held: Option<ViewBox>,
    /// The list of corners stays unfolded while one of them is selected, so
    /// going back lands on its row.
    corners_open: bool,
    /// The exact position, folded under a light's or the screen's card.
    exact_open: bool,
    history: RefCell<History>,
    settle: RefCell<Option<slint::Timer>>,
    /// True while the page is the one shown.
    shown: Cell<bool>,
    /// Where a light moved with the arrows went, for screen readers.
    announcement: String,
    announce: Option<slint::Timer>,
    /// The selection the inspector's fields were last filled for, and the
    /// name the light had then.
    fields_for: RefCell<Option<String>>,
    name_before: RefCell<String>,
    /// Asks for the focus, counted up each time (see `state.slint`).
    focus_inspector: i32,
    focus_row: Option<String>,
    focus_row_seq: i32,
}

/// The stage's size and the frame of the plan on it, as the window has it now.
fn frame(l: &Lueur) -> plan::Frame {
    let (w, h) = rt::ui()
        .map(|ui| {
            let g = ui.global::<RoomState>();
            (g.get_stage_width(), g.get_stage_height())
        })
        .unwrap_or((0.0, 0.0));
    let vb = l
        .pages
        .room
        .held
        .unwrap_or_else(|| plan::fit(&l.config.room, w, h));
    plan::Frame::new(vb, (0.0, 0.0), w, h)
}

pub fn bind(ui: &AppWindow) {
    let g = ui.global::<RoomState>();
    g.on_resized(|| rt::with(|_| {}));
    g.on_press(|x, y| rt::with(|l| press(l, x, y)));
    g.on_double(|x, y| {
        rt::with(|l| {
            let f = frame(l);
            let p = f.to_plan(x, y);
            let screen_selected = l.pages.room.other == Some(Other::Screen);
            if plan::hit(&l.config.room, p, f.k, screen_selected) == Some(Target::Wall) {
                insert_corner_at(l, p);
            }
        })
    });
    g.on_drag(|x, y, shift| {
        rt::with(|l| {
            let p = frame(l).to_plan(x, y);
            drag_to(l, p, shift);
        })
    });
    g.on_hover(|x, y| {
        // Read first: a pointer that moves over nothing new changes nothing.
        let hover = rt::read(|l| {
            let f = frame(l);
            let screen_selected = l.pages.room.other == Some(Other::Screen);
            plan::hit(&l.config.room, f.to_plan(x, y), f.k, screen_selected)
        });
        if rt::read(|l| l.pages.room.hover != hover) {
            rt::with(|l| l.pages.room.hover = hover);
        }
    });
    g.on_leave(|| {
        if rt::read(|l| l.pages.room.hover.is_some()) {
            rt::with(|l| l.pages.room.hover = None);
        }
    });
    g.on_release(|| rt::with(release));
    g.on_key(|text, shift, ctrl, alt| rt::with(|l| on_key(l, &text, shift, ctrl, alt)));
    g.on_pin_picked(|id| rt::with(|l| select_light(l, id.to_string())));
    g.on_pin_focused(|_| {});
    g.on_undo(|| rt::with(|l| travel(l, false)));
    g.on_redo(|| rt::with(|l| travel(l, true)));
    g.on_coach_done(|| rt::with(mark_arranged));
    g.on_pick(|key| rt::with(|l| pick(l, &key)));
    g.on_back(|| rt::with(|l| clear_selection(l, true)));
    g.on_add_light(|| rt::with(new_light));
    g.on_toggle_corners(|| rt::with(|l| l.pages.room.corners_open = !l.pages.room.corners_open));
    g.on_add_corner(|| rt::with(add_corner));
    g.on_reset_shape(|| rt::with(reset_shape));
    g.on_paint(|i| rt::with(|l| paint(l, i)));
    g.on_toggle_exact(|| rt::with(|l| l.pages.room.exact_open = !l.pages.room.exact_open));
    g.on_set_height(|v| {
        rt::with(|l| {
            if let Some(id) = selected_light(l) {
                edit_light(l, &id, |x| x.height = v);
            }
        })
    });
    g.on_toggle_follows(|| {
        rt::with(|l| {
            if let Some(id) = selected_light(l) {
                edit_light(l, &id, |x| x.enabled = !x.enabled);
            }
        })
    });
    g.on_identify(|| {
        rt::with(|l| {
            if let Some(id) = selected_light(l) {
                identify(l, &id);
            }
        })
    });
    g.on_assign(|| rt::with(|l| l.show(View::Bulbs)));
    g.on_remove_light(|| {
        rt::with(|l| {
            if let Some(id) = selected_light(l) {
                remove_light(l, &id, true);
            }
        })
    });
    g.on_remove_corner(|| {
        rt::with(|l| {
            if let Some(Other::Corner(i)) = l.pages.room.other {
                remove_corner(l, i, true);
            }
        })
    });
    g.on_set_facing(|v| rt::with(|l| l.edit(|c| c.room.screen.facing_deg = v)));
    g.on_set_width(|v| rt::with(|l| l.edit(|c| c.room.screen.width = v)));
    g.on_name_edited(|text| {
        rt::with(|l| {
            if let Some(id) = selected_light(l) {
                rename(l, &id, &text);
            }
        })
    });
    g.on_name_done(|| {
        rt::with(|l| {
            if let Some(id) = selected_light(l) {
                let before = l.pages.room.name_before.borrow().clone();
                settle_name(l, &id, &before);
            }
        })
    });
    g.on_pos_edited(|axis, text| {
        let axis = if axis == 0 { Axis::X } else { Axis::Y };
        // A value is taken while typing only once it is in bounds, so
        // typing "420" does not stop at 12 on the way.
        let max = if axis == Axis::X {
            plan::X_MAX
        } else {
            plan::Y_MAX
        };
        if let Ok(v) = text.trim().parse::<f32>()
            && (plan::MARGIN..=max).contains(&v)
        {
            rt::with(|l| set_pos(l, axis, v));
        }
    });
    g.on_pos_done(|axis| {
        let axis = if axis == 0 { Axis::X } else { Axis::Y };
        let text = rt::ui()
            .map(|ui| {
                let g = ui.global::<RoomState>();
                if axis == Axis::X {
                    g.get_x_text()
                } else {
                    g.get_y_text()
                }
            })
            .unwrap_or_default();
        rt::with(|l| settle_pos(l, axis, &text));
    });
    g.on_escape(|| rt::with(|l| clear_selection(l, true)));
}

/// Bring the page up to date: feed the history while the page shows, then
/// draw the plan and the column beside it.
pub fn project(l: &Lueur, ui: &AppWindow) {
    let local = &l.pages.room;
    let snap = Snap::of(&l.config);
    if !local.shown.replace(true) {
        local.history.borrow_mut().track(&snap);
    }
    observe_plan(l, &snap);
    let g = ui.global::<RoomState>();
    draw::project(l, &g);
    inspector::project(l, &g);
}

/// Called whenever another page is shown, so the history starts afresh the
/// next time the plan may have changed elsewhere.
pub fn hidden(l: &Lueur) {
    l.pages.room.shown.set(false);
}

/// The plan may have changed: a step stays open while a drag lasts, and
/// closes once the plan has rested.
fn observe_plan(l: &Lueur, snap: &Snap) {
    let local = &l.pages.room;
    let changed = changed_since(snap);
    let open = local.history.borrow_mut().observe(snap);
    if !open || local.drag.is_some() {
        *local.settle.borrow_mut() = None;
        return;
    }
    // Only a change starts the quiet time over.
    if local.settle.borrow().as_ref().is_some_and(|t| t.running()) && !changed {
        return;
    }
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::SingleShot, history::SETTLE, || {
        rt::with(|l| {
            let snap = Snap::of(&l.config);
            l.pages.room.history.borrow_mut().commit(&snap);
        })
    });
    *local.settle.borrow_mut() = Some(timer);
}

thread_local! {
    /// The plan as last seen, to tell a change from a mere redraw.
    static LAST: RefCell<Option<(Room, usize)>> = const { RefCell::new(None) };
}

fn changed_since(snap: &Snap) -> bool {
    LAST.with_borrow_mut(|last| {
        let now = (snap.room.clone(), snap.bindings.len());
        let changed = last.as_ref() != Some(&now);
        *last = Some(now);
        changed
    })
}

// --- Selection ---

fn selected_light(l: &Lueur) -> Option<String> {
    l.selected_light
        .clone()
        .filter(|id| l.config.room.lights.iter().any(|x| x.id == *id))
}

/// The list row of what is selected now, e.g. "light:<id>" or "corner:2".
fn selected_key(l: &Lueur) -> Option<String> {
    if let Some(id) = selected_light(l) {
        return Some(format!("light:{id}"));
    }
    match l.pages.room.other {
        Some(Other::Screen) => Some("screen".into()),
        Some(Other::Corner(i)) => Some(format!("corner:{i}")),
        None => None,
    }
}

/// The keys of the list's rows, in their order.
fn row_keys(l: &Lueur) -> Vec<String> {
    let room = &l.config.room;
    let mut keys: Vec<String> = room
        .lights
        .iter()
        .map(|x| format!("light:{}", x.id))
        .collect();
    keys.push("screen".into());
    if l.pages.room.corners_open {
        keys.extend((0..room.walls.len()).map(|i| format!("corner:{i}")));
    }
    keys
}

fn select_light(l: &mut Lueur, id: String) {
    l.pages.room.other = None;
    l.selected_light = Some(id);
}

fn select_other(l: &mut Lueur, other: Other) {
    l.selected_light = None;
    l.pages.room.other = Some(other);
    if matches!(other, Other::Corner(_)) {
        l.pages.room.corners_open = true;
    }
}

/// A pick from the list or an add button: the focus follows it into the
/// inspector. A pick on the plan with the pointer leaves it alone.
fn pick(l: &mut Lueur, key: &str) {
    if let Some(id) = key.strip_prefix("light:") {
        select_light(l, id.to_string());
    } else if key == "screen" {
        select_other(l, Other::Screen);
    } else if let Some(i) = key.strip_prefix("corner:").and_then(|i| i.parse().ok()) {
        select_other(l, Other::Corner(i));
    } else {
        return;
    }
    focus_inspector_soon(l);
}

/// The inspector is made once the window has drawn the pick: the focus
/// goes there a moment after.
fn focus_inspector_soon(_l: &mut Lueur) {
    slint::Timer::single_shot(Duration::from_millis(30), || {
        rt::with(|l| l.pages.room.focus_inspector += 1);
    });
}

/// Back to the list. With `refocus`, the focus returns to the row of what
/// was selected.
fn clear_selection(l: &mut Lueur, refocus: bool) {
    let key = selected_key(l);
    l.selected_light = None;
    l.pages.room.other = None;
    if refocus && let Some(key) = key {
        focus_row_soon(key);
    }
}

/// Back to the list, on the row `key` (or, when it is gone, the one now at
/// its place, `index`).
fn focus_row_soon(key: String) {
    slint::Timer::single_shot(Duration::from_millis(30), move || {
        rt::with(|l| {
            l.pages.room.focus_row = Some(key);
            l.pages.room.focus_row_seq += 1;
        });
    });
}

fn focus_row_index_soon(l: &Lueur, index: usize) {
    let keys = row_keys(l);
    if let Some(key) = keys.get(index.min(keys.len().saturating_sub(1))) {
        focus_row_soon(key.clone());
    }
}

/// Where an object's row sits in the list, to land on its neighbour.
fn row_index(l: &Lueur, key: &str) -> usize {
    row_keys(l).iter().position(|k| k == key).unwrap_or(0)
}

// --- Editing ---

fn edit_light(l: &mut Lueur, id: &str, change: impl FnOnce(&mut Light)) {
    l.edit(|c| {
        if let Some(x) = c.room.lights.iter_mut().find(|x| x.id == id) {
            change(x);
        }
    });
}

fn new_light(l: &mut Lueur) {
    let template = t("light_n");
    let room = l.config.room.clone();
    let name = plan::fresh_name(&room.lights, |n| fill(&template, &[("n", &n.to_string())]));
    let pos = plan::fresh_positions(&room, 1)[0];
    let light = Light {
        id: plan::new_light_id(),
        name,
        pos,
        height: 0.5,
        zone_override: None,
        enabled: true,
    };
    let id = light.id.clone();
    l.edit(|c| c.room.lights.push(light));
    select_light(l, id);
    focus_inspector_soon(l);
}

/// The button's way to add a corner: split the longest wall in two.
fn add_corner(l: &mut Lueur) {
    let walls = l.config.room.walls.clone();
    if walls.len() < 2 {
        return;
    }
    let best = plan::longest_wall(&walls);
    l.edit(|c| {
        let n = c.room.walls.len();
        let (a, b) = (c.room.walls[best], c.room.walls[(best + 1) % n]);
        c.room.walls.insert(
            best + 1,
            Vec2::new(((a.x + b.x) / 2.0).round(), ((a.y + b.y) / 2.0).round()),
        );
    });
    select_other(l, Other::Corner(best + 1));
    focus_inspector_soon(l);
}

fn remove_corner(l: &mut Lueur, index: usize, refocus: bool) {
    if l.config.room.walls.len() <= 3 {
        let text = t("min_corners");
        l.inform(text);
        return;
    }
    let row = row_index(l, &format!("corner:{index}"));
    l.edit(|c| {
        if index < c.room.walls.len() {
            c.room.walls.remove(index);
        }
    });
    l.pages.room.other = None;
    if refocus {
        focus_row_index_soon(l, row);
    }
}

/// Remove a light (and its bulb assignment), with an undo in the toast.
fn remove_light(l: &mut Lueur, id: &str, refocus: bool) {
    let row = row_index(l, &format!("light:{id}"));
    let c = &l.config;
    let Some((index, light, binding)) = c
        .room
        .lights
        .iter()
        .position(|x| x.id == id)
        .map(|i| (i, c.room.lights[i].clone(), c.binding_for(id).cloned()))
    else {
        return;
    };
    let text = fill(&t("removed"), &[("name", &light.name)]);
    let id = id.to_string();
    l.edit(|c| {
        c.room.lights.retain(|x| x.id != id);
        c.bindings.retain(|b| b.light_id != id);
    });
    if l.selected_light.as_deref() == Some(id.as_str()) {
        l.selected_light = None;
    }
    let action: ToastAction = Rc::new(move || {
        let (light, binding) = (light.clone(), binding.clone());
        rt::with(|l| {
            l.edit(|c| {
                // Already back, by the plan's own undo: once is enough.
                if c.room.lights.iter().any(|x| x.id == light.id) {
                    return;
                }
                let at = index.min(c.room.lights.len());
                c.room.lights.insert(at, light);
                // The bulb went to another light since: it stays there.
                if let Some(b) = binding
                    && !c.bindings.iter().any(|x| x.device_id == b.device_id)
                {
                    c.bindings.push(b);
                }
            })
        });
    });
    l.inform_with(text, t("undo"), action);
    if refocus {
        focus_row_index_soon(l, row);
    }
}

fn remove_selected(l: &mut Lueur, refocus: bool) {
    if let Some(id) = selected_light(l) {
        remove_light(l, &id, refocus);
    } else if let Some(Other::Corner(i)) = l.pages.room.other {
        remove_corner(l, i, refocus);
    }
}

/// Back to the room's first shape: its walls and its screen.
fn reset_shape(l: &mut Lueur) {
    let first = Room::default();
    l.edit(|c| {
        c.room.walls = first.walls;
        c.room.screen = first.screen;
    });
    clear_selection(l, false);
    let text = t("reset_done");
    l.inform(text);
}

/// A paint picked, or with the arrows -2 for the next and -3 for the one
/// before.
fn paint(l: &mut Lueur, i: i32) {
    let n = inspector::WALLS.len() as i32;
    let from = inspector::WALLS
        .iter()
        .position(|(_, c)| *c == l.config.room.wall)
        .map_or(0, |p| p as i32);
    let to = match i {
        -2 => (from + 1) % n,
        -3 => (from + n - 1) % n,
        i if (0..n).contains(&i) => i,
        _ => return,
    };
    let color = inspector::WALLS[to as usize].1;
    l.edit(|c| c.room.wall = color);
}

fn nudge(l: &mut Lueur, dx: f32, dy: f32) {
    let light = selected_light(l);
    let other = l.pages.room.other;
    let by = |p: Vec2| plan::clamp_p(Vec2::new(p.x + dx, p.y + dy));
    l.edit(|c| {
        if let Some(id) = &light {
            if let Some(x) = c.room.lights.iter_mut().find(|x| x.id == *id) {
                x.pos = by(x.pos);
            }
        } else if other == Some(Other::Screen) {
            c.room.screen.center = by(c.room.screen.center);
        } else if let Some(Other::Corner(i)) = other
            && let Some(w) = c.room.walls.get_mut(i)
        {
            *w = by(*w);
        }
    });
    announce_soon(l);
}

/// Where the selected object is now.
fn selected_pos(l: &Lueur) -> Option<Vec2> {
    let room = &l.config.room;
    if let Some(id) = selected_light(l) {
        return room.lights.iter().find(|x| x.id == id).map(|x| x.pos);
    }
    match l.pages.room.other {
        Some(Other::Screen) => Some(room.screen.center),
        Some(Other::Corner(i)) => room.walls.get(i).copied(),
        None => None,
    }
}

/// Move the selected object along one axis, kept on the plan.
fn set_pos(l: &mut Lueur, axis: Axis, v: f32) {
    let light = selected_light(l);
    let other = l.pages.room.other;
    let put = |p: Vec2| {
        plan::clamp_p(match axis {
            Axis::X => Vec2::new(v, p.y),
            Axis::Y => Vec2::new(p.x, v),
        })
    };
    l.edit(|c| {
        if let Some(id) = &light {
            if let Some(x) = c.room.lights.iter_mut().find(|x| x.id == *id) {
                x.pos = put(x.pos);
            }
        } else if other == Some(Other::Screen) {
            c.room.screen.center = put(c.room.screen.center);
        } else if let Some(Other::Corner(i)) = other
            && let Some(w) = c.room.walls.get_mut(i)
        {
            *w = put(*w);
        }
    });
    // Placing a light or the screen is arranging the room, as a drag is.
    if light.is_some() || other == Some(Other::Screen) {
        mark_arranged(l);
    }
}

/// Leaving a position field: take what it says, kept on the plan, and show
/// what was kept (the clamped value, or the old one for no number).
fn settle_pos(l: &mut Lueur, axis: Axis, text: &str) {
    if selected_pos(l).is_none() {
        return;
    }
    if let Ok(v) = text.trim().parse::<f32>()
        && v.is_finite()
    {
        set_pos(l, axis, v);
    }
    if let (Some(p), Some(ui)) = (selected_pos(l), rt::ui()) {
        let g = ui.global::<RoomState>();
        let v = if axis == Axis::X { p.x } else { p.y };
        let shown = v.to_string();
        if axis == Axis::X {
            g.set_x_text(shown.into());
        } else {
            g.set_y_text(shown.into());
        }
    }
}

/// The plan has been arranged: the first-run guide moves on.
fn mark_arranged(l: &mut Lueur) {
    if !l.config.interface.room_arranged {
        l.edit(|c| c.interface.room_arranged = true);
    }
}

fn rename(l: &mut Lueur, id: &str, name: &str) {
    let name: String = name.chars().take(plan::MAX_LIGHT_NAME).collect();
    edit_light(l, id, |x| x.name = name);
}

/// Once its field is left, a light without a name gets back the one it
/// had, or a fresh one: a nameless light would leave its row and its pin
/// on the plan blank.
fn settle_name(l: &mut Lueur, id: &str, before: &str) {
    let template = t("light_n");
    let lights = l.config.room.lights.clone();
    let Some(light) = lights.iter().find(|x| x.id == id) else {
        return;
    };
    let name = if !light.name.trim().is_empty() {
        light.name.trim().to_string()
    } else if !before.trim().is_empty() {
        before.trim().to_string()
    } else {
        plan::fresh_name(&lights, |n| fill(&template, &[("n", &n.to_string())]))
    };
    if name != light.name {
        rename(l, id, &name);
        if let Some(ui) = rt::ui() {
            ui.global::<RoomState>().set_name_text(name.into());
        }
    }
}

/// Flash the light's bulb, to tell which one it is.
fn identify(l: &mut Lueur, id: &str) {
    let Some(b) = l.config.binding_for(id).cloned() else {
        let text = t("no_bulb_yet");
        l.inform(text);
        return;
    };
    l.run(
        move |s| service::test_device(s, b.endpoint(), Rgb::new(255, 190, 120)),
        |_, _| {},
    );
}

// --- Announcements ---

/// Moves made with the arrows are read out once the keys rest, so holding
/// an arrow gives one message, not one per step.
fn announce_soon(l: &mut Lueur) {
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::SingleShot,
        Duration::from_millis(600),
        || {
            rt::with(|l| l.pages.room.announcement = where_now(l));
        },
    );
    l.pages.room.announce = Some(timer);
}

fn where_now(l: &Lueur) -> String {
    let Some(p) = selected_pos(l) else {
        return String::new();
    };
    let (x, y) = (p.x.to_string(), p.y.to_string());
    let room = &l.config.room;
    if let Some(id) = selected_light(l)
        && let Some(light) = room.lights.iter().find(|x| x.id == id)
    {
        let m = map_light(&room.screen, light);
        return fill(
            &t("moved_zone"),
            &[
                ("name", &light.name),
                ("x", &x),
                ("y", &y),
                ("letter", &t(plan::letter_key(m.edge))),
                ("zone", &t(plan::kind_key(m.edge))),
            ],
        );
    }
    let name = match l.pages.room.other {
        Some(Other::Corner(i)) => corner_name(&room.walls, i),
        _ => t("plan_screen"),
    };
    fill(&t("moved"), &[("name", &name), ("x", &x), ("y", &y)])
}

/// "Corner 2, top right": where it sits in the room as drawn.
fn corner_name(walls: &[Vec2], i: usize) -> String {
    let n = (i + 1).to_string();
    match plan::corner_where(walls, i) {
        Some(w) => fill(&t("corner_at"), &[("n", &n), ("where", &t(w))]),
        None => fill(&t("corner_n"), &[("n", &n)]),
    }
}

// --- History ---

/// Undo or redo, then let go of what the step took away.
fn travel(l: &mut Lueur, redo: bool) {
    *l.pages.room.settle.borrow_mut() = None;
    let snap = Snap::of(&l.config);
    let step = {
        let mut h = l.pages.room.history.borrow_mut();
        if redo { h.redo(&snap) } else { h.undo(&snap) }
    };
    if let Some(step) = step {
        l.edit(|c| history::restore(c, &step));
        // The history is where it wants to be: the redraw must not see the
        // restored plan as a new change.
        l.pages
            .room
            .history
            .borrow_mut()
            .observe(&Snap::of(&l.config));
    }
    if selected_light(l).is_none() {
        l.selected_light = None;
    }
    let walls = l.config.room.walls.len();
    if let Some(Other::Corner(i)) = l.pages.room.other
        && i >= walls
    {
        l.pages.room.other = None;
    }
}

// --- Keys ---

/// A key the controls did not use. True when the page took it.
fn on_key(l: &mut Lueur, key: &str, shift: bool, ctrl: bool, alt: bool) -> bool {
    use slint::platform::Key;
    let is = |k: Key| key == slint::SharedString::from(k).as_str();
    let selected = selected_key(l).is_some();
    if ctrl && !alt {
        let lower = key.to_lowercase();
        if l.pages.room.drag.is_none() && (lower == "z" || lower == "y") {
            travel(l, lower == "y" || shift);
            return true;
        }
        return false;
    }
    if ctrl || alt {
        return false;
    }
    let step = if shift { 25.0 } else { 5.0 };
    if is(Key::Escape) {
        if !selected {
            return false;
        }
        clear_selection(l, true);
    } else if is(Key::Delete) || is(Key::Backspace) {
        let removable =
            selected_light(l).is_some() || matches!(l.pages.room.other, Some(Other::Corner(_)));
        if !removable {
            return false;
        }
        remove_selected(l, true);
    } else if is(Key::LeftArrow) {
        nudge(l, -step, 0.0);
    } else if is(Key::RightArrow) {
        nudge(l, step, 0.0);
    } else if is(Key::UpArrow) {
        nudge(l, 0.0, -step);
    } else if is(Key::DownArrow) {
        nudge(l, 0.0, step);
    } else {
        return false;
    }
    selected
}

// --- The pointer on the plan ---

/// A press on the plan: grab what is under it, or let go of the selection
/// on the empty floor.
fn press(l: &mut Lueur, x: f32, y: f32) {
    let f = frame(l);
    let p = f.to_plan(x, y);
    let room = l.config.room.clone();
    let screen_selected = l.pages.room.other == Some(Other::Screen);
    let off = |q: Vec2| p - q;
    match plan::hit(&room, p, f.k, screen_selected) {
        Some(Target::Light(id)) => {
            let Some(light) = room.lights.iter().find(|x| x.id == id) else {
                return;
            };
            let off = off(light.pos);
            select_light(l, id.clone());
            begin(l, f.vb, Drag::Light { id, off });
        }
        Some(Target::Corner(index)) => {
            select_other(l, Other::Corner(index));
            let off = off(room.walls[index]);
            begin(l, f.vb, Drag::Corner { index, off });
        }
        Some(Target::Width) => begin(l, f.vb, Drag::Width),
        Some(Target::Screen) => {
            select_other(l, Other::Screen);
            let off = off(room.screen.center);
            begin(l, f.vb, Drag::Screen { off });
        }
        Some(Target::Seat) => begin(l, f.vb, Drag::Seat),
        // A double-click on a wall drops a corner there (see `bind`).
        Some(Target::Wall) => {}
        None => clear_selection(l, false),
    }
}

fn begin(l: &mut Lueur, vb: ViewBox, drag: Drag) {
    // The view is held still during a drag, or the plan would slide under
    // the pointer.
    l.pages.room.held = Some(vb);
    l.pages.room.drag = Some(drag);
    // The effects restart on a changed room only once the drag ends, not
    // at every move.
    l.arranging = true;
}

fn drag_to(l: &mut Lueur, p: Vec2, shift: bool) {
    let Some(drag) = l.pages.room.drag.clone() else {
        return;
    };
    l.edit(|c| {
        let s = &mut c.room.screen;
        match &drag {
            Drag::Light { id, off } => {
                if let Some(light) = c.room.lights.iter_mut().find(|x| x.id == *id) {
                    light.pos = plan::clamp_p(p - *off);
                }
            }
            Drag::Screen { off } => s.center = plan::clamp_p(p - *off),
            Drag::Corner { index, off } => {
                if let Some(w) = c.room.walls.get_mut(*index) {
                    *w = plan::clamp_p(p - *off);
                }
            }
            Drag::Seat => s.facing_deg = plan::facing_toward(s.center, p, shift),
            Drag::Width => s.width = plan::width_toward(s.center, p),
        }
    });
}

fn release(l: &mut Lueur) {
    let Some(drag) = l.pages.room.drag.take() else {
        return;
    };
    // Moving a light, the screen or the seat is arranging the room.
    if matches!(drag, Drag::Light { .. } | Drag::Seat | Drag::Screen { .. }) {
        mark_arranged(l);
    }
    l.pages.room.held = None;
    l.arranging = false;
}

/// Drop a corner on the wall nearest `p`, and select it.
fn insert_corner_at(l: &mut Lueur, p: Vec2) {
    let Some((i, q, _)) = plan::nearest_wall(&l.config.room.walls, p) else {
        return;
    };
    l.edit(|c| c.room.walls.insert(i + 1, plan::clamp_p(q)));
    select_other(l, Other::Corner(i + 1));
}
