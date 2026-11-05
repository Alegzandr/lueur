//! The column beside the plan: the list of what the room holds, the
//! inspector of what is selected, its exact position, and the zone badges.
//!
//! With nothing selected it lists what the room holds, in the order it is
//! set up: the lights first, each saying which part of the picture it
//! mirrors, so the letters on the plan read without a legend; then the
//! screen; then the walls, their paint and, folded away, their corners.
//! Once something is picked it shows that thing: what is set by hand first,
//! in one card; the exact position, which the plan already gives by
//! dragging, folded under it.

use super::colors::PlanColors;
use super::plan::{self, Other};
use super::{Now, RoomPage, corner_name, lamp};
use crate::i18n::{Lang, fill, pct, t};
use crate::state::View;
use crate::theme::{Theme, size};
use crate::ui;
use crate::views::bulbs::brands::bulb_label;
use gpui::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, IntoElement,
    KeyDownEvent, Rgba, Role, SharedString, Stateful, Window, div, prelude::*, px,
};
use lueur_core::color::Rgb;
use lueur_core::geometry::{Light, LightMapping, ScreenEdge, map_light};

/// The wall paints offered.
const WALLS: [(&str, Rgb); 6] = [
    ("white", Rgb::new(255, 255, 255)),
    ("cream", Rgb::new(250, 240, 215)),
    ("beige", Rgb::new(235, 215, 180)),
    ("blue", Rgb::new(190, 210, 235)),
    ("green", Rgb::new(195, 220, 195)),
    ("pink", Rgb::new(240, 205, 205)),
];

fn rgb(c: Rgb) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.0,
        g: c.g as f32 / 255.0,
        b: c.b as f32 / 255.0,
        a: 1.0,
    }
    .into()
}

/// The mark of a screen zone: its colour with its letter on it, so the zone
/// reads without telling colours apart. A light left out of effects is
/// grey, the letter still there.
pub(super) fn zone_badge(
    edge: ScreenEdge,
    off: bool,
    diameter: f32,
    colors: &PlanColors,
    theme: &Theme,
    cx: &App,
) -> Div {
    let (bg, ink) = if off {
        (theme.line_strong, theme.text)
    } else {
        (colors.zone(edge), colors.zone_ink)
    };
    div()
        .size(px(diameter))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(bg)
        .text_color(ink)
        .text_size(px(diameter * 0.6))
        .font_weight(FontWeight::BOLD)
        .line_height(px(diameter))
        .child(t(cx, plan::letter_key(edge)))
}

/// "zone L, left side", for screen readers.
fn zone_words(edge: ScreenEdge, cx: &App) -> String {
    fill(
        &t(cx, "zone_of"),
        &[
            ("letter", &t(cx, plan::letter_key(edge))),
            ("zone", &t(cx, plan::kind_key(edge))),
        ],
    )
}

