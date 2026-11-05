//! The light the lights picked give: one
//! brightness for all of them, the white or colour of those set by hand,
//! and for a single light, its identify flash and its place on the plan.

use super::LightingPage;
use super::controls::{Kind, btn, gradient_slider, hue_stops, rgb8, warmth_stops};
use super::model::{MAX_KELVIN, MIN_KELVIN, PALETTE, Room, hue_rgb, rgb_hue};
use crate::i18n::{fill, pct, t};
use crate::theme::{Theme, size};
use crate::ui::{self, choice};
use gpui::{
    AnyElement, Context, IntoElement, Role, SharedString, Toggled, Window, div, prelude::*, px,
};
use lueur_core::scene::Tint;
use std::collections::BTreeSet;

/// Which pool of brightness a mode draws from: the screen has its own, the
/// shows share another.
fn pool(mode: Option<&str>) -> Option<&'static str> {
    match mode {
        None => None,
        Some("ambilight") => Some("screen"),
        Some(_) => Some("show"),
    }
}

impl LightingPage {
    /// Switch the lights set by hand between a white and a colour, back to
    /// what they were last given in the other.
    fn set_tint_mode(&mut self, ids: Vec<String>, to_colour: bool, cx: &mut Context<Self>) {
        let Some(first) = ids.first() else { return };
        let tint = Room::of(self.lueur.read(cx)).state(first).tint;
        let next = match (tint, to_colour) {
            (Tint::White { kelvin }, true) => {
                self.last_kelvin = kelvin;
                let [r, g, b] = self.last_colour;
                Tint::Color { r, g, b }
            }
            (Tint::Color { r, g, b }, false) => {
                self.last_colour = [r, g, b];
                Tint::White {
                    kelvin: self.last_kelvin,
                }
            }
            _ => return,
        };
        self.lueur.update(cx, |l, cx| {
            for id in &ids {
                l.adjust_light(id, |s| s.tint = next, cx);
            }
        });
    }

