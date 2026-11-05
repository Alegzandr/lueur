//! What the network offers: the bulbs found that no light has yet, each
//! with what can be done with it (pair, identify, give to a light, add),
//! and the tips for a bulb that does not show up.

use super::brands::{device_brand, supports_music};
use super::controls::{Look, btn, disclosure, group_head, note, spinner};
use super::pairing::PairStep;
use super::{BulbsPage, actions};
use crate::i18n::{fill, t, tn};
use crate::theme::{Theme, size, theme};
use crate::ui;
use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, FontWeight, KeyDownEvent, Role, SharedString,
    anchored, deferred, div, prelude::*, px,
};
use lueur_core::device::DiscoveredDevice;

/// What "Use for…" can give a bulb to.
#[derive(Clone)]
enum Target {
    Light(String),
    NewLight,
}

impl BulbsPage {
    /// Smart Life bulbs are never found by searching alone: say so wherever
    /// the list is, until some have been imported.
    fn tuya_hint(&self, cx: &App) -> bool {
        self.tuya.is_none()
            && !self.discovered.iter().any(|d| d.kind.family() == "tuya")
            && !self
                .lueur
                .read(cx)
                .config
                .bindings
                .iter()
                .any(|b| b.kind.family() == "tuya")
    }

    /// Give bulb `d` to a light, or to a new one.
    fn give(&mut self, d: &DiscoveredDevice, target: Target, cx: &mut Context<Self>) {
        let name = self.device_name(d, cx);
        let d = d.clone();
        self.lueur.update(cx, |l, cx| match target {
            Target::NewLight => actions::add_devices(l, vec![(d, name)], true, cx),
            Target::Light(id) => actions::assign_device(l, &id, &d, &name, cx),
        });
        self.menu = None;
        cx.notify();
    }

    pub(super) fn network_column(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *theme(cx);
        let unbound = self.unbound(cx);
        let no_lights = self.lueur.read(cx).config.room.lights.is_empty();
        let mut col = div().flex().flex_col().gap(px(size::S6));
        if !unbound.is_empty() || no_lights {
            let head = self.handle("network-head", false, cx);
            let count = (self.has_scanned && !self.scanning)
                .then(|| note(tn(cx, "n_found", unbound.len()), &theme).into_any_element());
            let body = if unbound.is_empty() {
                self.empty_state(&theme, cx)
            } else {
                self.device_rows(&unbound, &theme, cx)
            };
            col = col.child(
                div()
                    .child(group_head(
                        "network-title",
                        t(cx, "on_network"),
                        Some(&head),
                        count,
                        &theme,
                    ))
                    .child(ui::card(&theme).child(body)),
            );
        }
        col.child(self.tips(&theme, cx)).into_any_element()
    }

    fn empty_state(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let hint = self.tuya_hint(cx);
        let title = |text: SharedString| {
            div()
                .text_color(theme.text)
                .font_weight(FontWeight::SEMIBOLD)
                .child(text)
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(size::S1))
            .px(px(size::S5))
            .py(px(size::S6))
            .text_size(px(size::T_MD))
            .text_color(theme.text_3)
            .text_center()
            .map(|d| {
                if self.scanning {
                    d.child(t(cx, "listening"))
                } else if self.has_scanned {
                    d.child(title(t(cx, "none_title")))
                } else {
                    d.child(title(t(cx, "first_title")))
                        .child(t(cx, "first_text"))
                }
            })
            .when(hint, |d| {
                d.child(div().mt(px(size::S3)).child(t(cx, "tuya_hint")))
                    .child(div().mt(px(size::S2)).child(btn(
                        "tuya-hint-open",
                        t(cx, "tuya_title"),
                        Look::Default,
                        true,
                        false,
                        None,
                        theme,
                        cx.listener(|this, _, window, cx| this.open_tuya(window, cx)),
                    )))
            })
            .into_any_element()
    }