/// A group's small title over its card.
fn group_title(text: SharedString, count: Option<usize>, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_h(px(24.0))
        .ml(px(size::S1))
        .mb(px(size::S2))
        .child(ui::section(text, theme))
        .when_some(count, |d, n| {
            d.child(
                div()
                    .min_w(px(18.0))
                    .h(px(16.0))
                    .px(px(5.0))
                    .rounded_full()
                    .bg(theme.surface_hover)
                    .text_color(theme.text_2)
                    .text_size(px(size::T_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(n.to_string()),
            )
        })
}

/// The small square that holds a row's mark.
fn mark(child: impl IntoElement, theme: &Theme) -> Div {
    div()
        .size(px(26.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .bg(theme.surface_2)
        .border_1()
        .border_color(theme.separator)
        .child(child)
}

fn chevron(theme: &Theme) -> impl IntoElement {
    ui::icon("chevron", 12.0, false, theme.text_3)
}

/// The chevron of a fold: pointing down while folded, up while open.
fn folded_chevron(open: bool, theme: &Theme) -> impl IntoElement {
    let turn = std::f32::consts::FRAC_PI_2 * if open { -1.0 } else { 1.0 };
    ui::icon("chevron", 12.0, false, theme.text_3)
        .with_transformation(gpui::Transformation::rotate(gpui::radians(turn)))
}

/// A row of a list card: pressed, it opens what it names.
fn item(id: impl Into<ElementId>, label: SharedString, theme: &Theme) -> Stateful<Div> {
    ui::pressable(id, theme)
        .role(Role::Button)
        .aria_label(label)
        .w_full()
        .min_h(px(44.0))
        .flex()
        .items_center()
        .gap(px(10.0))
        .pl(px(6.0))
        .pr(px(8.0))
        .py(px(5.0))
        .rounded(px(size::R_LG - 3.0))
        .hover(|s| s.bg(theme.surface_hover))
        .text_size(px(size::T_MD))
        .text_color(theme.text)
}

/// A hairline between rows that starts at their text, not at the card's
/// edge, as on the platforms' own inspectors.
fn rule(theme: &Theme, inset: f32) -> Div {
    div()
        .h(px(1.0))
        .ml(px(inset))
        .mr(px(8.0))
        .bg(theme.separator)
}

/// The name and the line under it.
fn two_lines(name: SharedString, meta: SharedString, name_ink: Hsla, theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .child(
            div()
                .truncate()
                .text_color(name_ink)
                .font_weight(FontWeight::MEDIUM)
                .child(name),
        )
        .child(
            div()
                .truncate()
                .text_size(px(size::T_SM))
                .text_color(theme.text_3)
                .child(meta),
        )
}

/// A quiet, small button with an icon.
fn plain_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: SharedString,
    ink: Hsla,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    ui::pressable(id, theme)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(28.0))
        .px(px(size::S2))
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(size::R_MD))
        .hover(|s| s.bg(theme.surface_hover))
        .text_size(px(size::T_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink)
        .child(ui::icon(icon, 13.0, false, ink))
        .child(label)
        .on_click(on_click)
}

/// A field's small heading.
fn field_head(text: SharedString, theme: &Theme) -> Div {
    div()
        .mb(px(4.0))
        .text_size(px(size::T_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.text_2)
        .child(text)
}

fn hint(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .mt(px(size::S2))
        .text_size(px(size::T_XS))
        .text_color(theme.text_3)
        .line_height(px(16.0))
        .child(text.into())
}

impl RoomPage {
    // --- The list ---

    pub(super) fn objects(
        &mut self,
        now: &Now,
        theme: &Theme,
        colors: &PlanColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let room = &now.config.room;
        let page = cx.entity().downgrade();

        // Lights.
        let mut lights: Vec<AnyElement> = Vec::new();
        for (i, light) in room.lights.iter().enumerate() {
            let m = map_light(&room.screen, light);
            let bound = now.config.binding_for(&light.id).is_some();
            let meta = if light.enabled {
                t(cx, plan::kind_key(m.edge))
            } else {
                t(cx, "left_out")
            };
            let label = format!(
                "{}, {}{}",
                light.name,
                if light.enabled {
                    zone_words(m.edge, cx)
                } else {
                    meta.to_string()
                },
                if bound {
                    String::new()
                } else {
                    format!(", {}", t(cx, "no_bulb"))
                }
            );
            let key = format!("light:{}", light.id);
            let handle = self.row_handle(&key, cx);
            let id = light.id.clone();
            let page = page.clone();
            if i > 0 {
                lights.push(rule(theme, 40.0).into_any_element());
            }
            lights.push(
                item(("room-light", i), label.into(), theme)
                    .track_focus(&handle)
                    .child(mark(
                        zone_badge(m.edge, !light.enabled, 18.0, colors, theme, cx),
                        theme,
                    ))
                    .child(two_lines(
                        light.name.clone().into(),
                        meta,
                        if light.enabled {
                            theme.text
                        } else {
                            theme.text_2
                        },
                        theme,
                    ))
                    .when(!bound, |d| {
                        d.child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(6.0))
                                .text_size(px(size::T_XS))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.glow_text)
                                .child(div().size(px(6.0)).rounded_full().bg(theme.glow))
                                .child(t(cx, "no_bulb")),
                        )
                    })
                    .child(chevron(theme))
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        let _ = page.update(cx, |p, cx| p.pick_light(id, cx));
                    })
                    .into_any_element(),
            );
        }
        if room.lights.is_empty() {
            lights.push(
                div()
                    .pl(px(42.0))
                    .pr(px(size::S3))
                    .pt(px(size::S3))
                    .pb(px(size::S2))
                    .text_size(px(size::T_SM))
                    .text_color(theme.text_3)
                    .child(t(cx, "no_lights"))
                    .into_any_element(),
            );
        }
        let add_page = page.clone();
        lights.push(rule(theme, 40.0).into_any_element());
        lights.push(
            item("room-add-light", t(cx, "add_light"), theme)
                .min_h(px(38.0))
                .text_color(theme.text_2)
                .child(
                    div()
                        .size(px(26.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::icon("plus", 14.0, false, theme.text_2)),
                )
                .child(t(cx, "add_light"))
                .on_click(move |_, _, cx| {
                    let _ = add_page.update(cx, |p, cx| p.new_light(cx));
                })
                .into_any_element(),
        );

        // The screen.
        let screen_meta = fill(
            &t(cx, "screen_meta"),
            &[
                ("w", &room.screen.width.round().to_string()),
                ("deg", &room.screen.facing_deg.to_string()),
            ],
        );
        let screen_handle = self.row_handle("screen", cx);
        let screen_page = page.clone();
        let screen = item(
            "room-screen",
            format!("{}, {screen_meta}", t(cx, "plan_screen")).into(),
            theme,
        )
        .track_focus(&screen_handle)
        .child(mark(ui::icon("screen", 16.0, false, theme.text_2), theme))
        .child(two_lines(
            t(cx, "plan_screen"),
            screen_meta.into(),
            theme.text,
            theme,
        ))
        .child(chevron(theme))
        .on_click(move |_, _, cx| {
            let _ = screen_page.update(cx, |p, cx| p.pick_other(Other::Screen, cx));
        });

        // The walls: their paint, then their corners folded away.
        let walls = self.walls_card(now, theme, cx);

        div()
            .flex()
            .flex_col()
            .child(group_title(
                t(cx, "lights"),
                (!room.lights.is_empty()).then_some(room.lights.len()),
                theme,
            ))
            .child(ui::card(theme).p(px(3.0)).children(lights))
            .child(
                div()
                    .mt(px(size::S5))
                    .child(group_title(t(cx, "plan_screen"), None, theme))
                    .child(ui::card(theme).p(px(3.0)).child(screen)),
            )
            .child(
                div()
                    .mt(px(size::S5))
                    .child(group_title(t(cx, "walls_title"), None, theme))
                    .child(walls),
            )
            .into_any_element()
    }

    fn walls_card(&mut self, now: &Now, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let room = &now.config.room;
        let page = cx.entity().downgrade();
        let picked = WALLS.iter().position(|(_, c)| *c == room.wall);

        // The paints are a radio group: Tab lands on it, the arrows move the
        // choice. A paint of the user's own shows as one more swatch, picked.
        let swatch = |id: ElementId, color: Hsla, label: SharedString, on: bool| {
            div()
                .id(id)
                .role(Role::RadioButton)
                .aria_label(label.clone())
                .aria_selected(on)
                .tooltip(ui::tip(label))
                .size(px(30.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .when(on, |d| d.border_2().border_color(theme.select))
                .child(
                    div()
                        .size(px(if on { 21.0 } else { 24.0 }))
                        .rounded_full()
                        .bg(color)
                        .border_1()
                        .border_color(gpui::black().opacity(0.16)),
                )
        };
        let mut swatches: Vec<AnyElement> = WALLS
            .iter()
            .enumerate()
            .map(|(i, (name, color))| {
                let color = *color;
                let page = page.clone();
                swatch(
                    ElementId::NamedInteger("room-wall".into(), i as u64),
                    rgb(color),
                    t(cx, &format!("wall_{name}")),
                    picked == Some(i),
                )
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let _ = page.update(cx, |p, cx| p.edit(cx, |c| c.room.wall = color));
                })
                .into_any_element()
            })
            .collect();
        if picked.is_none() {
            swatches.push(
                swatch(
                    "room-wall-own".into(),
                    rgb(room.wall),
                    t(cx, "wall_custom"),
                    true,
                )
                .into_any_element(),
            );
        }
        let keys_page = page.clone();
        let from = picked.unwrap_or(0);
        let paints = ui::pressable("room-walls", theme)
            .role(Role::RadioGroup)
            .aria_label(t(cx, "walls"))
            .cursor_default()
            .rounded(px(size::R_MD))
            .flex()
            .justify_between()
            .gap(px(4.0))
            .on_key_down(move |e: &KeyDownEvent, _, cx| {
                let last = WALLS.len() - 1;
                let to = match e.keystroke.key.as_str() {
                    "right" | "down" => (from + 1) % WALLS.len(),
                    "left" | "up" => {
                        if from == 0 {
                            last
                        } else {
                            from - 1
                        }
                    }
                    "home" => 0,
                    "end" => last,
                    _ => return,
                };
                cx.stop_propagation();
                let color = WALLS[to].1;
                let _ = keys_page.update(cx, |p, cx| p.edit(cx, |c| c.room.wall = color));
            })
            .children(swatches);

        // The corners, folded.
        let open = self.corners_open;
        let fold_page = page.clone();
        let summary = item("room-corners", t(cx, "corners"), theme)
            .aria_expanded(open)
            .child(
                div()
                    .size(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(10.0))
                            .rounded(px(2.0))
                            .bg(PlanColors::of(theme).floor)
                            .border_1()
                            .border_color(PlanColors::of(theme).wall),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::MEDIUM)
                    .child(t(cx, "corners")),
            )
            .child(
                div()
                    .text_size(px(size::T_SM))
                    .text_color(theme.text_3)
                    .child(room.walls.len().to_string()),
            )
            .child(folded_chevron(open, theme))
            .on_click(move |_, _, cx| {
                let _ = fold_page.update(cx, |p, cx| {
                    p.corners_open = !p.corners_open;
                    cx.notify();
                });
            });

        let mut corners: Vec<AnyElement> = Vec::new();
        if open {
            for (i, w) in room.walls.iter().enumerate() {
                let key = format!("corner:{i}");
                let handle = self.row_handle(&key, cx);
                let name = corner_name(&room.walls, i, cx);
                let page = page.clone();
                if i > 0 {
                    corners.push(rule(theme, 42.0).into_any_element());
                }
                corners.push(
                    item(("room-corner", i), name.clone().into(), theme)
                        .min_h(px(34.0))
                        .pl(px(42.0))
                        .track_focus(&handle)
                        .child(div().flex_1().truncate().child(name))
                        .child(
                            div()
                                .text_size(px(size::T_SM))
                                .text_color(theme.text_3)
                                .child(format!("{}, {}", w.x, w.y)),
                        )
                        .on_click(move |_, _, cx| {
                            let _ = page.update(cx, |p, cx| p.pick_other(Other::Corner(i), cx));
                        })
                        .into_any_element(),
                );
            }
            let add_page = page.clone();
            let reset_page = page.clone();
            corners.push(rule(theme, 42.0).into_any_element());
            corners.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(2.0))
                    .pl(px(34.0))
                    .pr(px(4.0))
                    .pt(px(6.0))
                    .pb(px(4.0))
                    .child(plain_button(
                        "room-add-corner",
                        "plus",
                        t(cx, "add_corner"),
                        theme.text_2,
                        theme,
                        move |_, _, cx| {
                            let _ = add_page.update(cx, |p, cx| p.add_corner(cx));
                        },
                    ))
                    .child(plain_button(
                        "room-reset-shape",
                        "reset",
                        t(cx, "reset_shape"),
                        theme.text_2,
                        theme,
                        move |_, _, cx| {
                            let _ = reset_page.update(cx, |p, cx| p.reset_shape(cx));
                        },
                    ))
                    .into_any_element(),
            );
        }

        ui::card(theme)
            .child(
                div()
                    .p(px(size::S3))
                    .child(paints)
                    .child(hint(t(cx, "walls_tip"), theme).mt(px(size::S3))),
            )
            .child(rule(theme, 8.0))
            .child(div().p(px(3.0)).child(summary).children(corners))
            .into_any_element()
    }

    // --- The inspector ---

    pub(super) fn inspector(
        &mut self,
        now: &Now,
        theme: &Theme,
        colors: &PlanColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page = cx.entity().downgrade();
        let back = {
            let page = page.clone();
            ui::pressable("room-back", theme)
                .role(Role::Button)
                .aria_label(t(cx, "nav_room"))
                .flex()
                .items_center()
                .gap(px(2.0))
                .mb(px(size::S2))
                .py(px(3.0))
                .pl(px(3.0))
                .pr(px(8.0))
                .rounded(px(size::R_SM))
                .text_size(px(size::T_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_2)
                .hover(|s| s.bg(theme.surface_hover).text_color(theme.text))
                .child(ui::icon("back", 13.0, false, theme.text_2))
                .child(t(cx, "nav_room"))
                .on_click(move |_, window, cx| {
                    let _ = page.update(cx, |p, cx| {
                        let ours = p.focus_is_ours(window, cx);
                        p.clear_selection(ours, cx);
                    });
                })
        };
        let body = if let Some(light) = now
            .selected
            .as_ref()
            .and_then(|id| now.config.room.lights.iter().find(|l| l.id == *id))
        {
            self.light_inspector(light, now, theme, colors, window, cx)
        } else {
            match self.other {
                Some(Other::Screen) => self.screen_inspector(now, theme, window, cx),
                Some(Other::Corner(i)) => self.corner_inspector(i, now, theme, cx),
                None => div().into_any_element(),
            }
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .child(back)
            .child(div().w_full().flex().flex_col().child(body))
            .into_any_element()
    }

    /// The name of the screen or a corner, where the focus lands on a pick.
    fn heading(&self, text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
        let text = text.into();
        div()
            .id("room-heading")
            .track_focus(&self.heading_focus)
            .role(Role::Heading)
            .aria_level(2)
            .aria_label(text.clone())
            .mb(px(size::S4))
            .ml(px(size::S2))
            .text_size(px(size::T_LG))
            .font_weight(FontWeight::BOLD)
            .text_color(theme.text)
            .child(text)
            .into_any_element()
    }

    /// The exact position, folded under its card: the plan is the way to
    /// place things, the numbers are there for the keyboard and fine work.
    fn exact(
        &self,
        p: lueur_core::geometry::Vec2,
        width: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.exact_open;
        let page = cx.entity().downgrade();
        let summary = ui::pressable("room-exact", theme)
            .role(Role::Button)
            .aria_label(t(cx, "exact_position"))
            .aria_expanded(open)
            .min_h(px(40.0))
            .flex()
            .items_center()
            .gap(px(size::S2))
            .text_size(px(size::T_SM))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text_2)
            .hover(|s| s.text_color(theme.text))
            .child(div().flex_1().child(t(cx, "exact_position")))
            .when(!open, |d| {
                d.child(
                    div()
                        .text_color(theme.text_3)
                        .child(format!("{}, {}", p.x, p.y)),
                )
            })
            .child(folded_chevron(open, theme))
            .on_click(move |_, _, cx| {
                let _ = page.update(cx, |p, cx| {
                    p.exact_open = !p.exact_open;
                    cx.notify();
                });
            });
        div()
            .border_t_1()
            .border_color(theme.separator)
            .child(summary)
            .when(open, |d| {
                d.child(
                    div()
                        .pb(px(size::S3))
                        .child(self.position(width, false, theme, cx)),
                )
            })
            .into_any_element()
    }

    /// The keyboard's way to place things: X and Y fields, kept in the
    /// plan's bounds.
    fn position(
        &self,
        width: f32,
        legend: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(fields) = &self.fields else {
            return div().into_any_element();
        };
        let axis = |letter: &'static str, input: gpui::Entity<ui::TextInput>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(size::T_SM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text_2)
                .child(letter)
                .child(div().flex_1().min_w_0().child(input))
        };
        let tip = fill(
            &t(cx, "pos_tip"),
            &[
                ("min", &plan::MARGIN.to_string()),
                ("maxX", &plan::X_MAX.to_string()),
                ("maxY", &plan::Y_MAX.to_string()),
                ("w", &width.round().to_string()),
            ],
        );
        div()
            .flex()
            .flex_col()
            .when(legend, |d| d.child(field_head(t(cx, "position"), theme)))
            .child(
                div()
                    .flex()
                    .gap(px(size::S2))
                    .child(axis("X", fields.x.clone()))
                    .child(axis("Y", fields.y.clone())),
            )
            .child(hint(tip, theme))
            .into_any_element()
    }

    fn light_inspector(
        &mut self,
        light: &Light,
        now: &Now,
        theme: &Theme,
        colors: &PlanColors,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scr = &now.config.room.screen;
        let m = map_light(scr, light);
        let page = cx.entity().downgrade();
        let lang = *cx.global::<Lang>();
        let id = light.id.clone();

        let name = self
            .fields
            .as_ref()
            .and_then(|f| f.name.clone())
            .map(|n| div().w_full().child(n).into_any_element());

        // The screen as it sits on a desk, with the part this light mirrors.
        let preview = preview(&m, light.enabled, theme, colors, cx);
        let caption = {
            let says = if light.enabled {
                t(cx, plan::describe(&m))
            } else {
                t(cx, "left_out")
            };
            div()
                .id("room-mirrors")
                .role(Role::Label)
                .aria_label(SharedString::from(format!(
                    "{}, {}",
                    zone_words(m.edge, cx),
                    says
                )))
                .mt(px(size::S3))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .text_size(px(size::T_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_2)
                .child(zone_badge(m.edge, !light.enabled, 15.0, colors, theme, cx))
                .child(says)
        };

        let height = {
            let page = page.clone();
            let id = id.clone();
            let ends = (t(cx, "h_floor"), t(cx, "h_ceiling"));
            let words: Vec<SharedString> = ["h_floor", "h_low", "h_level", "h_high", "h_ceiling"]
                .iter()
                .map(|k| t(cx, k))
                .collect();
            let reset = fill(&t(cx, "reset_to"), &[("v", &t(cx, "h_level"))]);
            ui::slider(
                "room-height",
                t(cx, "height"),
                light.height,
                (0.0, 1.0, 0.01),
                move |v, _, cx| {
                    let id = id.clone();
                    let _ = page.update(cx, |p, cx| {
                        p.edit(cx, |c| {
                            if let Some(l) = c.room.lights.iter_mut().find(|x| x.id == id) {
                                l.height = v;
                            }
                        })
                    });
                },
            )
            .default_value(0.5, reset.into())
            .format(move |v| {
                let i = ["h_floor", "h_low", "h_level", "h_high", "h_ceiling"]
                    .iter()
                    .position(|k| *k == plan::height_key(v))
                    .unwrap_or(2);
                words[i].to_string()
            })
            .ends(ends.0, ends.1)
            .render(theme, window, cx)
            .into_any_element()
        };

        let follows = {
            let page = page.clone();
            let id = id.clone();
            let on = light.enabled;
            ui::switch(
                "room-follows",
                t(cx, "follows"),
                on,
                theme,
                move |_, _, cx| {
                    let id = id.clone();
                    let _ = page.update(cx, |p, cx| {
                        p.edit(cx, |c| {
                            if let Some(l) = c.room.lights.iter_mut().find(|x| x.id == id) {
                                l.enabled = !on;
                            }
                        })
                    });
                },
            )
        };

        // What the distance to the screen does, said once, under its numbers.
        let facts = light.enabled.then(|| {
            let fact = |label: SharedString, v: f32| {
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(size::T_SM))
                            .text_color(theme.text_3)
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(px(size::T_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(pct(lang, v)),
                    )
            };
            div()
                .py(px(size::S3))
                .border_t_1()
                .border_color(theme.separator)
                .child(
                    div()
                        .flex()
                        .gap(px(size::S3))
                        .child(fact(t(cx, "fact_brightness"), m.gain))
                        .child(fact(t(cx, "fact_response"), m.responsiveness)),
                )
                .child(hint(t(cx, "facts_tip"), theme))
        });

        let card = ui::card(theme)
            .px(px(size::S3))
            .child(
                div()
                    .py(px(size::S3))
                    .child(field_head(t(cx, "height"), theme))
                    .child(height),
            )
            .child(
                div()
                    .min_h(px(44.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(size::S3))
                    .border_t_1()
                    .border_color(theme.separator)
                    .child(
                        div()
                            .text_size(px(size::T_MD))
                            .text_color(theme.text)
                            .child(t(cx, "follows")),
                    )
                    .child(follows),
            )
            .children(facts)
            .child(self.exact(light.pos, scr.width, theme, cx));

        // The bulb by name, never by its address.
        let bulb = match now.config.binding_for(&light.id) {
            Some(b) => {
                let page = page.clone();
                let id = id.clone();
                ui::card(theme)
                    .flex_row()
                    .items_center()
                    .gap(px(size::S2))
                    .min_h(px(48.0))
                    .pl(px(size::S3))
                    .pr(px(size::S2))
                    .child(ui::icon("bulb", 15.0, false, theme.text_3))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(size::T_SM))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(bulb_label(b, Some(&light.name))),
                    )
                    .child(
                        ui::button(
                            "room-identify",
                            t(cx, "identify"),
                            false,
                            theme,
                            move |_, _, cx| {
                                let id = id.clone();
                                let _ = page.update(cx, |p, cx| p.identify(&id, cx));
                            },
                        )
                        .h(px(26.0))
                        .px(px(size::S3))
                        .tooltip(ui::tip(t(cx, "identify_tip"))),
                    )
            }
            None => {
                let lueur = self.lueur.clone();
                ui::card(theme)
                    .flex_row()
                    .items_center()
                    .gap(px(size::S2))
                    .min_h(px(48.0))
                    .pl(px(size::S3))
                    .pr(px(size::S2))
                    .child(div().w(px(15.0)).flex().justify_center().child(lamp(theme)))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(size::T_SM))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.glow_text)
                            .child(t(cx, "no_bulb")),
                    )
                    .child(
                        ui::button(
                            "room-assign",
                            t(cx, "assign"),
                            false,
                            theme,
                            move |_, _, cx| {
                                lueur.update(cx, |l, cx| l.show(View::Bulbs, cx));
                            },
                        )
                        .h(px(26.0))
                        .px(px(size::S3)),
                    )
            }
        };

        let remove = plain_button(
            "room-remove-light",
            "trash",
            t(cx, "remove_light"),
            theme.danger,
            theme,
            move |_, window, cx| {
                let id = id.clone();
                let _ = page.update(cx, |p, cx| {
                    let ours = p.focus_is_ours(window, cx);
                    p.remove_light(&id, ours, cx);
                });
            },
        );

        div()
            .flex()
            .flex_col()
            .children(name)
            .child(
                div()
                    .mt(px(size::S3))
                    .mb(px(size::S4))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(preview)
                    .child(caption),
            )
            .child(card)
            .child(div().mt(px(size::S3)).child(bulb))
            .child(div().mt(px(size::S4)).flex().child(remove))
            .into_any_element()
    }

    fn screen_inspector(
        &mut self,
        now: &Now,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scr = now.config.room.screen;
        let first = lueur_core::geometry::Screen::default();
        let page = cx.entity().downgrade();
        let facing = {
            let page = page.clone();
            ui::slider(
                "room-facing",
                t(cx, "facing"),
                scr.facing_deg,
                (-180.0, 180.0, 1.0),
                move |v, _, cx| {
                    let _ = page.update(cx, |p, cx| p.edit(cx, |c| c.room.screen.facing_deg = v));
                },
            )
            .default_value(
                first.facing_deg,
                fill(
                    &t(cx, "reset_to"),
                    &[("v", &format!("{}°", first.facing_deg))],
                )
                .into(),
            )
            .format(|v| format!("{v}°"))
            .render(theme, window, cx)
            .into_any_element()
        };
        let width = {
            let page = page.clone();
            ui::slider(
                "room-width",
                t(cx, "width"),
                scr.width,
                (plan::SCREEN_MIN, plan::SCREEN_MAX, 5.0),
                move |v, _, cx| {
                    let _ = page.update(cx, |p, cx| p.edit(cx, |c| c.room.screen.width = v));
                },
            )
            .default_value(
                first.width,
                fill(&t(cx, "reset_to"), &[("v", &first.width.to_string())]).into(),
            )
            .format(|v| v.round().to_string())
            .render(theme, window, cx)
            .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .child(self.heading(t(cx, "plan_screen"), theme))
            .child(
                ui::card(theme)
                    .px(px(size::S3))
                    .child(
                        div()
                            .py(px(size::S3))
                            .child(field_head(t(cx, "facing"), theme))
                            .child(facing),
                    )
                    .child(
                        div()
                            .py(px(size::S3))
                            .border_t_1()
                            .border_color(theme.separator)
                            .child(field_head(t(cx, "width"), theme))
                            .child(width),
                    )
                    .child(self.exact(scr.center, scr.width, theme, cx)),
            )
            .child(
                hint(t(cx, "screen_hint"), theme)
                    .mx(px(size::S1))
                    .mt(px(size::S3)),
            )
            .into_any_element()
    }

    fn corner_inspector(
        &mut self,
        index: usize,
        now: &Now,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let walls = &now.config.room.walls;
        let page = cx.entity().downgrade();
        let name = corner_name(walls, index, cx);
        div()
            .flex()
            .flex_col()
            .child(self.heading(name, theme))
            .when(walls.get(index).is_some(), |d| {
                d.child(
                    ui::card(theme)
                        .px(px(size::S3))
                        .py(px(size::S3))
                        .child(self.position(now.config.room.screen.width, true, theme, cx)),
                )
            })
            .child(div().mt(px(size::S4)).flex().child(plain_button(
                "room-remove-corner",
                "trash",
                t(cx, "remove_corner"),
                theme.danger,
                theme,
                move |_, window, cx| {
                    let _ = page.update(cx, |p, cx| {
                        let ours = p.focus_is_ours(window, cx);
                        p.remove_corner(index, ours, cx);
                    });
                },
            )))
            .into_any_element()
    }
}