    pub(super) fn tune(
        &mut self,
        width: f32,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let lang = l.lang();
        let ids = room.mood_targets();
        if ids.is_empty() {
            return None;
        }
        // The one light the settings go to, if there is one.
        let single: Option<String> = match room.picked() {
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
        };

        // --- Brightness ---
        // The lights on say how bright the selection is; with all of them
        // off, the brightness they come back at.
        let lit: Vec<&String> = ids.iter().filter(|id| room.on(id)).collect();
        let from: Vec<&String> = if lit.is_empty() {
            ids.iter().collect()
        } else {
            lit
        };
        let brightnesses: Vec<u8> = from.iter().map(|id| room.bright_of(id)).collect();
        let brightness = (brightnesses.iter().map(|&b| f32::from(b)).sum::<f32>()
            / brightnesses.len().max(1) as f32)
            .round();
        // Not all as bright: the slider shows their average, and says so.
        let mixed = brightnesses.iter().collect::<BTreeSet<_>>().len() > 1;
        // An effect has one brightness for every light playing it: moving
        // it moves the lights outside the selection playing it too.
        let pools: BTreeSet<&str> = ids.iter().filter_map(|id| pool(room.mode_of(id))).collect();
        let shared: Vec<String> = if pools.is_empty() {
            Vec::new()
        } else {
            l.config
                .bindings
                .iter()
                .map(|b| &b.light_id)
                .filter(|id| {
                    !ids.contains(id) && pool(room.mode_of(id)).is_some_and(|p| pools.contains(p))
                })
                .map(|id| room.name_of(id))
                .collect()
        };
        let by_hand: Vec<String> = ids
            .iter()
            .filter(|id| room.mode_of(id).is_none())
            .cloned()
            .collect();
        let tint = by_hand.first().map(|id| room.state(id).tint);

        let label = t(cx, "brightness");
        let mixed_text = t(cx, "bright_mixed");
        let lueur = self.lueur.clone();
        let targets = ids.clone();
        // An effect goes no dimmer than 5 %: the thumb stops where its light
        // does.
        let min = if by_hand.is_empty() { 5.0 } else { 1.0 };
        let slider = ui::slider(
            "brightness",
            label.clone(),
            brightness.max(min),
            (min, 100.0, 1.0),
            move |v, _, cx| {
                let v = v.round().clamp(1.0, 100.0) as u8;
                lueur.update(cx, |l, cx| l.set_brightness(&targets, v, cx));
            },
        )
        .format(move |v| {
            let p = pct(lang, v / 100.0);
            if mixed {
                fill(&mixed_text, &[("value", &p)])
            } else {
                p
            }
        })
        .render(theme, window, cx);
        let mut blocks: Vec<AnyElement> = vec![
            div()
                .flex()
                .flex_col()
                .child(ui::row(0, label, None, row_control(width, slider), theme))
                .when(!shared.is_empty(), |d| {
                    d.child(
                        div()
                            .px(px(size::S4))
                            .pb(px(size::S3))
                            .text_size(px(size::T_SM))
                            .text_color(theme.text_3)
                            .child(fill(
                                &t(cx, "bright_shared"),
                                &[("names", &shared.join(", "))],
                            )),
                    )
                })
                .into_any_element(),
        ];

        // --- White and colour, for the lights set by hand ---
        if let Some(tint) = tint {
            blocks.push(self.tint_block(tint, by_hand, theme, window, cx));
        }

        if let Some(single) = single {
            let identify = t(cx, "identify");
            let plan = t(cx, "show_on_plan");
            let lueur = self.lueur.clone();
            let lueur2 = self.lueur.clone();
            let single2 = single.clone();
            blocks.push(
                div()
                    .flex()
                    .gap(px(size::S1))
                    .px(px(size::S2))
                    .py(px(size::S1))
                    .child(
                        btn("identify", Kind::Plain, true, false, theme)
                            .aria_label(identify.clone())
                            .tooltip(ui::tip(t(cx, "identify_tip")))
                            .child(ui::icon("flash", 13.0, false, theme.text_2))
                            .child(identify)
                            .on_click(move |_, _, cx| {
                                lueur.update(cx, |l, cx| l.identify_light(&single, cx))
                            }),
                    )
                    .child(
                        btn("show-on-plan", Kind::Plain, true, false, theme)
                            .aria_label(plan.clone())
                            .child(ui::icon("plan", 13.0, false, theme.text_2))
                            .child(plan)
                            .on_click(move |_, _, cx| {
                                lueur2.update(cx, |l, cx| l.show_on_plan(&single2, cx))
                            }),
                    )
                    .into_any_element(),
            );
        }

        let blocks = blocks.into_iter().enumerate().map(|(i, b)| {
            div()
                .when(i > 0, |d| d.border_t_1().border_color(theme.separator))
                .child(b)
        });
        Some(
            div()
                .id("adjust")
                .role(Role::Group)
                .aria_label(t(cx, "adjust"))
                .mt(px(size::S6))
                .child(ui::card(theme).children(blocks))
                .into_any_element(),
        )
    }

