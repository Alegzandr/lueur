//! Every light in the room as a card that shows its light and says what it
//! does, and picks the lights the settings beside it go to, after
//! `lighting/Lights.svelte`.

use super::LightingPage;
use super::controls::{Kind, btn, group_title, light_dot};
use super::model::Room;
use crate::i18n::{fill, pct, t};
use crate::state::View;
use crate::theme::{Theme, size};
use crate::ui;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Role, SharedString, Toggled, div, prelude::*, px,
};

/// The narrowest a light's card gets before the row takes one fewer.
const CARD_MIN: f32 = 160.0;

/// How many cards of at least `min` fit in `width`, `gap` apart.
pub(super) fn columns(width: f32, min: f32, gap: f32) -> usize {
    (((width + gap) / (min + gap)).floor() as usize).max(1)
}

impl LightingPage {
    pub(super) fn lights(&self, width: f32, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let lang = l.lang();
        if l.config.room.lights.is_empty() {
            return div().into_any_element();
        }
        let pickable = l
            .config
            .room
            .lights
            .iter()
            .filter(|x| room.bound(&x.id))
            .count();
        // Picking only means something with two lights or more.
        let selectable = pickable > 1;
        let whole = room.picked().is_none();
        let lueur = self.lueur.clone();

        let mut cards = Vec::new();
        for light in &l.config.room.lights {
            let id = light.id.clone();
            let name: SharedString = light.name.clone().into();
            let bound = room.bound(&id);
            let mode = room.mode_of(&id);
            let other = if bound { room.elsewhere(&id) } else { None };
            let picked = selectable && !whole && l.target.contains(&id);
            let on = room.on(&id);
            let busy = l.power_busy.contains(&id);

            // What it does, in words.
            let (sub, sub_tip, live, plain): (SharedString, Option<SharedString>, bool, bool) =
                if !bound {
                    (t(cx, "no_bulb"), None, false, false)
                } else if mode.is_some() && other.is_some() {
                    let host = other.clone().unwrap_or_default();
                    if host.is_empty() {
                        (
                            t(cx, "held_by_other"),
                            Some(t(cx, "held_other_tip")),
                            false,
                            false,
                        )
                    } else {
                        (
                            fill(&t(cx, "held_by"), &[("host", &host)]).into(),
                            Some(fill(&t(cx, "held_tip"), &[("host", &host)]).into()),
                            false,
                            false,
                        )
                    }
                } else if mode.is_some() && room.reconnecting(&id) {
                    (
                        t(cx, "reconnecting"),
                        Some(t(cx, "reconnecting_tip")),
                        false,
                        false,
                    )
                } else if mode.is_none() && other.as_ref().is_some_and(|h| !h.is_empty()) {
                    let host = other.clone().unwrap_or_default();
                    (
                        fill(&t(cx, "elsewhere_sub"), &[("host", &host)]).into(),
                        None,
                        false,
                        false,
                    )
                } else {
                    (
                        room.doing_of(&id).name(lang).into(),
                        None,
                        mode.is_some(),
                        true,
                    )
                };
            let bright = (plain && on).then(|| pct(lang, f32::from(room.bright_of(&id)) / 100.0));

            let labelled = |what: &str| -> SharedString {
                fill(&t(cx, "labelled"), &[("name", &name), ("what", what)]).into()
            };
            // The one button on the card: a place for a bulb, taking a bulb
            // back, or the light's power.
            let action = if !bound {
                let tip = t(cx, "assign");
                let lueur = lueur.clone();
                btn(("assign", cards.len()), Kind::Default, true, false, theme)
                    .w(px(24.0))
                    .px_0()
                    .aria_label(labelled(&tip))
                    .tooltip(ui::tip(tip))
                    .child(ui::icon("plus", 14.0, false, theme.text))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        lueur.update(cx, |l, cx| l.show(View::Bulbs, cx));
                    })
                    .into_any_element()
            } else if mode.is_some() && other.is_some() {
                let label = t(cx, "take_over");
                let lueur = lueur.clone();
                let id = id.clone();
                btn(("take", cards.len()), Kind::Default, true, false, theme)
                    .aria_label(labelled(&label))
                    .tooltip(ui::tip(t(cx, "take_over_tip")))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        lueur.update(cx, |l, cx| l.take_light(id.clone(), cx));
                    })
                    .into_any_element()
            } else {
                // Whether it is on, the disc shows: the button stays quiet.
                let what = t(cx, if on { "turn_off" } else { "turn_on" });
                let lueur = lueur.clone();
                let id = id.clone();
                btn(("power", cards.len()), Kind::Plain, true, busy, theme)
                    .w(px(24.0))
                    .px_0()
                    .aria_label(labelled(&what))
                    .tooltip(ui::tip(what))
                    .child(ui::icon("power", 14.0, false, theme.text_3))
                    .when(!busy, |b| {
                        b.on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let id = id.clone();
                            lueur.update(cx, |l, cx| l.set_light_power(id, !on, cx));
                        })
                    })
                    .into_any_element()
            };

            let disc = div()
                .relative()
                .flex_none()
                .child(light_dot(&room, &id, 28.0, false, theme))
                // Picked: a tick on the light and a neutral edge, like a
                // checkbox.
                .when(picked, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(-4.0))
                            .bottom(px(-4.0))
                            .size(px(15.0))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.select)
                            .border_2()
                            .border_color(theme.surface)
                            .child(ui::icon("check", 9.0, false, theme.on_ink)),
                    )
                });

            let text = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .truncate()
                        .text_size(px(size::T_MD))
                        .font_weight(if bound {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if bound { theme.text } else { theme.text_2 })
                        .child(name.clone()),
                )
                .child(
                    div()
                        .id(("light-sub", cards.len()))
                        .flex()
                        .items_baseline()
                        .gap(px(6.0))
                        .min_w_0()
                        .text_size(px(size::T_SM))
                        .text_color(theme.text_2)
                        .whitespace_nowrap()
                        .when_some(sub_tip, |d, tip| d.tooltip(ui::tip(tip)))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                // An effect plays: its name in the amber of
                                // light.
                                .when(live, |d| {
                                    d.text_color(theme.glow_text)
                                        .font_weight(FontWeight::MEDIUM)
                                })
                                .child(sub.clone()),
                        )
                        .children(
                            bright
                                .clone()
                                .map(|b| div().flex_none().text_color(theme.text_3).child(b)),
                        ),
                );

            let description: SharedString = match &bright {
                Some(b) => format!("{sub}, {b}").into(),
                None => sub.clone(),
            };
            let card = ui::pressable(("light", cards.len()), theme)
                .relative()
                .flex()
                .items_center()
                .gap(px(size::S3))
                .min_w_0()
                .min_h(px(60.0))
                .py(px(size::S2))
                .pr(px(size::S2))
                .pl(px(size::S3))
                .rounded(px(size::R_LG))
                .map(|d| {
                    if !bound {
                        // No bulb yet: an outline waiting for one, not a card.
                        d.border_1().border_color(theme.line_strong)
                    } else if picked {
                        // The tint of a selection, never the room's glow.
                        d.bg(theme.material_hover)
                            .border_2()
                            .border_color(theme.select)
                    } else {
                        d.bg(theme.material)
                            .border_1()
                            .border_color(theme.separator)
                    }
                })
                .child(disc)
                .child(text)
                .child(action);
            let card = if bound && selectable {
                // The whole card picks the light; its button sits above it.
                let lueur = lueur.clone();
                let id = id.clone();
                card.role(Role::Button)
                    .aria_label(name.clone())
                    .aria_description(description)
                    .aria_toggled(if picked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .hover(|s| s.bg(theme.material_hover))
                    .on_click(move |_, _, cx| lueur.update(cx, |l, cx| l.toggle_target(&id, cx)))
            } else {
                // Nothing to pick: a group the keyboard passes by.
                card.role(Role::Group)
                    .aria_label(name.clone())
                    .aria_description(description)
                    .tab_stop(false)
                    .cursor_default()
            };
            cards.push(card.into_any_element());
        }

        let chip = selectable.then(|| {
            // Who the settings go to, not what plays: a neutral edge and a
            // tick, never the amber of something running.
            let label = t(cx, "select_all");
            let lueur = lueur.clone();
            ui::pressable("select-all", theme)
                .role(Role::Button)
                .aria_label(label.clone())
                .aria_toggled(if whole { Toggled::True } else { Toggled::False })
                .tooltip(ui::tip(t(cx, "select_all_tip")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .h(px(24.0))
                .pl(px(size::S2))
                .pr(px(size::S3))
                .rounded_full()
                .text_size(px(size::T_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text)
                .whitespace_nowrap()
                .map(|d| {
                    if whole {
                        d.bg(theme.material_hover)
                            .border_2()
                            .border_color(theme.select)
                    } else {
                        d.bg(theme.material)
                            .border_1()
                            .border_color(theme.separator)
                            .hover(|s| s.bg(theme.material_hover))
                    }
                })
                .child(
                    div()
                        .flex_none()
                        .size(px(14.0))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .map(|d| {
                            if whole {
                                d.bg(theme.select).child(ui::icon(
                                    "check",
                                    10.0,
                                    false,
                                    theme.on_ink,
                                ))
                            } else {
                                d.border_1().border_color(theme.control_line)
                            }
                        }),
                )
                .child(label)
                .on_click(move |_, _, cx| {
                    lueur.update(cx, |l, cx| {
                        l.target.clear();
                        cx.notify();
                    })
                })
        });

        div()
            .id("lights")
            .role(Role::Group)
            .aria_label(t(cx, "lights"))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(size::S3))
                    .min_h(px(24.0))
                    .px(px(size::S1))
                    .pb(px(size::S2))
                    .child(group_title(t(cx, "lights"), theme))
                    .children(chip),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(columns(width, CARD_MIN, size::S2) as u16)
                    .gap(px(size::S2))
                    .children(cards),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_cards_fill_the_row() {
        assert_eq!(super::columns(495.0, 160.0, 8.0), 2);
        assert_eq!(super::columns(496.0, 160.0, 8.0), 3);
        assert_eq!(super::columns(10.0, 160.0, 8.0), 1);
    }
}