/// The screen as it sits on a desk, with the part a light mirrors, after
/// the inspector's small SVG (168 by 106 units, drawn here at 1.25 times).
fn preview(m: &LightMapping, on: bool, theme: &Theme, colors: &PlanColors, cx: &App) -> AnyElement {
    const S: f32 = 1.25;
    let zone = if on {
        colors.zone(m.edge)
    } else {
        theme.line_strong
    };
    let (gw, gh) = (160.0 * S, 90.0 * S);
    let x = 4.0 * S + m.zone.x * gw + 3.0 * S;
    let y = 4.0 * S + m.zone.y * gh + 3.0 * S;
    let w = (m.zone.w * gw - 6.0 * S).max(4.0 * S);
    let h = (m.zone.h * gh - 6.0 * S).max(4.0 * S);
    let fill_alpha = 0.22 + 0.28 * (1.0 - m.spread);
    div()
        .id("room-preview")
        .role(Role::Image)
        .aria_label(t(cx, "mirrors"))
        .flex()
        .flex_col()
        .items_center()
        .child(
            div()
                .relative()
                .w(px(168.0 * S))
                .h(px(98.0 * S))
                .rounded(px(7.0 * S))
                .bg(colors.bezel)
                .border_1()
                .border_color(theme.line_strong)
                .shadow_md()
                .child(
                    div()
                        .absolute()
                        .left(px(4.0 * S - 1.0))
                        .top(px(4.0 * S - 1.0))
                        .w(px(gw))
                        .h(px(gh))
                        .rounded(px(4.0 * S))
                        .bg(theme.surface_2),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(x - 1.0))
                        .top(px(y - 1.0))
                        .w(px(w))
                        .h(px(h))
                        .rounded(px(3.0 * S))
                        .bg(zone.opacity(fill_alpha))
                        .border_1()
                        .border_color(zone),
                ),
        )
        .child(div().w(px(28.0 * S)).h(px(5.0 * S)).bg(colors.bezel))
        .child(
            div()
                .w(px(52.0 * S))
                .h(px(3.0 * S))
                .rounded(px(1.5 * S))
                .bg(colors.bezel),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::bulbs::brands::bulb_label;
    use lueur_core::config::DeviceBinding;

    #[test]
    fn a_bulb_is_named_by_its_name_and_brand_never_its_address() {
        let mut b = DeviceBinding {
            light_id: "l".into(),
            device_id: "d".into(),
            ip: "192.168.1.2".into(),
            port: 1,
            model: "color4".into(),
            name: String::new(),
            kind: Default::default(),
        };
        assert_eq!(bulb_label(&b, Some("Desk")), "Yeelight color4");
        b.name = "Desk".into();
        assert_eq!(bulb_label(&b, Some("Desk")), "Yeelight color4");
        b.name = "Lamp".into();
        assert_eq!(bulb_label(&b, Some("Desk")), "Lamp · Yeelight color4");
        assert!(!bulb_label(&b, Some("Desk")).contains("192"));
    }

    #[test]
    fn the_paints_offered_include_white() {
        assert_eq!(WALLS[0].1, Rgb::WHITE);
    }
}
