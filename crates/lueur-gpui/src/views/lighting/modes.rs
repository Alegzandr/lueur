//! What the lights picked do, after `lighting/Modes.svelte`: one grid of
//! fixed modes (the moods), one of animated ones (the screen and the shows),
//! each a framed photograph of the room under it, and under them the
//! settings of the animated ones playing there.

use super::LightingPage;
use super::controls::{picture, zone_badge};
use super::model::{self, ANIMATED, PACED, Room, current_feel, show_pace, times};
use crate::i18n::{fill, t};
use crate::state::View;
use crate::theme::{Theme, size};
use crate::ui::{self, choice};
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, FontWeight, IntoElement, Role, SharedString,
    Toggled, Window, div, ease_in_out, prelude::*, px,
};
use lueur_core::config::AudioSource;
use lueur_core::scene::{MOODS, NATURAL};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// From this column width, the grids take six columns rather than three.
const WIDE: f32 = 760.0;

/// How a tile stands: playing on every light picked, on some, or none.
#[derive(Clone, Copy, PartialEq)]
enum Plays {
    All,
    Some,
    None,
}

/// One tile of a grid: its id, its name, what plays, and its tip.
struct Tile {
    id: &'static str,
    name: SharedString,
    plays: Plays,
    tip: Option<SharedString>,
    span: u16,
    animated: bool,
    pending: bool,
    disabled: bool,
}

