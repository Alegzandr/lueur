//! The Room page: the plan of
//! the room on the left, and beside it the list of what it holds or, once
//! something is picked, that thing's inspector.
//!
//! The plan is drawn in [`draw`], the list and the inspector in
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
use crate::state::{Lueur, ToastAction, View, model};
use crate::theme::{size, theme};
use crate::ui::{self, InputEvent, TextInput};
use colors::PlanColors;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Render, Role, SharedString, Subscription, Task, Window, div, prelude::*, px,
};
use history::{History, Snap};
use lueur_core::color::Rgb;
use lueur_core::config::{AppConfig, CaptureSettings, DeviceBinding};
use lueur_core::geometry::{Room, Vec2, map_light};
use lueur_shell::service;
use plan::{Other, Target, ViewBox};
use std::collections::HashMap;
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

/// Where the focus goes once the page has drawn what it lands on.
enum PendingFocus {
    /// The inspector's first control, after a pick from the list.
    Inspector,
    /// Back to the list: the row `key`, else the row now at `index` (what
    /// took a removed object's place).
    List { key: Option<String>, index: usize },
}

/// What the running effect reads only when it starts: the room as drawn
/// (not its paint), which bulb drives which light, and the capture (not its
/// frame rate). Everything else is pushed live.
#[derive(PartialEq)]
struct Topology {
    room: Room,
    bindings: Vec<DeviceBinding>,
    capture: CaptureSettings,
}

fn topology(c: &AppConfig) -> Topology {
    let mut room = c.room.clone();
    room.wall = Rgb::WHITE;
    let mut capture = c.capture;
    capture.target_fps = 0;
    Topology {
        room,
        bindings: c.bindings.clone(),
        capture,
    }
}