    fn device_rows(
        &mut self,
        unbound: &[DiscoveredDevice],
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Addresses mean nothing to most people: a bulb is told apart by
        // its name, brand and model, and its address only shows when two
        // share a name.
        let names: Vec<String> = unbound.iter().map(|d| self.device_name(d, cx)).collect();
        let shared = |name: &str| names.iter().filter(|n| *n == name).count() > 1;
        let mut rows: Vec<AnyElement> = Vec::new();
        for (i, (d, name)) in unbound.iter().zip(&names).enumerate() {
            let row = self.device_row(i, d, name, shared(name), theme, cx);
            rows.push(row);
        }
        if self.tuya_hint(cx) {
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::S3))
                    .min_h(px(44.0))
                    .py(px(size::S2))
                    .pr(px(size::S3))
                    .pl(px(size::S4))
                    .border_t_1()
                    .border_color(theme.separator)
                    .child(note(t(cx, "tuya_hint"), theme).flex_1())
                    .child(btn(
                        "tuya-hint-open",
                        t(cx, "tuya_title"),
                        Look::Plain,
                        true,
                        false,
                        None,
                        theme,
                        cx.listener(|this, _, window, cx| this.open_tuya(window, cx)),
                    ))
                    .into_any_element(),
            );
        }
        div().flex().flex_col().children(rows).into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn device_row(
        &mut self,
        index: usize,
        d: &DiscoveredDevice,
        name: &str,
        shared_name: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = |what: &str| ElementId::Name(SharedString::from(format!("{what}-{}", d.id)));
        // What stops it from doing all a bulb can, in amber like any light
        // waiting on the user, and in words.
        let warn = if d.faster {
            Some(t(cx, "pair_faster"))
        } else if !d.paired {
            Some(t(cx, "needs_pairing"))
        } else if !supports_music(d) {
            Some(t(cx, "no_music"))
        } else {
            None
        };
        let meta = div()
            .text_size(px(size::T_SM))
            .text_color(theme.text_3)
            .child(device_brand(d))
            .when(shared_name, |m| m.child(format!(" · {}", d.ip)))
            .when_some(warn, |m, w| {
                m.child(div().text_color(theme.glow_text).child(format!(", {w}")))
            })
            .flex()
            .flex_wrap();
        let unbound_lights: Vec<(String, String)> = {
            let c = &self.lueur.read(cx).config;
            c.room
                .lights
                .iter()
                .filter(|l| c.binding_for(&l.id).is_none())
                .map(|l| (l.id.clone(), l.name.clone()))
                .collect()
        };
        let action: AnyElement = if !d.paired {
            let waiting = self
                .pairing
                .as_ref()
                .is_some_and(|p| matches!(p.step, PairStep::Waiting));
            let this_one = waiting && self.pairing.as_ref().is_some_and(|p| p.device.id == d.id);
            let focus = self.handle(&format!("pair-{}", d.id), true, cx);
            let label = t(
                cx,
                if this_one {
                    "pairing"
                } else if d.faster {
                    "pair_again"
                } else {
                    "pair"
                },
            );
            let device = d.clone();
            btn(
                key("pair"),
                label,
                if d.faster { Look::Plain } else { Look::Default },
                true,
                waiting,
                this_one.then(|| spinner(key("pair-spin"), 12.0, theme.text_3, cx.reduce_motion())),
                theme,
                cx.listener(move |this, _, _, cx| this.pair(device.clone(), cx)),
            )
            .when(!waiting, |b| b.track_focus(&focus))
            .tooltip(ui::tip(t(
                cx,
                if d.faster {
                    "pair_faster_tip"
                } else {
                    "pair_tip"
                },
            )))
            .into_any_element()
        } else if !unbound_lights.is_empty() {
            self.use_for(d, name, unbound_lights, theme, cx)
        } else {
            let device = d.clone();
            btn(
                key("add"),
                t(cx, "add"),
                Look::Default,
                true,
                false,
                None,
                theme,
                cx.listener(move |this, _, _, cx| this.give(&device, Target::NewLight, cx)),
            )
            .into_any_element()
        };
        let identify = d.paired.then(|| {
            let (lueur, endpoint) = (self.lueur.clone(), d.endpoint());
            btn(
                key("identify"),
                t(cx, "identify"),
                Look::Plain,
                true,
                false,
                None,
                theme,
                move |_, _, cx| {
                    let endpoint = endpoint.clone();
                    lueur.update(cx, |l, cx| actions::identify(l, endpoint, cx));
                },
            )
            .tooltip(ui::tip(t(cx, "identify_tip")))
        });
        div()
            .flex()
            .items_center()
            .gap(px(size::S3))
            .min_h(px(50.0))
            .py(px(size::S2))
            .pr(px(size::S3))
            .pl(px(size::S4))
            .when(index > 0, |r| r.border_t_1().border_color(theme.separator))
            // A bulb found but not in the room yet: a dashed place, like a
            // light with no bulb, holding the bulb mark.
            .child(
                div()
                    .size(px(28.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_dashed()
                    .border_color(theme.line_strong)
                    .child(ui::icon("bulb", 14.0, false, theme.text_2)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(size::T_MD))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .truncate()
                            .child(name.to_string()),
                    )
                    .child(meta),
            )
            .children(identify)
            .child(action)
            .into_any_element()
    }

    /// "Use for…": a button that opens the list of the lights still
    /// without a bulb, and a new light. Escape closes it; the arrows move
    /// through it.
    fn use_for(
        &mut self,
        d: &DiscoveredDevice,
        name: &str,
        lights: Vec<(String, String)>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menu.as_deref() == Some(d.id.as_str());
        let button_key = format!("use-{}", d.id);
        let button_focus = self.handle(&button_key, true, cx);
        let label = t(cx, "use_for");
        let aria = fill(&t(cx, "labelled"), &[("name", name), ("what", &label)]);
        let id = d.id.clone();
        let toggle = cx.listener(move |this, _, _, cx| {
            this.menu = if this.menu.as_deref() == Some(id.as_str()) {
                None
            } else {
                // Opened, the keyboard goes to its first choice.
                this.focus_next = Some(format!("use-{id}-0"));
                Some(id.clone())
            };
            cx.notify();
        });
        let button = ui::pressable(ElementId::Name(button_key.clone().into()), theme)
            .track_focus(&button_focus)
            .role(Role::Button)
            .aria_label(SharedString::from(aria))
            .aria_expanded(open)
            .h(px(26.0))
            .max_w(px(180.0))
            .pl(px(size::S3))
            .pr(px(size::S2))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(size::S2))
            .rounded(px(7.0))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.line)
            .hover(|s| s.bg(theme.surface_hover))
            .text_size(px(size::T_SM))
            .text_color(theme.text)
            .child(div().truncate().child(label))
            .child(
                ui::icon("chevron", 12.0, false, theme.text_3)
                    .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.25))),
            )
            .on_click(toggle);

        let list =
            open.then(|| {
                let mut choices: Vec<(Target, SharedString)> = lights
                    .into_iter()
                    .map(|(id, name)| (Target::Light(id), name.into()))
                    .collect();
                choices.push((Target::NewLight, t(cx, "new_light")));
                let handles: Vec<FocusHandle> = (0..choices.len())
                    .map(|i| self.handle(&format!("use-{}-{i}", d.id), true, cx))
                    .collect();
                let keys = {
                    let handles = handles.clone();
                    let button_key = button_key.clone();
                    cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                        let at = handles.iter().position(|h| h.is_focused(window));
                        let last = handles.len() - 1;
                        let to = match (e.keystroke.key.as_str(), at) {
                            ("escape", _) => {
                                cx.stop_propagation();
                                this.menu = None;
                                this.focus_next = Some(button_key.clone());
                                cx.notify();
                                return;
                            }
                            ("down", Some(i)) => (i + 1).min(last),
                            ("up", Some(i)) => i.saturating_sub(1),
                            ("home", _) => 0,
                            ("end", _) => last,
                            _ => return,
                        };
                        cx.stop_propagation();
                        handles[to].focus(window, cx);
                    })
                };
                let items = choices.into_iter().zip(handles).enumerate().map(
                    |(i, ((target, text), focus))| {
                        let device = d.clone();
                        ui::pressable(ElementId::Name(format!("use-{}-{i}", d.id).into()), theme)
                            .track_focus(&focus)
                            .role(Role::MenuItem)
                            .aria_label(text.clone())
                            .h(px(28.0))
                            .px(px(size::S3))
                            .flex()
                            .items_center()
                            .rounded(px(size::R_SM))
                            .hover(|s| s.bg(theme.surface_hover))
                            .text_size(px(size::T_MD))
                            .text_color(theme.text)
                            .child(text)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.give(&device, target.clone(), cx)
                            }))
                    },
                );
                let close = cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                });
                deferred(
                    anchored().snap_to_window().child(
                        div()
                            .id(ElementId::Name(format!("use-{}-list", d.id).into()))
                            .role(Role::Menu)
                            .on_key_down(keys)
                            .mt(px(30.0))
                            .min_w(px(180.0))
                            .max_h(px(320.0))
                            .overflow_y_scroll()
                            .p(px(4.0))
                            .rounded(px(size::R_MD))
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.line)
                            .shadow_lg()
                            .on_mouse_down_out(close)
                            .children(items),
                    ),
                )
                .with_priority(1)
            });
        div()
            .flex()
            .flex_col()
            .child(button)
            .children(list)
            .into_any_element()
    }

    /// The tips for a bulb that does not show up, open by themselves when
    /// a search found nothing at all.
    fn tips(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let open = self
            .tips_open
            .unwrap_or(self.has_scanned && self.discovered.is_empty());
        let items = ["tip_1", "tip_2", "tip_3", "tip_4", "tip_5"]
            .into_iter()
            .enumerate()
            .map(|(i, key)| {
                div()
                    .flex()
                    .gap(px(size::S2))
                    .child(div().w(px(16.0)).flex_none().child(format!("{}.", i + 1)))
                    .child(div().flex_1().min_w_0().child(t(cx, key)))
            });
        ui::card(theme)
            .child(
                div().px(px(size::S4)).py(px(size::S3)).child(
                    disclosure(
                        "tips",
                        t(cx, "tips_title"),
                        open,
                        theme,
                        cx.listener(move |this, _, _, cx| {
                            this.tips_open = Some(!open);
                            cx.notify();
                        }),
                    )
                    .text_size(px(size::T_MD))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text),
                ),
            )
            .when(open, |c| {
                c.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(size::S2))
                        .pl(px(size::S4 + 22.0))
                        .pr(px(size::S5))
                        .pb(px(size::S4))
                        .text_size(px(size::T_MD))
                        .text_color(theme.text_2)
                        .children(items),
                )
            })
            .into_any_element()
    }
}
