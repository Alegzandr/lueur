//! The scenes the user saved, first on the page: the whole room as it was,
//! back in one click.
//! Saving asks for a name in place; deleting asks first, in the scene's own
//! place, with an undo after.

use super::LightingPage;
use super::controls::{Kind, btn, group_title, lamp, picture};
use super::lights::columns;
use super::model::{Look, Room, distinct_looks, scene_named};
use crate::i18n::{fill, t};
use crate::theme::{Theme, size};
use crate::ui::{self, InputEvent, TextInput};
use gpui::{
    Animation, AnimationExt, AnyElement, AppContext, Context, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Role, SharedString, Toggled, Window, div, ease_in_out, prelude::*, px,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// The narrowest a scene's card gets before the row takes one fewer.
const SCENE_MIN: f32 = 150.0;

impl LightingPage {
    /// Open the field a scene's name is typed in, with the keyboard in it.
    fn start_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            TextInput::new(
                cx,
                "",
                crate::i18n::t(cx, "scene_name"),
                crate::i18n::t(cx, "scene_name_hint"),
                40,
            )
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, e: &InputEvent, window, cx| match e {
                InputEvent::Submit => {
                    let name = input.read(cx).text().trim().to_string();
                    if !name.is_empty() {
                        this.lueur.update(cx, |l, cx| l.save_scene(&name, cx));
                        this.stop_naming(window, cx);
                    }
                }
                InputEvent::Changed => cx.notify(),
            },
        );
        self._subscriptions.push(sub);
        window.focus(&input.focus_handle(cx), cx);
        self.naming = Some(input);
        cx.notify();
    }

    /// Close the field; the keyboard goes back to where it came from.
    fn stop_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        window.focus(&self.save_focus, cx);
        cx.notify();
    }

    /// Keep a scene asked about; the keyboard goes back to its delete mark.
    fn keep_scene(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.asked = self.confirming.take();
        window.focus(&self.delete_focus, cx);
        cx.notify();
    }

    pub(super) fn scenes(
        &mut self,
        width: f32,
        theme: &Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let scenes = l.config.scenes.clone();
        let busy = l.busy;
        let pending = l.pending.clone();
        // A scene as the room it makes: the photograph of each thing its
        // lights do, side by side, in the order of the room.
        let looks: Vec<Vec<Look>> = scenes
            .iter()
            .map(|s| {
                distinct_looks(
                    l.config.room.lights.iter().filter_map(|x| {
                        s.lights
                            .get(&x.id)
                            .filter(|_| room.bound(&x.id))
                            .map(Look::of_scene)
                    }),
                    3,
                )
            })
            .collect();
        let showing: Vec<bool> = scenes.iter().map(|s| room.shows_scene(s)).collect();
        let typed = self
            .naming
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string());
        let replacing = typed
            .as_deref()
            .filter(|n| !n.is_empty())
            .and_then(|n| scene_named(&scenes, n))
            .map(|s| s.name.clone());
        let page = cx.entity().downgrade();

        let save_label = t(cx, "scene_save");
        let head_button = (self.naming.is_none() && !scenes.is_empty()).then(|| {
            let page = page.clone();
            btn("scene-save", Kind::Plain, true, false, theme)
                .track_focus(&self.save_focus)
                .aria_label(save_label.clone())
                .tooltip(ui::tip(t(cx, "scene_save_tip")))
                .child(ui::icon("plus", 13.0, false, theme.text_2))
                .child(save_label.clone())
                .on_click(move |_, window, cx| {
                    let _ = page.update(cx, |p, cx| p.start_naming(window, cx));
                })
        });

        let naming = self.naming.clone().map(|input| {
            let empty = typed.as_deref().is_none_or(str::is_empty);
            let confirm = t(
                cx,
                if replacing.is_some() {
                    "scene_replace"
                } else {
                    "scene_save_confirm"
                },
            );
            let hint: SharedString = match &replacing {
                Some(name) => fill(&t(cx, "scene_replace_tip"), &[("name", name)]).into(),
                None => t(cx, "scene_save_tip"),
            };
            let cancel = t(cx, "sleep_cancel");
            let (p1, p2, p3) = (page.clone(), page.clone(), page.clone());
            let field = input.clone();
            div()
                .flex()
                .flex_col()
                .gap(px(size::S1))
                .child(
                    ui::card(theme)
                        .flex_row()
                        .items_center()
                        .gap(px(size::S2))
                        .p(px(size::S2))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .on_key_down(move |e: &KeyDownEvent, window, cx| {
                                    if e.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        let _ = p1.update(cx, |p, cx| p.stop_naming(window, cx));
                                    }
                                })
                                .child(input),
                        )
                        .child(
                            btn("scene-confirm", Kind::Default, true, empty, theme)
                                .aria_label(confirm.clone())
                                .child(confirm)
                                .when(!empty, |b| {
                                    b.on_click(move |_, window, cx| {
                                        let name = field.read(cx).text().trim().to_string();
                                        let _ = p2.update(cx, |p, cx| {
                                            p.lueur.update(cx, |l, cx| l.save_scene(&name, cx));
                                            p.stop_naming(window, cx);
                                        });
                                    })
                                }),
                        )
                        .child(
                            btn("scene-cancel", Kind::Plain, true, false, theme)
                                .aria_label(cancel.clone())
                                .child(cancel)
                                .on_click(move |_, window, cx| {
                                    let _ = p3.update(cx, |p, cx| p.stop_naming(window, cx));
                                }),
                        ),
                )
                .child(
                    div()
                        .px(px(size::S1))
                        .text_size(px(size::T_SM))
                        .text_color(theme.text_3)
                        .child(hint),
                )
        });

        // No scene yet: the place one goes, which saves the first.
        let empty = (self.naming.is_none() && scenes.is_empty()).then(|| {
            let page = page.clone();
            ui::pressable("scene-empty", theme)
                .track_focus(&self.save_focus)
                .role(Role::Button)
                .aria_label(save_label.clone())
                .aria_description(t(cx, "scenes_empty"))
                .flex()
                .items_center()
                .gap(px(size::S3))
                .w_full()
                .min_h(px(90.0))
                .p(px(size::S3))
                .rounded(px(size::R_LG))
                .border_1()
                .border_color(theme.line_strong)
                .hover(|s| s.bg(theme.material_hover))
                .child(
                    div()
                        .flex_none()
                        .size(px(28.0))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .border_1()
                        .border_dashed()
                        .border_color(theme.line_strong)
                        .child(ui::icon("plus", 14.0, false, theme.text_2)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(size::T_MD))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(save_label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(size::T_SM))
                                .text_color(theme.text_2)
                                .child(t(cx, "scenes_empty")),
                        ),
                )
                .on_click(move |_, window, cx| {
                    let _ = page.update(cx, |p, cx| p.start_naming(window, cx));
                })
        });

        let reduce = cx.reduce_motion();
        let mut cards = Vec::new();
        for (i, scene) in scenes.iter().enumerate() {
            let name: SharedString = scene.name.clone().into();
            if self.confirming.as_deref() == Some(scene.id.as_str()) {
                let ask: SharedString =
                    fill(&t(cx, "scene_delete_ask"), &[("name", &scene.name)]).into();
                let delete = t(cx, "delete");
                let cancel = t(cx, "sleep_cancel");
                let id = scene.id.clone();
                let (p1, p2, p3) = (page.clone(), page.clone(), page.clone());
                cards.push(
                    div()
                        .id(("scene-confirm", i))
                        .role(Role::Group)
                        .aria_label(ask.clone())
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(size::S2))
                        .min_h(px(90.0))
                        .pl(px(size::S3))
                        .pr(px(size::S2))
                        .rounded(px(size::R_LG))
                        .bg(theme.material)
                        .border_1()
                        .border_color(theme.separator)
                        // Escape keeps the scene.
                        .on_key_down(move |e: &KeyDownEvent, window, cx| {
                            if e.keystroke.key == "escape" {
                                cx.stop_propagation();
                                let _ = p1.update(cx, |p, cx| p.keep_scene(window, cx));
                            }
                        })
                        .child(
                            div()
                                .truncate()
                                .text_size(px(size::T_MD))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(ask),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(size::S1))
                                .child(
                                    btn(("scene-delete-yes", i), Kind::Danger, true, false, theme)
                                        .track_focus(&self.confirm_focus)
                                        .aria_label(delete.clone())
                                        .child(delete)
                                        .on_click(move |_, _, cx| {
                                            let id = id.clone();
                                            let _ = p2.update(cx, |p, cx| {
                                                p.confirming = None;
                                                p.lueur.update(cx, |l, cx| l.delete_scene(&id, cx));
                                            });
                                        }),
                                )
                                .child(
                                    btn(("scene-delete-no", i), Kind::Plain, true, false, theme)
                                        .aria_label(cancel.clone())
                                        .child(cancel)
                                        .on_click(move |_, window, cx| {
                                            let _ = p3.update(cx, |p, cx| p.keep_scene(window, cx));
                                        }),
                                ),
                        )
                        .into_any_element(),
                );
                continue;
            }
            let on = showing[i];
            let is_pending = pending.as_deref() == Some(scene.id.as_str());
            let disabled = busy && !is_pending;
            let slices = looks[i].iter().map(|look| {
                picture(look.picture, Rc::new(Cell::new(1.0)), look.tint, 6.0)
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(theme.sunken)
            });
            let slices = div()
                .flex()
                .gap(px(2.0))
                .h(px(48.0))
                .children(slices)
                .when(is_pending && reduce, |d| d.opacity(0.5));
            // A scene coming back breathes; under reduced motion it dims.
            let slices = if is_pending && !reduce {
                slices
                    .with_animation(
                        SharedString::from(format!("scene-breathe-{i}")),
                        Animation::new(Duration::from_millis(1600)).repeat(),
                        |d, t| d.opacity(1.0 - 0.55 * ease_in_out(1.0 - (2.0 * t - 1.0).abs())),
                    )
                    .into_any_element()
            } else {
                slices.into_any_element()
            };
            let lueur = self.lueur.clone();
            let recalled = scene.clone();
            let group = SharedString::from(format!("scene-{i}"));
            let del_label: SharedString =
                fill(&t(cx, "scene_delete"), &[("name", &scene.name)]).into();
            let page_del = page.clone();
            let del_id = scene.id.clone();
            cards.push(
                div()
                    .id(("scene", i))
                    .group(group.clone())
                    .relative()
                    .rounded(px(size::R_LG))
                    .bg(theme.material)
                    // Showing now: an amber ring and a lamp, not the colour
                    // alone.
                    .map(|d| {
                        if on {
                            d.border_2().border_color(theme.active)
                        } else {
                            d.border_1().border_color(theme.separator)
                        }
                    })
                    .child(
                        ui::pressable(("scene-recall", i), theme)
                            .role(Role::Button)
                            .aria_label(name.clone())
                            .aria_toggled(if on { Toggled::True } else { Toggled::False })
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(7.0))
                            .pt(px(5.0))
                            .px(px(5.0))
                            .pb(px(9.0))
                            .rounded(px(size::R_LG))
                            .when(disabled, |d| d.opacity(0.6).cursor_default())
                            .when(!disabled, |d| {
                                d.hover(|s| s.bg(theme.material_hover))
                                    .on_click(move |_, _, cx| {
                                        let scene = recalled.clone();
                                        lueur.update(cx, |l, cx| l.recall_scene(scene, cx))
                                    })
                            })
                            .child(slices)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .min_w_0()
                                    .px(px(4.0))
                                    .text_size(px(size::T_MD))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text)
                                    .when(on, |d| d.child(lamp(6.0, theme)))
                                    .child(div().min_w_0().truncate().child(name.clone())),
                            ),
                    )
                    // Delete waits out of sight until the scene is hovered or
                    // reached by the keyboard.
                    .child(
                        ui::pressable(("scene-del", i), theme)
                            .when(self.asked.as_deref() == Some(scene.id.as_str()), |d| {
                                d.track_focus(&self.delete_focus)
                            })
                            .role(Role::Button)
                            .aria_label(del_label.clone())
                            .tooltip(ui::tip(del_label))
                            .absolute()
                            .top(px(9.0))
                            .right(px(9.0))
                            .size(px(20.0))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.line)
                            .opacity(0.0)
                            .group_hover(group, |s| s.opacity(1.0))
                            .focus_visible(|s| s.opacity(1.0))
                            .child(ui::icon("close", 11.0, false, theme.text_2))
                            .on_click(move |_, window, cx| {
                                let id = del_id.clone();
                                let _ = page_del.update(cx, |p, cx| {
                                    p.confirming = Some(id);
                                    window.focus(&p.confirm_focus, cx);
                                    cx.notify();
                                });
                            }),
                    )
                    .into_any_element(),
            );
        }

        let title = t(cx, "scenes");
        div()
            .id("scenes")
            .role(Role::Group)
            .aria_label(title.clone())
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
                    .child(group_title(title, theme))
                    .children(head_button),
            )
            .children(naming)
            .children(empty)
            .when(!cards.is_empty(), |d| {
                d.child(
                    div()
                        .when(self.naming.is_some(), |d| d.mt(px(size::S3)))
                        .grid()
                        .grid_cols(columns(width, SCENE_MIN, size::S2) as u16)
                        .gap(px(size::S2))
                        .children(cards),
                )
            })
            .into_any_element()
    }
}