/// The fields of the inspector, made for the object picked and remade when
/// the pick changes.
struct Fields {
    /// The selection they are for, as in [`RoomPage::selected_key`].
    key: String,
    name: Option<Entity<TextInput>>,
    x: Entity<TextInput>,
    y: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

impl Fields {
    fn any_focused(&self, window: &Window, cx: &App) -> bool {
        self.name
            .iter()
            .chain([&self.x, &self.y])
            .any(|f| f.focus_handle(cx).is_focused(window))
    }
}

/// What the page reads from the app's state when it draws.
struct Now {
    config: AppConfig,
    selected: Option<String>,
    running: bool,
    busy: bool,
}

pub struct RoomPage {
    lueur: Entity<Lueur>,
    other: Option<Other>,
    drag: Option<Drag>,
    hover: Option<Target>,
    /// The view held still while dragging, and the one drawn last.
    held: Option<ViewBox>,
    drawn: ViewBox,
    /// The stage's size in pixels, from the last frame.
    stage: (f32, f32),
    /// The list of corners stays unfolded while one of them is selected, so
    /// going back lands on its row.
    corners_open: bool,
    /// The exact position, folded under a light's or the screen's card.
    exact_open: bool,
    history: History,
    settle: Option<Task<()>>,
    /// True while the page is the one shown.
    shown: bool,
    started_with: Option<Topology>,
    /// Where a light moved with the arrows went, for screen readers.
    announcement: SharedString,
    announce: Option<Task<()>>,
    plan_focus: FocusHandle,
    inspector_focus: FocusHandle,
    heading_focus: FocusHandle,
    rows: HashMap<String, FocusHandle>,
    pending_focus: Option<PendingFocus>,
    fields: Option<Fields>,
    _observe: Subscription,
}

impl RoomPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lueur = model(cx);
        let observe = cx.observe(&lueur, |this, _, cx| this.on_lueur(cx));
        let shown = lueur.read(cx).view == View::Room;
        let mut history = History::default();
        if shown {
            history.track(&Snap::of(&lueur.read(cx).config));
        }
        RoomPage {
            lueur,
            other: None,
            drag: None,
            hover: None,
            held: None,
            drawn: ViewBox {
                x: 0.0,
                y: 0.0,
                w: plan::VIEW_W,
                h: plan::VIEW_H,
            },
            stage: (0.0, 0.0),
            corners_open: false,
            exact_open: false,
            history,
            settle: None,
            shown,
            started_with: None,
            announcement: SharedString::default(),
            announce: None,
            plan_focus: cx.focus_handle().tab_stop(true),
            inspector_focus: cx.focus_handle(),
            heading_focus: cx.focus_handle(),
            rows: HashMap::new(),
            pending_focus: None,
            fields: None,
            _observe: observe,
        }
    }

    /// The app's state changed: follow the effect, feed the history while
    /// the page shows, and draw again.
    fn on_lueur(&mut self, cx: &mut Context<Self>) {
        let l = self.lueur.read(cx);
        let running = l.status.running;
        let shown = l.view == View::Room;
        let snap = Snap::of(&l.config);
        if !running {
            self.started_with = None;
        } else if self.started_with.is_none() {
            self.started_with = Some(topology(&l.config));
        }
        if shown {
            if !self.shown {
                self.history.track(&snap);
            }
            self.observe_plan(&snap, cx);
        }
        self.shown = shown;
        cx.notify();
    }

    /// The plan may have changed: a step stays open while a drag lasts, and
    /// closes once the plan has rested.
    fn observe_plan(&mut self, snap: &Snap, cx: &mut Context<Self>) {
        self.settle = None;
        if self.history.observe(snap) && self.drag.is_none() {
            self.settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(history::SETTLE).await;
                let _ = this.update(cx, |this, cx| {
                    let snap = Snap::of(&this.lueur.read(cx).config);
                    this.history.commit(&snap);
                    cx.notify();
                });
            }));
        }
    }

    fn now(&self, cx: &App) -> Now {
        let l = self.lueur.read(cx);
        let selected = l
            .selected_light
            .clone()
            .filter(|id| l.config.room.lights.iter().any(|x| x.id == *id));
        Now {
            config: l.config.clone(),
            selected,
            running: l.status.running,
            busy: l.busy,
        }
    }

    fn edit(&self, cx: &mut Context<Self>, change: impl FnOnce(&mut AppConfig)) {
        self.lueur.update(cx, |l, cx| l.edit(cx, change));
    }

    // --- Selection ---

    pub(super) fn selected_light(&self, cx: &App) -> Option<String> {
        let l = self.lueur.read(cx);
        l.selected_light
            .clone()
            .filter(|id| l.config.room.lights.iter().any(|x| x.id == *id))
    }

    /// The list row of what is selected now, e.g. "light:<id>" or "corner:2".
    fn selected_key(&self, cx: &App) -> Option<String> {
        if let Some(id) = self.selected_light(cx) {
            return Some(format!("light:{id}"));
        }
        match self.other {
            Some(Other::Screen) => Some("screen".into()),
            Some(Other::Corner(i)) => Some(format!("corner:{i}")),
            None => None,
        }
    }

    /// The keys of the list's rows, in their order.
    fn row_keys(&self, cx: &App) -> Vec<String> {
        let room = &self.lueur.read(cx).config.room;
        let mut keys: Vec<String> = room
            .lights
            .iter()
            .map(|l| format!("light:{}", l.id))
            .collect();
        keys.push("screen".into());
        if self.corners_open {
            keys.extend((0..room.walls.len()).map(|i| format!("corner:{i}")));
        }
        keys
    }

    pub(super) fn select_light(&mut self, id: String, cx: &mut Context<Self>) {
        self.other = None;
        self.lueur.update(cx, |l, cx| {
            l.selected_light = Some(id);
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn select_other(&mut self, other: Other, cx: &mut Context<Self>) {
        self.lueur.update(cx, |l, cx| {
            l.selected_light = None;
            cx.notify();
        });
        self.other = Some(other);
        if matches!(other, Other::Corner(_)) {
            self.corners_open = true;
        }
        cx.notify();
    }

    /// A pick from the list or an add button: the focus follows it into
    /// the inspector. A pick on the plan with the pointer leaves it alone.
    fn pick_light(&mut self, id: String, cx: &mut Context<Self>) {
        self.select_light(id, cx);
        self.pending_focus = Some(PendingFocus::Inspector);
    }

    fn pick_other(&mut self, other: Other, cx: &mut Context<Self>) {
        self.select_other(other, cx);
        self.pending_focus = Some(PendingFocus::Inspector);
    }

    /// Only take the focus back from the inspector or from nowhere, never
    /// from a control the user has since moved to.
    fn focus_is_ours(&self, window: &Window, cx: &App) -> bool {
        window.focused(cx).is_none() || self.inspector_focus.contains_focused(window, cx)
    }

    /// Back to the list. With `refocus`, the focus returns to the row of
    /// what was selected.
    fn clear_selection(&mut self, refocus: bool, cx: &mut Context<Self>) {
        let key = self.selected_key(cx);
        self.lueur.update(cx, |l, cx| {
            if l.selected_light.take().is_some() {
                cx.notify();
            }
        });
        self.other = None;
        if refocus && key.is_some() {
            self.pending_focus = Some(PendingFocus::List { key, index: 0 });
        }
        cx.notify();
    }

    /// The focus asked for, now that what it lands on is drawn.
    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_focus.take() else {
            return;
        };
        let handle = match pending {
            PendingFocus::Inspector => match &self.fields {
                Some(Fields { name: Some(n), .. }) => Some(n.focus_handle(cx)),
                _ => Some(self.heading_focus.clone()),
            },
            PendingFocus::List { key, index } => {
                let keys = self.row_keys(cx);
                let key = key
                    .filter(|k| keys.contains(k))
                    .or_else(|| keys.get(index.min(keys.len().saturating_sub(1))).cloned());
                key.map(|k| self.row_handle(&k, cx))
            }
        };
        if let Some(handle) = handle {
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
    }

    /// The focus handle of a list row, made the first time it is asked.
    fn row_handle(&mut self, key: &str, cx: &mut Context<Self>) -> FocusHandle {
        self.rows
            .entry(key.to_string())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    // --- Editing ---

    fn new_light(&mut self, cx: &mut Context<Self>) {
        let template = t(cx, "light_n");
        let room = self.lueur.read(cx).config.room.clone();
        let name = plan::fresh_name(&room.lights, |n| fill(&template, &[("n", &n.to_string())]));
        let pos = plan::fresh_positions(&room, 1)[0];
        let light = lueur_core::geometry::Light {
            id: plan::new_light_id(),
            name,
            pos,
            height: 0.5,
            zone_override: None,
            enabled: true,
        };
        let id = light.id.clone();
        self.edit(cx, |c| c.room.lights.push(light));
        self.pick_light(id, cx);
    }

    /// The button's way to add a corner: split the longest wall in two.
    fn add_corner(&mut self, cx: &mut Context<Self>) {
        let walls = self.lueur.read(cx).config.room.walls.clone();
        if walls.len() < 2 {
            return;
        }
        let best = plan::longest_wall(&walls);
        self.edit(cx, |c| {
            let n = c.room.walls.len();
            let (a, b) = (c.room.walls[best], c.room.walls[(best + 1) % n]);
            c.room.walls.insert(
                best + 1,
                Vec2::new(((a.x + b.x) / 2.0).round(), ((a.y + b.y) / 2.0).round()),
            );
        });
        self.pick_other(Other::Corner(best + 1), cx);
    }

    fn remove_corner(&mut self, index: usize, refocus: bool, cx: &mut Context<Self>) {
        if self.lueur.read(cx).config.room.walls.len() <= 3 {
            let text = t(cx, "min_corners");
            self.lueur.update(cx, |l, cx| l.inform(text, cx));
            return;
        }
        let row = self.row_index(&format!("corner:{index}"), cx);
        self.edit(cx, |c| {
            if index < c.room.walls.len() {
                c.room.walls.remove(index);
            }
        });
        self.other = None;
        if refocus {
            self.pending_focus = Some(PendingFocus::List {
                key: None,
                index: row,
            });
        }
        cx.notify();
    }

    /// Remove a light (and its bulb assignment), with an undo in the toast.
    fn remove_light(&mut self, id: &str, refocus: bool, cx: &mut Context<Self>) {
        let row = self.row_index(&format!("light:{id}"), cx);
        let found = {
            let c = &self.lueur.read(cx).config;
            c.room
                .lights
                .iter()
                .position(|l| l.id == id)
                .map(|i| (i, c.room.lights[i].clone(), c.binding_for(id).cloned()))
        };
        let Some((index, light, binding)) = found else {
            return;
        };
        let text = fill(&t(cx, "removed"), &[("name", &light.name)]);
        let undo_label = t(cx, "undo");
        let id = id.to_string();
        self.lueur.update(cx, |l, cx| {
            l.edit(cx, |c| {
                c.room.lights.retain(|x| x.id != id);
                c.bindings.retain(|b| b.light_id != id);
            });
            if l.selected_light.as_deref() == Some(id.as_str()) {
                l.selected_light = None;
            }
            let action: ToastAction = Rc::new(move |_, cx| {
                let (light, binding) = (light.clone(), binding.clone());
                model(cx).update(cx, |l, cx| {
                    l.edit(cx, |c| {
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
            l.inform_with(text, undo_label, action, cx);
        });
        if refocus {
            self.pending_focus = Some(PendingFocus::List {
                key: None,
                index: row,
            });
        }
        cx.notify();
    }

    fn remove_selected(&mut self, refocus: bool, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_light(cx) {
            self.remove_light(&id, refocus, cx);
        } else if let Some(Other::Corner(i)) = self.other {
            self.remove_corner(i, refocus, cx);
        }
    }

    /// Back to the room's first shape: its walls and its screen.
    fn reset_shape(&mut self, cx: &mut Context<Self>) {
        let first = Room::default();
        self.edit(cx, |c| {
            c.room.walls = first.walls;
            c.room.screen = first.screen;
        });
        self.clear_selection(false, cx);
        let text = t(cx, "reset_done");
        self.lueur.update(cx, |l, cx| l.inform(text, cx));
    }

    fn nudge(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let light = self.selected_light(cx);
        let other = self.other;
        let by = |p: Vec2| plan::clamp_p(Vec2::new(p.x + dx, p.y + dy));
        self.edit(cx, |c| {
            if let Some(id) = &light {
                if let Some(l) = c.room.lights.iter_mut().find(|x| x.id == *id) {
                    l.pos = by(l.pos);
                }
            } else if other == Some(Other::Screen) {
                c.room.screen.center = by(c.room.screen.center);
            } else if let Some(Other::Corner(i)) = other
                && let Some(w) = c.room.walls.get_mut(i)
            {
                *w = by(*w);
            }
        });
        self.announce_soon(cx);
    }

    /// Where the selected object is now.
    fn selected_pos(&self, cx: &App) -> Option<Vec2> {
        let room = &self.lueur.read(cx).config.room;
        if let Some(id) = self.selected_light(cx) {
            return room.lights.iter().find(|l| l.id == id).map(|l| l.pos);
        }
        match self.other {
            Some(Other::Screen) => Some(room.screen.center),
            Some(Other::Corner(i)) => room.walls.get(i).copied(),
            None => None,
        }
    }

    /// Move the selected object along one axis, kept on the plan.
    fn set_pos(&mut self, axis: Axis, v: f32, cx: &mut Context<Self>) {
        let light = self.selected_light(cx);
        let other = self.other;
        let put = |p: Vec2| {
            plan::clamp_p(match axis {
                Axis::X => Vec2::new(v, p.y),
                Axis::Y => Vec2::new(p.x, v),
            })
        };
        self.edit(cx, |c| {
            if let Some(id) = &light {
                if let Some(l) = c.room.lights.iter_mut().find(|x| x.id == *id) {
                    l.pos = put(l.pos);
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
            self.mark_arranged(cx);
        }
    }

    /// The plan has been arranged: the first-run guide moves on.
    fn mark_arranged(&mut self, cx: &mut Context<Self>) {
        if !self.lueur.read(cx).config.interface.room_arranged {
            self.edit(cx, |c| c.interface.room_arranged = true);
        }
    }

    fn rename(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let name: String = name.chars().take(plan::MAX_LIGHT_NAME).collect();
        self.edit(cx, |c| {
            if let Some(l) = c.room.lights.iter_mut().find(|x| x.id == id) {
                l.name = name;
            }
        });
    }

    /// Once its field is left, a light without a name gets back the one it
    /// had, or a fresh one: a nameless light would leave its row and its
    /// pin on the plan blank.
    fn settle_name(&mut self, id: &str, before: &str, cx: &mut Context<Self>) {
        let template = t(cx, "light_n");
        let lights = self.lueur.read(cx).config.room.lights.clone();
        let Some(light) = lights.iter().find(|l| l.id == id) else {
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
            self.rename(id, &name, cx);
        }
    }

    /// Flash the light's bulb, to tell which one it is.
    fn identify(&mut self, id: &str, cx: &mut Context<Self>) {
        let binding = self.lueur.read(cx).config.binding_for(id).cloned();
        let Some(b) = binding else {
            let text = t(cx, "no_bulb_yet");
            self.lueur.update(cx, |l, cx| l.inform(text, cx));
            return;
        };
        self.lueur.update(cx, |l, cx| {
            l.run(
                cx,
                move |s| service::test_device(s, b.endpoint(), Rgb::new(255, 190, 120)),
                |_, _, _| {},
            )
        });
    }

    /// Restart the effects so they read the room as drawn now.
    fn restart(&mut self, cx: &mut Context<Self>) {
        if self.lueur.read(cx).busy {
            return;
        }
        // Taken again from the config once the effects run on it.
        self.started_with = None;
        self.lueur.update(cx, |l, cx| {
            l.busy = true;
            cx.notify();
            let config = l.config.clone();
            l.run_then(
                cx,
                move |s| {
                    service::save_config(s, config)?;
                    service::restart_effect(s)
                },
                |l, result, cx| {
                    l.busy = false;
                    match result {
                        Ok(status) => l.status = status,
                        Err(e) => {
                            l.fail(&e, cx);
                            l.refresh_status(cx);
                        }
                    }
                },
            );
        });
    }

    /// Where an object's row sits in the list, to land on its neighbour.
    fn row_index(&self, key: &str, cx: &App) -> usize {
        self.row_keys(cx).iter().position(|k| k == key).unwrap_or(0)
    }

    // --- Announcements ---

    /// Moves made with the arrows are read out once the keys rest, so
    /// holding an arrow gives one message, not one per step.
    fn announce_soon(&mut self, cx: &mut Context<Self>) {
        self.announce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.announcement = this.where_now(cx).into();
                cx.notify();
            });
        }));
    }

    fn where_now(&self, cx: &App) -> String {
        let Some(p) = self.selected_pos(cx) else {
            return String::new();
        };
        let (x, y) = (p.x.to_string(), p.y.to_string());
        let room = &self.lueur.read(cx).config.room;
        if let Some(id) = self.selected_light(cx)
            && let Some(light) = room.lights.iter().find(|l| l.id == id)
        {
            let m = map_light(&room.screen, light);
            return fill(
                &t(cx, "moved_zone"),
                &[
                    ("name", &light.name),
                    ("x", &x),
                    ("y", &y),
                    ("letter", &t(cx, plan::letter_key(m.edge))),
                    ("zone", &t(cx, plan::kind_key(m.edge))),
                ],
            );
        }
        let name = match self.other {
            Some(Other::Corner(i)) => corner_name(&room.walls, i, cx),
            _ => t(cx, "plan_screen").to_string(),
        };
        fill(&t(cx, "moved"), &[("name", &name), ("x", &x), ("y", &y)])
    }

    // --- History ---

    /// Undo or redo, then let go of what the step took away.
    fn travel(&mut self, redo: bool, cx: &mut Context<Self>) {
        self.settle = None;
        let snap = Snap::of(&self.lueur.read(cx).config);
        let step = if redo {
            self.history.redo(&snap)
        } else {
            self.history.undo(&snap)
        };
        if let Some(step) = step {
            self.edit(cx, |c| history::restore(c, &step));
        }
        let walls = self.lueur.read(cx).config.room.walls.len();
        if self.selected_light(cx).is_none() {
            self.lueur.update(cx, |l, _| l.selected_light = None);
        }
        if let Some(Other::Corner(i)) = self.other
            && i >= walls
        {
            self.other = None;
        }
        cx.notify();
    }

    // --- Keys ---

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = e.keystroke.key.as_str();
        let m = &e.keystroke.modifiers;
        let selected = self.selected_key(cx).is_some();
        // Text fields keep their own arrows and keys; Escape still closes
        // the inspector they sit in.
        if self
            .fields
            .as_ref()
            .is_some_and(|f| f.any_focused(window, cx))
        {
            if key == "escape" && selected {
                cx.stop_propagation();
                self.clear_selection(true, cx);
            }
            return;
        }
        if (m.control || m.platform) && !m.alt {
            if self.drag.is_none() && (key == "z" || key == "y") {
                cx.stop_propagation();
                self.travel(key == "y" || m.shift, cx);
            }
            return;
        }
        if m.control || m.platform || m.alt {
            return;
        }
        let step = if m.shift { 25.0 } else { 5.0 };
        match key {
            "escape" => {
                let ours = self.focus_is_ours(window, cx);
                self.clear_selection(ours, cx);
            }
            "delete" | "backspace" => {
                let removable = self.selected_light(cx).is_some()
                    || matches!(self.other, Some(Other::Corner(_)));
                if !removable {
                    return;
                }
                let ours = self.focus_is_ours(window, cx);
                self.remove_selected(ours, cx);
            }
            "left" => self.nudge(-step, 0.0, cx),
            "right" => self.nudge(step, 0.0, cx),
            "up" => self.nudge(0.0, -step, cx),
            "down" => self.nudge(0.0, step, cx),
            _ => return,
        }
        if selected {
            cx.stop_propagation();
        }
    }

    // --- Fields ---

    /// Make the inspector's fields for what is picked, or bring the ones
    /// there up to date with a move made elsewhere (the plan, the arrows).
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.selected_key(cx);
        let pos = self.selected_pos(cx);
        let (Some(key), Some(pos)) = (key, pos) else {
            self.fields = None;
            return;
        };
        if self.fields.as_ref().is_none_or(|f| f.key != key) {
            self.fields = Some(self.make_fields(key, pos, window, cx));
            return;
        }
        let Some(f) = &self.fields else { return };
        for (input, v) in [(&f.x, pos.x), (&f.y, pos.y)] {
            let text = v.to_string();
            if !input.focus_handle(cx).is_focused(window) && input.read(cx).text() != text {
                input.update(cx, |i, cx| i.set_text(text, cx));
            }
        }
    }

    fn make_fields(
        &mut self,
        key: String,
        pos: Vec2,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Fields {
        let mut subs = Vec::new();
        let mut axis_field = |axis: Axis, label: &str, v: f32, cx: &mut Context<Self>| {
            let label = t(cx, label);
            let input = cx.new(|cx| TextInput::new(cx, v.to_string(), label, "", 4));
            subs.push(
                cx.subscribe(&input, move |this, input, e: &InputEvent, cx| match e {
                    // A value is taken while typing only once it is in bounds,
                    // so typing "420" does not stop at 12 on the way.
                    InputEvent::Changed => {
                        let max = if axis == Axis::X {
                            plan::X_MAX
                        } else {
                            plan::Y_MAX
                        };
                        if let Ok(v) = input.read(cx).text().trim().parse::<f32>()
                            && (plan::MARGIN..=max).contains(&v)
                        {
                            this.set_pos(axis, v, cx);
                        }
                    }
                    InputEvent::Submit => this.settle_pos(axis, cx),
                }),
            );
            let handle = input.focus_handle(cx);
            subs.push(cx.on_blur(&handle, window, move |this, _, cx| {
                this.settle_pos(axis, cx)
            }));
            input
        };
        let x = axis_field(Axis::X, "pos_x", pos.x, cx);
        let y = axis_field(Axis::Y, "pos_y", pos.y, cx);

        let name = key.strip_prefix("light:").map(|id| {
            let id = id.to_string();
            let before = self
                .lueur
                .read(cx)
                .config
                .room
                .lights
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.name.clone())
                .unwrap_or_default();
            let label = t(cx, "light_name");
            let input =
                cx.new(|cx| TextInput::new(cx, before.clone(), label, "", plan::MAX_LIGHT_NAME));
            let changed_id = id.clone();
            let submit_id = id.clone();
            let submit_before = before.clone();
            subs.push(
                cx.subscribe(&input, move |this, input, e: &InputEvent, cx| match e {
                    InputEvent::Changed => {
                        let text = input.read(cx).text().to_string();
                        this.rename(&changed_id, &text, cx);
                    }
                    InputEvent::Submit => this.settle_name(&submit_id, &submit_before, cx),
                }),
            );
            let handle = input.focus_handle(cx);
            subs.push(cx.on_blur(&handle, window, move |this, _, cx| {
                this.settle_name(&id, &before, cx)
            }));
            input
        });

        Fields {
            key,
            name,
            x,
            y,
            _subscriptions: subs,
        }
    }

    /// Leaving a position field: take what it says, kept on the plan, and
    /// show what was kept (the clamped value, or the old one for no number).
    fn settle_pos(&mut self, axis: Axis, cx: &mut Context<Self>) {
        let Some(f) = &self.fields else { return };
        let input = if axis == Axis::X {
            f.x.clone()
        } else {
            f.y.clone()
        };
        if let Ok(v) = input.read(cx).text().trim().parse::<f32>()
            && v.is_finite()
        {
            self.set_pos(axis, v, cx);
        }
        if let Some(p) = self.selected_pos(cx) {
            let v = if axis == Axis::X { p.x } else { p.y };
            input.update(cx, |i, cx| i.set_text(v.to_string(), cx));
        }
    }

    // --- Pieces of the page ---

    fn header(&self, theme: &crate::theme::Theme, cx: &mut Context<Self>) -> AnyElement {
        let (can_undo, can_redo) = (self.history.can_undo(), self.history.can_redo());
        let mac = cfg!(target_os = "macos");
        let undo_tip = format!("{} ({})", t(cx, "undo"), if mac { "⌘Z" } else { "Ctrl+Z" });
        let redo_tip = format!("{} ({})", t(cx, "redo"), if mac { "⇧⌘Z" } else { "Ctrl+Y" });
        let page = cx.entity().downgrade();
        let history_button = |id: &'static str,
                              icon: &'static str,
                              label: SharedString,
                              tip: String,
                              on: bool,
                              redo: bool| {
            let page = page.clone();
            ui::pressable(id, theme)
                .role(Role::Button)
                .aria_label(label)
                .tooltip(ui::tip(tip.into()))
                .size(px(32.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(size::R_MD))
                .bg(theme.surface)
                .border_1()
                .border_color(theme.line)
                .when(on, |d| d.hover(|s| s.bg(theme.surface_hover)))
                .when(!on, |d| d.opacity(0.4).cursor_default())
                .child(ui::icon(icon, 15.0, false, theme.text))
                .on_click(move |_, _, cx| {
                    if on {
                        let _ = page.update(cx, |p, cx| p.travel(redo, cx));
                    }
                })
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(size::S4))
            .min_h(px(34.0))
            .child(ui::page_title("page-title", t(cx, "nav_room"), theme))
            .child(
                div()
                    .flex()
                    .gap(px(size::S1))
                    .child(history_button(
                        "room-undo",
                        "undo",
                        t(cx, "undo"),
                        undo_tip,
                        can_undo,
                        false,
                    ))
                    .child(history_button(
                        "room-redo",
                        "redo",
                        t(cx, "redo"),
                        redo_tip,
                        can_redo,
                        true,
                    )),
            )
            .into_any_element()
    }

    /// A line above the plan: a lamp dot (it waits on you), the words and
    /// one button.
    fn banner(
        &self,
        id: &'static str,
        text: SharedString,
        action: SharedString,
        theme: &crate::theme::Theme,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        ui::card(theme)
            .flex_row()
            .items_center()
            .gap(px(size::S4))
            .py(px(size::S2))
            .pl(px(size::S4))
            .pr(px(size::S2))
            .child(lamp(theme))
            .child(
                div()
                    .flex_1()
                    .text_size(px(size::T_MD))
                    .text_color(theme.text)
                    .child(text),
            )
            .child(ui::button(id, action, true, theme, on_click))
            .into_any_element()
    }
}

/// The amber lamp dot: a light waiting on you.
fn lamp(theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .size(px(8.0))
        .flex_none()
        .rounded_full()
        .bg(theme.glow)
}

/// "Corner 2, top right": where it sits in the room as drawn.
fn corner_name(walls: &[Vec2], i: usize, cx: &App) -> String {
    let n = (i + 1).to_string();
    match plan::corner_where(walls, i) {
        Some(w) => fill(&t(cx, "corner_at"), &[("n", &n), ("where", &t(cx, w))]),
        None => fill(&t(cx, "corner_n"), &[("n", &n)]),
    }
}

impl Render for RoomPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let colors = PlanColors::of(&theme);
        self.sync_fields(window, cx);
        self.apply_pending_focus(window, cx);
        let now = self.now(cx);

        // The plan takes the height the window gives the page, less its
        // title and margins, so it fills the window.
        let body_h = (f32::from(window.viewport_size().height) - 132.0).max(440.0);
        let coach = (!now.config.interface.room_arranged && !now.config.room.lights.is_empty())
            .then(|| {
                let page = cx.entity().downgrade();
                self.banner(
                    "room-coach",
                    t(cx, "coach"),
                    t(cx, "coach_done"),
                    &theme,
                    move |_, _, cx| {
                        let _ = page.update(cx, |p, cx| p.mark_arranged(cx));
                    },
                )
            });
        let restart = (now.running
            && self
                .started_with
                .as_ref()
                .is_some_and(|s| *s != topology(&now.config)))
        .then(|| {
            let page = cx.entity().downgrade();
            let busy = now.busy;
            self.banner(
                "room-restart",
                t(cx, "restart_text"),
                t(cx, "apply"),
                &theme,
                move |_, _, cx| {
                    if !busy {
                        let _ = page.update(cx, |p, cx| p.restart(cx));
                    }
                },
            )
        });
        let header = self.header(&theme, cx);
        let stage = self.plan(&now, &theme, &colors, cx);
        let side = if now.selected.is_some() || self.other.is_some() {
            self.inspector(&now, &theme, &colors, window, cx)
        } else {
            self.objects(&now, &theme, &colors, cx)
        };

        div()
            .id("room-page")
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .flex_col()
            .gap(px(size::S3))
            .child(header)
            .child(
                div()
                    .flex()
                    .gap(px(size::S5))
                    .h(px(body_h))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(size::S3))
                            .children(coach)
                            .children(restart)
                            .child(stage),
                    )
                    .child(
                        div()
                            .id("room-inspector")
                            .track_focus(&self.inspector_focus)
                            .w(px(296.0))
                            .flex_none()
                            .h_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .child(side),
                    ),
            )
            .child(
                div()
                    .id("room-announcement")
                    .role(Role::Status)
                    .aria_label(self.announcement.clone())
                    .size_0()
                    .overflow_hidden(),
            )
            .text_color(theme.text)
            .font_weight(FontWeight::NORMAL)
    }
}
