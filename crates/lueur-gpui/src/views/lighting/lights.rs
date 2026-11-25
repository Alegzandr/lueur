//! Every light in the room as a card that shows its light, says what it
//! does and holds its own switch, under a head with the switch for every
//! light and the sleep timer. A click on a card opens that light's settings
//! alone; a second click goes back to the whole room.

use super::LightingPage;
use super::controls::{Kind, btn, group_title, light_dot, light_wash};
use super::model::Room;
use crate::i18n::{fill, pct, t, tn};
use crate::state::View;
use crate::theme::{Theme, size};
use crate::ui;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, Role, SharedString, Toggled, Window, div,
    prelude::*, px,
};

/// The narrowest a light's card gets before the row takes one fewer.
const CARD_MIN: f32 = 240.0;

/// How many cards of at least `min` fit in `width`, `gap` apart.
pub(super) fn columns(width: f32, min: f32, gap: f32) -> usize {
    (((width + gap) / (min + gap)).floor() as usize).max(1)
}

impl LightingPage {
    pub(super) fn lights(
        &mut self,
        width: f32,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.lueur.read(cx).config.room.lights.is_empty() {
            return div().into_any_element();
        }
        let any_bound = !self.lueur.read(cx).config.bindings.is_empty();
        // The sleep timer sits beside the switch for every light: both put
        // the room out, one now, one later.
        let sleep = any_bound.then(|| self.sleep_button(theme, window, cx));
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let lang = l.lang();
        let lueur = self.lueur.clone();
        // Opening one light's settings only means something with two or more.
        let pickable = l.config.bindings.len() > 1;
        let open = room.picked().filter(|p| p.len() == 1).map(|p| p[0].clone());

        let mut cards = Vec::new();
        for light in &l.config.room.lights {
            let id = light.id.clone();
            let name: SharedString = light.name.clone().into();
            let bound = room.bound(&id);
            let mode = room.mode_of(&id);
            let other = if bound { room.elsewhere(&id) } else { None };
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
            // The one control on the card: a place for a bulb, taking a bulb
            // back, or the light's own switch.
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
                // A switch, as on a wall: its position says whether the
                // light is on, without reading the disc's colour.
                let lueur = lueur.clone();
                let id = id.clone();
                ui::switch(
                    ("power", cards.len()),
                    name.clone(),
                    on,
                    theme,
                    move |_, _, cx| {
                        cx.stop_propagation();
                        if !busy {
                            let id = id.clone();
                            lueur.update(cx, |l, cx| l.set_light_power(id, !on, cx));
                        }
                    },
                )
                .flex_none()
                .when(busy, |d| d.opacity(0.6))
                .tooltip(ui::tip(t(cx, if on { "turn_off" } else { "turn_on" })))
                .into_any_element()
            };

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
            // A click on the card opens this light's settings alone, as on
            // the bulbs' own apps; its switch and button stay its own.
            let opens = bound && pickable;
            let is_open = open.as_deref() == Some(id.as_str());
            let card = ui::pressable(("light", cards.len()), theme)
                .relative()
                .aria_label(name.clone())
                .aria_description(description)
                .flex()
                .items_center()
                .gap(px(size::S3))
                .min_w_0()
                .min_h(px(60.0))
                .py(px(size::S2))
                .px(px(size::S3))
                .rounded(px(size::R_LG))
                .map(|d| {
                    if !bound {
                        // No bulb yet: an outline waiting for one, not a card.
                        d.border_1().border_color(theme.line_strong)
                    } else if is_open {
                        // Open: a neutral ink edge, never the amber of light.
                        d.bg(theme.material_hover)
                            .border_2()
                            .border_color(theme.select)
                    } else {
                        d.bg(theme.material)
                            .border_1()
                            .border_color(theme.separator)
                    }
                })
                .children(light_wash(&room, &id, theme))
                .child(light_dot(&room, &id, 28.0, false, theme))
                .child(text)
                .child(action);
            let card = if opens {
                let lueur = lueur.clone();
                let id = id.clone();
                card.role(Role::Button)
                    .aria_toggled(if is_open {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .tooltip(ui::tip(t(
                        cx,
                        if is_open {
                            "back_to_room_tip"
                        } else {
                            "light_settings_tip"
                        },
                    )))
                    .hover(|s| s.bg(theme.material_hover))
                    .on_click(move |_, _, cx| lueur.update(cx, |l, cx| l.pick_light(&id, cx)))
            } else {
                // Nothing to open: a group the keyboard passes by, straight
                // to its switch or button.
                card.role(Role::Group).tab_stop(false).cursor_default()
            };
            cards.push(card.into_any_element());
        }

        // How many lights shine, so the switch for all of them says what it
        // will do before it is pressed.
        let total = l.config.bindings.len();
        let lit = l
            .config
            .bindings
            .iter()
            .filter(|b| room.on(&b.light_id))
            .count();
        let count: Option<SharedString> = any_bound.then(|| {
            if lit == 0 {
                t(cx, "lights_none_on")
            } else {
                fill(&tn(cx, "lights_on", lit), &[("total", &total.to_string())]).into()
            }
        });
        let power = any_bound.then(|| {
            let busy = l.busy;
            let any_on = lit > 0;
            let label = t(cx, if any_on { "all_off" } else { "all_on" });
            let lueur = lueur.clone();
            btn("all-power", Kind::Default, true, busy, theme)
                .aria_label(label.clone())
                .when(any_on, |b| b.tooltip(ui::tip(t(cx, "all_off_tip"))))
                .gap(px(6.0))
                .child(ui::icon("power", 13.0, false, theme.text_2))
                .child(label)
                .when(!busy, |b| {
                    b.on_click(move |_, _, cx| {
                        lueur.update(cx, |l, cx| l.set_all_power(!any_on, cx))
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
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(size::S3))
                    .min_h(px(28.0))
                    .px(px(size::S1))
                    .pb(px(size::S2))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(size::S2))
                            .child(group_title(t(cx, "lights"), theme))
                            .children(count.map(|c| {
                                div()
                                    .id("lights-count")
                                    .role(Role::Status)
                                    .text_size(px(size::T_SM))
                                    .text_color(theme.text_2)
                                    .child(c)
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(size::S2))
                            .children(sleep)
                            .children(power),
                    ),
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
