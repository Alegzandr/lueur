//! The Lighting page: the scenes saved, folding away, then the room's
//! lights as cards, each with its own switch, under the switch for every
//! light and the sleep timer; and after them, or beside them, the settings
//! (the modes, brightness, white and colour) for the whole room or for the
//! one light whose card was clicked, with a getting-started card until the
//! room is set up.
//!
//! The page also holds the two backdrops the window draws under itself
//! (`backdrop.rs`), and restarts the running effects once the room they
//! play has changed and rested ([`actions::follow_room`]).

mod actions;
mod backdrop;
mod controls;
mod glow;
mod lights;
mod model;
mod modes;
mod scenes;
mod sleep;
mod target;
mod tune;

use crate::i18n::t;
use crate::state::{Lueur, View};
use crate::theme::{Theme, size, theme};
use crate::ui::{self, TextInput};
use backdrop::{GlowBackdrop, RoomBackdrop};
use controls::{Kind, btn, group_title};
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, IntoElement, Render, Role,
    Subscription, Task, Window, canvas, div, prelude::*, px,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The widest the page grows: two columns once the window has the room.
const MAX_WIDTH: f32 = 1240.0;
/// From this window width, the lights and the settings sit side by side.
const TWO_COLUMNS: f32 = 1200.0;
/// The gap between the two columns (`--s-7`).
const COLUMN_GAP: f32 = 40.0;

pub struct LightingPage {
    lueur: Entity<Lueur>,
    glow: Entity<GlowBackdrop>,
    room_photo: Entity<RoomBackdrop>,
    /// The page's width at the last frame, which sets its columns.
    width: f32,
    /// The room was just arranged for the first time: the getting-started
    /// card stays for its last step, starting Screen, until the lights
    /// follow it.
    last_step_pending: bool,
    arranged: bool,
    /// The field a scene is being named in, while it is.
    naming: Option<Entity<TextInput>>,
    /// The scene whose deletion is being asked about.
    confirming: Option<String>,
    save_focus: FocusHandle,
    confirm_focus: FocusHandle,
    /// The last scene asked about and kept, whose delete mark takes the
    /// keyboard back.
    asked: Option<String>,
    delete_focus: FocusHandle,
    /// The sleep timer's panel, open or not, and its field for any other
    /// duration.
    sleep_open: bool,
    sleep_field: Option<Entity<TextInput>>,
    sleep_focus: FocusHandle,
    sleep_first: FocusHandle,
    /// Where the clock was drawn, which the panel hangs from.
    sleep_at: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// When the panel last closed on a click outside it, so the same click
    /// on the clock does not open it again.
    sleep_closed_at: Option<Instant>,
    /// What the lights were last given in the other tint, so switching
    /// between White and Colour comes back to it.
    last_kelvin: u16,
    last_colour: [u8; 3],
    /// The hue slider for a colour of one's own, open or not.
    custom_colour: bool,
    _subscriptions: Vec<Subscription>,
    _ticker: Task<()>,
}