    fn tint_block(
        &mut self,
        tint: Tint,
        by_hand: Vec<String>,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = t(cx, "tint");
        let page = cx.entity().downgrade();
        let ids = by_hand.clone();
        let colour = matches!(tint, Tint::Color { .. });
        let segmented = ui::segmented(
            "tint",
            label.clone(),
            Some(&colour),
            vec![
                choice(false, t(cx, "white")),
                choice(true, t(cx, "tint_colour")),
            ],
            theme,
            move |to, _, cx| {
                let ids = ids.clone();
                let _ = page.update(cx, |p, cx| p.set_tint_mode(ids, to, cx));
            },
        );
        let lueur = self.lueur.clone();
        let below = match tint {
            Tint::White { kelvin } => {
                let ids = by_hand.clone();
                let shown: SharedString = format!("{kelvin} K").into();
                let slider = gradient_slider(
                    "warmth",
                    t(cx, "warmth"),
                    f32::from(kelvin),
                    (f32::from(MIN_KELVIN), f32::from(MAX_KELVIN), 100.0),
                    warmth_stops(),
                    shown.clone(),
                    theme,
                    window,
                    cx,
                    move |v, _, cx| {
                        let kelvin = v.round() as u16;
                        lueur.update(cx, |l, cx| {
                            for id in &ids {
                                l.adjust_light(id, |s| s.tint = Tint::White { kelvin }, cx);
                            }
                        });
                    },
                );
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(slider)
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(size::T_XS))
                            .text_color(theme.text_3)
                            .child(t(cx, "warmth_warm"))
                            .child(shown)
                            .child(t(cx, "warmth_cool")),
                    )
                    .into_any_element()
            }
            Tint::Color { r, g, b } => {
                let current = [r, g, b];
                let in_palette = PALETTE.iter().any(|(c, _)| *c == current);
                let dots = PALETTE
                    .iter()
                    .enumerate()
                    .map(|(i, (c, key))| {
                        let picked = *c == current;
                        let name = t(cx, key);
                        let ids = by_hand.clone();
                        let lueur = lueur.clone();
                        let c = *c;
                        let page = cx.entity().downgrade();
                        swatch(("colour", i), name, picked, theme)
                            .bg(rgb8(c, 1.0))
                            .on_click(move |_, _, cx| {
                                let _ = page.update(cx, |p, cx| {
                                    p.custom_colour = false;
                                    cx.notify();
                                });
                                let [r, g, b] = c;
                                lueur.update(cx, |l, cx| {
                                    for id in &ids {
                                        l.adjust_light(
                                            id,
                                            |s| s.tint = Tint::Color { r, g, b },
                                            cx,
                                        );
                                    }
                                });
                            })
                            .into_any_element()
                    })
                    .collect::<Vec<_>>();
                // A colour of one's own: a ring of every hue that opens a
                // hue slider under the palette.
                let custom = t(cx, "custom_colour");
                let page = cx.entity().downgrade();
                let open = self.custom_colour;
                let any = swatch("colour-any", custom.clone(), !in_palette || open, theme)
                    .aria_expanded(open)
                    .overflow_hidden()
                    .child(
                        div().size_full().flex().children(
                            hue_stops()
                                .into_iter()
                                .map(|c| div().flex_1().h_full().bg(c)),
                        ),
                    )
                    .on_click(move |_, _, cx| {
                        let _ = page.update(cx, |p, cx| {
                            p.custom_colour = !p.custom_colour;
                            cx.notify();
                        });
                    });
                let hue_row = (open || !in_palette).then(|| {
                    let ids = by_hand.clone();
                    let lueur = lueur.clone();
                    let hue = rgb_hue(current);
                    gradient_slider(
                        "hue",
                        custom.clone(),
                        hue.round(),
                        (0.0, 360.0, 1.0),
                        hue_stops(),
                        format!("{}°", hue.round()).into(),
                        theme,
                        window,
                        cx,
                        move |v, _, cx| {
                            let [r, g, b] = hue_rgb(v);
                            lueur.update(cx, |l, cx| {
                                for id in &ids {
                                    l.adjust_light(id, |s| s.tint = Tint::Color { r, g, b }, cx);
                                }
                            });
                        },
                    )
                });
                div()
                    .flex()
                    .flex_col()
                    .gap(px(size::S3))
                    .child(
                        div()
                            .id("palette")
                            .role(Role::Group)
                            .aria_label(t(cx, "tint_colour"))
                            .flex()
                            .flex_wrap()
                            .gap(px(size::S2))
                            .children(dots)
                            .child(any),
                    )
                    .children(hue_row)
                    .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .child(ui::row(0, label, None, segmented, theme))
            .child(div().px(px(size::S4)).pb(px(size::S3)).child(below))
            .into_any_element()
    }
}

/// A control in a row takes up to half the card, so a slider has room to
/// be dragged with care.
pub(super) fn row_control(card: f32, control: impl IntoElement) -> gpui::Div {
    div().w(px((card * 0.5).min(320.0))).child(control)
}

/// A round colour to pick, ringed in the text's ink when it is the one.
fn swatch(
    id: impl Into<gpui::ElementId>,
    name: SharedString,
    picked: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    ui::pressable(id, theme)
        .role(Role::Button)
        .aria_label(name.clone())
        .aria_toggled(if picked {
            Toggled::True
        } else {
            Toggled::False
        })
        .tooltip(ui::tip(name))
        .size(px(26.0))
        .rounded_full()
        .map(|d| {
            if picked {
                d.border_2().border_color(theme.text)
            } else {
                d.border_1()
                    .border_color(crate::theme::oklch(0.0, 0.0, 0.0, 0.18))
            }
        })
}