impl LightingPage {
    pub(super) fn modes(
        &self,
        width: f32,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let busy = l.busy;
        let pending = l.pending.clone();
        let ids = room.mood_targets();
        let names = |ids: &[String]| {
            ids.iter()
                .map(|id| room.name_of(id))
                .collect::<Vec<_>>()
                .join(", ")
        };

        // --- Fixed: the moods ---
        let mut fixed = Vec::new();
        for (id, tip) in std::iter::once((NATURAL, Some(t(cx, "mood_natural_tip"))))
            .chain(MOODS.iter().map(|m| (m.id, None)))
        {
            let showing: Vec<String> = ids
                .iter()
                .filter(|x| room.mood_of(x) == Some(id))
                .cloned()
                .collect();
            let plays = if !ids.is_empty() && showing.len() == ids.len() {
                Plays::All
            } else if showing.is_empty() {
                Plays::None
            } else {
                Plays::Some
            };
            fixed.push(Tile {
                id,
                name: t(cx, &format!("mood_{id}")),
                plays,
                tip: if plays == Plays::Some {
                    Some(names(&showing).into())
                } else {
                    tip
                },
                span: if id == NATURAL { 2 } else { 1 },
                animated: false,
                pending: pending.as_deref() == Some(id),
                disabled: pending.is_some(),
            });
        }

        // --- Animated: the screen and the shows ---
        let targets = room.effect_targets();
        let mut animated = Vec::new();
        for id in ANIMATED {
            let playing: Vec<String> = targets
                .iter()
                .filter(|x| room.mode_of(x) == Some(id))
                .cloned()
                .collect();
            let plays = if !targets.is_empty() && playing.len() == targets.len() {
                Plays::All
            } else if playing.is_empty() {
                Plays::None
            } else {
                Plays::Some
            };
            // Played by some of the lights picked: who, said on hover rather
            // than in the tile, which only carries a dashed ring.
            let tip = match plays {
                Plays::All => Some(t(cx, "click_to_stop")),
                Plays::Some => {
                    Some(format!("{}. {}", names(&playing), t(cx, "click_to_extend")).into())
                }
                Plays::None => None,
            };
            let is_pending = pending.as_deref() == Some(id);
            animated.push(Tile {
                id,
                name: t(cx, &format!("mode_{id}")),
                plays,
                tip,
                span: 1,
                animated: true,
                pending: is_pending,
                disabled: busy && !is_pending,
            });
        }

        let cols: u16 = if width >= WIDE { 6 } else { 3 };
        // Each picture is as high as a single column's is, the natural
        // light's too, so a row's pictures line up whatever their width.
        let pic_h =
            ((width - f32::from(cols - 1) * size::S2) / f32::from(cols) * 0.46 - 6.0).max(54.0);
        let reduce = cx.reduce_motion();
        let lueur = self.lueur.clone();
        let tile = |tile: Tile| -> AnyElement {
            let lueur = lueur.clone();
            let id = tile.id;
            let zoom = Rc::new(Cell::new(1.0));
            let pic = picture(model::picture_of(id), zoom.clone(), None, size::R_LG - 5.0)
                .h(px(pic_h))
                .min_h(px(54.0))
                .bg(theme.sunken);
            // An animated mode playing: its picture drifts closer, slowly; a
            // pending choice breathes. Both stand still under reduced motion,
            // the pending one dimmed instead.
            let pic = if tile.pending {
                if reduce {
                    pic.opacity(0.5).into_any_element()
                } else {
                    pic.with_animation(
                        SharedString::from(format!("breathe-{id}")),
                        Animation::new(Duration::from_millis(1600)).repeat(),
                        |d, t| d.opacity(1.0 - 0.55 * ease_in_out(1.0 - (2.0 * t - 1.0).abs())),
                    )
                    .into_any_element()
                }
            } else if tile.animated && tile.plays == Plays::All && !reduce {
                pic.with_animation(
                    SharedString::from(format!("drift-{id}")),
                    Animation::new(Duration::from_secs(16)).repeat(),
                    move |d, t| {
                        zoom.set(1.0 + 0.07 * ease_in_out(1.0 - (2.0 * t - 1.0).abs()));
                        d
                    },
                )
                .into_any_element()
            } else {
                pic.into_any_element()
            };
            let toggled = match tile.plays {
                Plays::All => Toggled::True,
                Plays::Some => Toggled::Mixed,
                Plays::None => Toggled::False,
            };
            let disabled = tile.disabled;
            ui::pressable(SharedString::from(format!("tile-{id}")), theme)
                .role(Role::Button)
                .aria_label(tile.name.clone())
                .aria_toggled(toggled)
                .when_some(tile.tip.clone(), |d, tip| {
                    d.aria_description(tip.clone()).tooltip(ui::tip(tip))
                })
                .col_span(tile.span)
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(7.0))
                .pt(px(5.0))
                .px(px(5.0))
                .pb(px(9.0))
                .rounded(px(size::R_LG))
                .bg(theme.material)
                // Playing on every light picked: an amber ring, an edge
                // rather than a colour alone; on some of them, a dashed one.
                .map(|d| match tile.plays {
                    Plays::All => d.border_2().border_color(theme.active),
                    Plays::Some => d.border_2().border_dashed().border_color(theme.active),
                    Plays::None => d.border_1().border_color(theme.separator),
                })
                .when(disabled && !tile.pending, |d| {
                    d.opacity(0.55).cursor_default()
                })
                .when(!disabled, |d| {
                    d.hover(|s| s.bg(theme.material_hover))
                        .on_click(move |_, _, cx| {
                            lueur.update(cx, |l, cx| {
                                if id == NATURAL || MOODS.iter().any(|m| m.id == id) {
                                    l.apply_mood(id, cx)
                                } else {
                                    l.choose_mode(id, cx)
                                }
                            })
                        })
                })
                .child(pic)
                // Names wrap rather than truncate: French ones run long.
                .child(
                    div()
                        .px(px(4.0))
                        .text_size(px(size::T_MD))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .line_height(px(16.0))
                        .child(tile.name),
                )
                .into_any_element()
        };
        let grid = |tiles: Vec<Tile>| {
            div()
                .grid()
                .grid_cols(cols)
                .gap(px(size::S2))
                .children(tiles.into_iter().map(&tile).collect::<Vec<_>>())
        };
        let kind = |text: SharedString| {
            div()
                .px(px(size::S1))
                .pb(px(size::S2))
                .text_size(px(size::T_MD))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.text_2)
                .child(text)
        };
        let fixed_title = t(cx, "modes_fixed");
        let animated_title = t(cx, "modes_animated");
        let fixed_grid = grid(fixed);
        let animated_grid = grid(animated);
        let settings = self.mode_settings(&ids, width, theme, window, cx);
        div()
            .id("modes")
            .role(Role::Group)
            .aria_label(t(cx, "mode"))
            .flex()
            .flex_col()
            .child(kind(fixed_title))
            .child(fixed_grid)
            .child(div().mt(px(size::S4)).child(kind(animated_title)))
            .child(animated_grid)
            .children(settings)
            .into_any_element()
    }

    /// The settings of what plays on the lights picked: what of the picture
    /// each one mirrors and how fast it follows, the shows' pace, and what
    /// the music listens to.
    fn mode_settings(
        &self,
        picked: &[String],
        width: f32,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let lang = l.lang();
        let modes: Vec<Option<&str>> = picked.iter().map(|id| room.mode_of(id)).collect();
        let screen = l.config.room.screen;
        let screen_lights: Vec<(String, String, lueur_core::geometry::LightMapping)> = picked
            .iter()
            .filter(|id| room.mode_of(id) == Some("ambilight"))
            .filter_map(|id| {
                let light = l.config.room.lights.iter().find(|x| &x.id == id)?;
                Some((
                    id.clone(),
                    light.name.clone(),
                    lueur_core::geometry::map_light(&screen, light),
                ))
            })
            .collect();
        let paced = modes.iter().any(|m| m.is_some_and(|m| PACED.contains(&m)));
        let music = modes.contains(&Some("music"));
        if screen_lights.is_empty() && !paced && !music {
            return None;
        }
        let status = l.status.clone();
        let config = l.config.clone();
        let one = picked.len() == 1;
        let lueur = self.lueur.clone();
        let mut rows: Vec<AnyElement> = Vec::new();

        if !screen_lights.is_empty() {
            if status.screen_lost {
                // The screen cannot be read: said in words, the colour only
                // marks it.
                let text = t(cx, "screen_lost");
                rows.push(
                    div()
                        .id("screen-lost")
                        .role(Role::Status)
                        .aria_label(text.clone())
                        .flex()
                        .items_center()
                        .gap(px(size::S2))
                        .px(px(size::S4))
                        .py(px(size::S2))
                        .text_size(px(size::T_SM))
                        .text_color(theme.danger)
                        .child(ui::icon("alert", 14.0, false, theme.danger))
                        .child(text)
                        .into_any_element(),
                );
            }
            // What of the picture each light mirrors, changed on the plan.
            for (i, (id, name, mapping)) in screen_lights.into_iter().enumerate() {
                use lueur_core::geometry::ScreenEdge;
                let (letter_key, zone_key) = match mapping.edge {
                    ScreenEdge::Left => ("zone_letter_left", "map_left"),
                    ScreenEdge::Center => ("zone_letter_center", "map_slice"),
                    ScreenEdge::Right => ("zone_letter_right", "map_right"),
                };
                let letter = t(cx, letter_key);
                let what = t(cx, model::describe(&mapping));
                let zone_of: SharedString = fill(
                    &t(cx, "zone_of"),
                    &[("letter", &letter), ("zone", &t(cx, zone_key))],
                )
                .into();
                let label: SharedString = if one {
                    t(cx, "mirrors")
                } else {
                    name.clone().into()
                };
                let change = t(cx, "change_on_plan");
                let button_label: SharedString =
                    fill(&t(cx, "labelled"), &[("name", &name), ("what", &change)]).into();
                let hdr = (i == 0 && status.hdr).then(|| {
                    // Read in HDR: a plain outlined tag, not amber, since it
                    // says how the picture is read, not that a light is on.
                    div()
                        .id("hdr")
                        .ml(px(size::S1))
                        .px(px(5.0))
                        .rounded(px(size::R_SM))
                        .border_1()
                        .border_color(theme.line_strong)
                        .text_size(px(size::T_XS))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_2)
                        .tooltip(ui::tip(t(cx, "hdr_tip")))
                        .child(t(cx, "hdr"))
                });
                let lueur = lueur.clone();
                rows.push(
                    div()
                        .id(("zone", i))
                        .role(Role::Group)
                        .aria_label(SharedString::from(format!("{label}: {zone_of}, {what}")))
                        .flex()
                        .items_center()
                        .gap(px(size::S3))
                        .min_h(px(44.0))
                        .pl(px(size::S4))
                        .pr(px(size::S2))
                        .when(!rows.is_empty(), |d| {
                            d.border_t_1().border_color(theme.separator)
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(size::T_MD))
                                .text_color(theme.text)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .whitespace_nowrap()
                                .text_size(px(size::T_MD))
                                .text_color(theme.text_2)
                                .child(zone_badge(mapping.edge, letter, theme))
                                .child(what)
                                .children(hdr),
                        )
                        .child(
                            super::controls::btn(
                                ("zone-plan", i),
                                super::controls::Kind::Plain,
                                true,
                                false,
                                theme,
                            )
                            .w(px(24.0))
                            .px_0()
                            .aria_label(button_label)
                            .tooltip(ui::tip(change))
                            .child(ui::icon("plan", 14.0, false, theme.text_2))
                            .on_click(move |_, _, cx| {
                                lueur.update(cx, |l, cx| l.show_on_plan(&id, cx))
                            }),
                        )
                        .into_any_element(),
                );
            }
            // The three feels, explained together behind the row's info mark.
            let feels: Vec<_> = model::FEELS
                .iter()
                .map(|(f, _)| {
                    choice(*f, t(cx, &format!("feel_{f}"))).tip(t(cx, &format!("feel_{f}_tip")))
                })
                .collect();
            let feel_tip: SharedString = model::FEELS
                .iter()
                .map(|(f, _)| {
                    format!(
                        "{}: {}",
                        t(cx, &format!("feel_{f}")),
                        t(cx, &format!("feel_{f}_tip"))
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
                .into();
            let response = t(cx, "response");
            let feel = current_feel(&config);
            let lueur = lueur.clone();
            let control = ui::segmented(
                "response",
                response.clone(),
                feel.as_ref(),
                feels,
                theme,
                move |f: &'static str, _, cx| {
                    lueur.update(cx, |l, cx| l.edit(cx, |c| model::apply_feel(c, f)))
                },
            );
            rows.push(
                ui::row(rows.len(), response, Some(feel_tip), control, theme).into_any_element(),
            );
        }

        // The music keeps the track's own time: beside it, a speed would read
        // as the music's.
        if paced && !music {
            let label = t(cx, "speed");
            let lueur = lueur.clone();
            let default = f32::from(lueur_core::show::ShowSettings::default().speed);
            let control = ui::slider(
                "show-speed",
                label.clone(),
                f32::from(config.show.speed),
                (1.0, 100.0, 1.0),
                move |v, _, cx| {
                    lueur.update(cx, |l, cx| l.edit(cx, |c| c.show.speed = v.round() as u8))
                },
            )
            .default_value(
                default,
                fill(
                    &t(cx, "reset_to"),
                    &[("v", &times(lang, show_pace(default)))],
                )
                .into(),
            )
            .format(move |v| times(lang, show_pace(v)))
            .ends(t(cx, "speed_slow"), t(cx, "speed_fast"))
            .render(theme, window, cx);
            rows.push(
                ui::row(
                    rows.len(),
                    label,
                    None,
                    super::tune::row_control(width, control),
                    theme,
                )
                .into_any_element(),
            );
        }

        if music {
            let label = t(cx, "flashes");
            let on = config.show.flashes;
            let lueur_flash = lueur.clone();
            let control = ui::switch("flashes", label.clone(), on, theme, move |_, _, cx| {
                lueur_flash.update(cx, |l, cx| l.edit(cx, |c| c.show.flashes = !on))
            });
            rows.push(
                ui::row(
                    rows.len(),
                    label,
                    Some(t(cx, "flashes_tip")),
                    control,
                    theme,
                )
                .into_any_element(),
            );
            // What the music listens to, by the name the source had when
            // picked.
            let sound: String = match &config.audio {
                AudioSource::Default => t(
                    cx,
                    if cfg!(windows) {
                        "audio_default_output"
                    } else {
                        "audio_default_input"
                    },
                )
                .to_string(),
                AudioSource::Device { name, .. } | AudioSource::App { name, .. } => name.clone(),
            };
            let waiting = status.music_waiting;
            let shown: SharedString = if waiting {
                fill(&t(cx, "sound_waiting"), &[("name", &sound)]).into()
            } else {
                sound.into()
            };
            let change = t(cx, "change");
            let lueur = lueur.clone();
            let control = div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(size::S2))
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(size::T_MD))
                        .text_color(if waiting { theme.text_3 } else { theme.text })
                        .child(shown),
                )
                .child(
                    super::controls::btn(
                        "listen-change",
                        super::controls::Kind::Plain,
                        true,
                        false,
                        theme,
                    )
                    .aria_label(change.clone())
                    .child(change)
                    .on_click(move |_, _, cx| lueur.update(cx, |l, cx| l.show(View::Settings, cx))),
                );
            rows.push(
                ui::row(rows.len(), t(cx, "listens_to"), None, control, theme).into_any_element(),
            );
        }

        Some(
            ui::card(theme)
                .mt(px(size::S3))
                .children(rows)
                .into_any_element(),
        )
    }
}