impl LightingPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lueur = super::watch(cx);
        let glow = cx.new(|cx| GlowBackdrop::new(lueur.clone(), cx));
        let room_photo = cx.new(|cx| RoomBackdrop::new(lueur.clone(), cx));
        let arranged = lueur.read(cx).config.interface.room_arranged;
        let watch = cx.observe(&lueur, |this, lueur, cx| {
            actions::follow_room(&lueur, cx);
            let l = lueur.read(cx);
            let arranged = l.config.interface.room_arranged;
            if arranged && !this.arranged {
                this.last_step_pending = true;
            }
            this.arranged = arranged;
            // Once the lights have followed the screen, the guide has done
            // its work, even if they stop.
            if l.status.running {
                this.last_step_pending = false;
            }
        });
        actions::follow_room(&lueur, cx);
        // The sleep timer counts down by the second while one runs.
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| {
                    let l = this.lueur.read(cx);
                    if l.sleep.is_some() && l.view == View::Lighting {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        LightingPage {
            lueur,
            glow,
            room_photo,
            width: 0.0,
            last_step_pending: false,
            arranged,
            naming: None,
            confirming: None,
            save_focus: cx.focus_handle(),
            confirm_focus: cx.focus_handle(),
            asked: None,
            delete_focus: cx.focus_handle(),
            sleep_open: false,
            sleep_field: None,
            sleep_focus: cx.focus_handle(),
            sleep_first: cx.focus_handle(),
            sleep_at: Rc::new(Cell::new(None)),
            sleep_closed_at: None,
            last_kelvin: 2700,
            last_colour: model::PALETTE[1].0,
            custom_colour: false,
            _subscriptions: vec![watch],
            _ticker: ticker,
        }
    }

    /// What the window draws under itself: the glow of the lights behind
    /// this page, the room's photograph behind the others.
    pub fn backdrop(&self, cx: &App) -> AnyElement {
        if self.lueur.read(cx).view == View::Lighting {
            self.glow.clone().into_any_element()
        } else {
            self.room_photo.clone().into_any_element()
        }
    }

    /// The page's own width, measured as it is drawn: the columns and grids
    /// count from it.
    fn measure(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity().downgrade();
        let known = self.width;
        canvas(
            move |bounds, _, cx| {
                let w = f32::from(bounds.size.width);
                if (w - known).abs() > 0.5 {
                    let _ = page.update(cx, |p, cx| {
                        p.width = w;
                        cx.notify();
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h(px(1.0))
    }

    /// The page's head: its title. The switch for every light and the sleep
    /// timer sit over the lights themselves.
    fn head(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let l = self.lueur.read(cx);
        let bound = l.config.bindings.len();
        let live = l.status.running;
        let busy = l.busy;
        let lueur = self.lueur.clone();
        let tools = if bound == 0 && live {
            // Every bulb removed while an effect plays: the one way to stop
            // it left on the page, beside Ctrl+Enter and the tray.
            let label = t(cx, "stop_effects");
            Some(
                btn("stop-effects", Kind::Default, false, busy, theme)
                    .aria_label(label.clone())
                    .child(label)
                    .on_click(move |_, _, cx| lueur.update(cx, |l, cx| l.toggle_effect(cx)))
                    .into_any_element(),
            )
        } else {
            None
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(size::S4))
            .min_h(px(34.0))
            .mb(px(size::S5))
            .child(ui::page_title("page-title", t(cx, "nav_lighting"), theme))
            .children(tools)
            .into_any_element()
    }

    /// The getting-started card: the room on the left, lighting up as the
    /// steps are done, and the steps beside it.
    fn setup(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let l = self.lueur.read(cx);
        let bound = l.config.bindings.len();
        let arranged = l.config.interface.room_arranged;
        let live = l.status.running;
        let busy = l.busy;
        if bound > 0 && arranged && !self.last_step_pending {
            return None;
        }
        let steps = [
            (bound > 0, "step1_title", "step1_btn"),
            (arranged, "step2_title", "step2_btn"),
            (live, "step3_title", "start_ambilight"),
        ];
        let next = steps.iter().position(|s| !s.0);
        // The room as it will be once the next step is done: dark until
        // there are bulbs, lit softly once they are in, then following the
        // screen.
        let picture = match next {
            Some(0) | None => "off",
            Some(1) => "relax",
            _ => "ambilight",
        };
        let lueur = self.lueur.clone();
        let rows = steps.iter().enumerate().map(|(i, &(done, title, action))| {
            let is_next = Some(i) == next;
            let mark = div()
                .flex_none()
                .size(px(20.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(size::T_XS))
                .font_weight(FontWeight::SEMIBOLD)
                .map(|d| {
                    if done {
                        d.bg(theme.ok)
                            .child(ui::icon("check", 12.0, false, gpui::white()))
                    } else if is_next {
                        d.border_2()
                            .border_color(theme.text)
                            .child((i + 1).to_string())
                    } else {
                        d.border_1()
                            .border_color(theme.line_strong)
                            .child((i + 1).to_string())
                    }
                });
            let title = t(cx, title);
            let lueur = lueur.clone();
            let disabled = i == 2 && (busy || bound == 0);
            div()
                .flex()
                .items_center()
                .gap(px(size::S3))
                .min_h(px(48.0))
                .pl(px(size::S4))
                .pr(px(size::S2))
                .text_color(if is_next { theme.text } else { theme.text_3 })
                .child(mark)
                .child(
                    div()
                        .flex_1()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(size::T_MD))
                        .when(done, |d| {
                            d.line_through().text_decoration_color(theme.line_strong)
                        })
                        .child(title),
                )
                .when(is_next, |d| {
                    let label = t(cx, action);
                    d.child(
                        btn(("step", i), Kind::Primary, false, disabled, theme)
                            .aria_label(label.clone())
                            .child(label)
                            .when(!disabled, |b| {
                                b.on_click(move |_, _, cx| {
                                    lueur.update(cx, |l, cx| match i {
                                        0 => l.show(View::Bulbs, cx),
                                        1 => l.show(View::Room, cx),
                                        _ => l.choose_mode("ambilight", cx),
                                    })
                                })
                            }),
                    )
                })
        });
        let pic_w = (self.width * 0.4).min(300.0);
        Some(
            div()
                .flex()
                .flex_col()
                .mb(px(size::S6))
                .child(
                    div()
                        .pb(px(size::S2))
                        .px(px(size::S1))
                        .child(group_title(t(cx, "setup_title"), theme)),
                )
                .child(
                    ui::card(theme)
                        .flex_row()
                        .items_center()
                        .gap(px(size::S2))
                        .p(px(5.0))
                        .child(
                            controls::picture(
                                model::picture_of(picture),
                                Rc::new(Cell::new(1.0)),
                                None,
                                size::R_LG - 5.0,
                            )
                            .flex_none()
                            .w(px(pic_w))
                            .h(px(150.0))
                            .bg(theme.sunken),
                        )
                        .child(
                            div()
                                .id("setup-steps")
                                .role(Role::List)
                                .flex_1()
                                .flex()
                                .flex_col()
                                .children(rows),
                        ),
                )
                .into_any_element(),
        )
    }
}

impl Render for LightingPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let bound = self.lueur.read(cx).config.bindings.len();
        let two = f32::from(window.viewport_size().width) >= TWO_COLUMNS;
        let content = self.width.min(MAX_WIDTH);
        let (left_w, right_w) = if two {
            let w = (content - COLUMN_GAP).max(0.0);
            ((w * 5.0 / 12.0).max(340.0), (w * 7.0 / 12.0).min(w - 340.0))
        } else {
            (content, content)
        };
        let head = self.head(&theme, cx);
        let setup = self.setup(&theme, cx);
        let lights = self.lights(left_w, &theme, window, cx);
        let scenes = (bound > 0).then(|| self.scenes(left_w, &theme, window, cx));
        let side = (bound > 0).then(|| {
            div()
                .id("settings-column")
                .role(Role::Group)
                .flex()
                .flex_col()
                .when(two, |d| d.w(px(right_w)).flex_none())
                .child(self.targets(&theme, cx))
                .child(self.modes(right_w, &theme, window, cx))
                .children(self.tune(right_w, &theme, window, cx))
                .into_any_element()
        });
        let left = div()
            .flex()
            .flex_col()
            .gap(px(size::S6))
            .when(two, |d| d.w(px(left_w)).flex_none())
            .children(scenes)
            .child(lights);
        div()
            .relative()
            .w_full()
            .max_w(px(MAX_WIDTH))
            .mx_auto()
            .flex()
            .flex_col()
            .child(self.measure(cx))
            .child(head)
            .children(setup)
            .child(
                div()
                    .flex()
                    .when(two, |d| d.flex_row().items_start().gap(px(COLUMN_GAP)))
                    .when(!two, |d| d.flex_col().gap(px(size::S6)))
                    .child(left)
                    .children(side),
            )
    }
}
