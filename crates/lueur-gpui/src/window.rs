//! The main window: the sidebar and the page it shows, after `src/App.svelte`.

use crate::app::{
    GoBulbs, GoLighting, GoRoom, GoSettings, NextControl, PreviousControl, ToggleEffect,
};
use crate::i18n::{self, t};
use crate::state::{Lueur, View, model};
use crate::theme::{Theme, size, theme};
use crate::ui;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, IntoElement, Render, Role,
    SharedString, Window, div, img, prelude::*, px,
};

pub struct MainWindow {
    lueur: Entity<Lueur>,
    focus: FocusHandle,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lueur = model(cx);
        cx.observe(&lueur, |_, _, cx| cx.notify()).detach();
        // The OS switched light or dark: follow it, unless the settings
        // force one.
        cx.observe_window_appearance(window, |this, _, cx| {
            let appearance = this.lueur.read(cx).config.interface.appearance;
            crate::state::apply_theme(appearance, cx);
        })
        .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        MainWindow { lueur, focus }
    }

    fn go(&mut self, view: View, cx: &mut Context<Self>) {
        self.lueur.update(cx, |l, cx| l.show(view, cx));
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let current = self.lueur.read(cx).view;
        let mod_key = if cfg!(target_os = "macos") {
            "Meta"
        } else {
            "Control"
        };
        let nav = View::ALL.iter().enumerate().map(|(i, &view)| {
            let selected = view == current;
            let label = t(cx, view.label_key());
            let fg = if selected { theme.text } else { theme.text_2 };
            ui::pressable(
                SharedString::from(format!("nav-{}", view.label_key())),
                theme,
            )
            .role(Role::Tab)
            .aria_label(label.clone())
            .aria_selected(selected)
            .aria_keyshortcuts(SharedString::from(format!("{mod_key}+{}", i + 1)))
            .h(px(32.0))
            .px(px(size::S3))
            .flex()
            .items_center()
            .gap(px(size::S3))
            .rounded(px(size::R_MD))
            .when(selected, |d| d.bg(theme.select_soft))
            .hover(|s| s.bg(theme.select_soft))
            .text_color(fg)
            .text_size(px(size::T_MD))
            .when(selected, |d| d.font_weight(FontWeight::SEMIBOLD))
            .child(ui::icon(view.icon(), 18.0, selected, fg))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.lueur.update(cx, |l, cx| l.show(view, cx));
            }))
        });
        div()
            .id("sidebar")
            .role(Role::Navigation)
            .w(px(220.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(size::S1))
            .p(px(size::S3))
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::S2))
                    .px(px(size::S2))
                    .pt(px(size::S2))
                    .pb(px(size::S5))
                    .child(img("logo-mark.svg").size(px(22.0)))
                    .child(
                        div()
                            .text_size(px(size::T_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("Lueur"),
                    ),
            )
            .child(
                div()
                    .id("pages")
                    .role(Role::TabList)
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children(nav),
            )
            .into_any_element()
    }

    fn page(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let view = self.lueur.read(cx).view;
        let title = ui::page_title("page-title", t(cx, view.label_key()), theme);
        let body = match view {
            View::Lighting => self.lighting(theme, cx),
            // Not yet drawn in GPUI: the Tauri interface still has them.
            View::Room | View::Bulbs | View::Settings => div().into_any_element(),
        };
        div()
            .id("page")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(size::S5))
            .px(px(size::S6 + size::S2))
            .py(px(size::S6))
            .child(title)
            .child(body)
            .into_any_element()
    }

    fn lighting(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let l = self.lueur.read(cx);
        let running = l.status.running;
        let has_lights = !l.config.bindings.is_empty();
        let lights: Vec<(String, String, bool, bool, bool)> = l
            .config
            .room
            .lights
            .iter()
            .map(|light| {
                let bound = l.config.binding_for(&light.id).is_some();
                let on = l.config.states.get(&light.id).is_none_or(|s| s.on);
                let driven = l.status.light_ids.contains(&light.id);
                (light.id.clone(), light.name.clone(), bound, on, driven)
            })
            .collect();

        let toggle_label = t(
            cx,
            if running {
                "stop_effects"
            } else {
                "start_ambilight"
            },
        );
        let lueur = self.lueur.clone();
        let mut page = div().flex().flex_col().gap(px(size::S5));
        if has_lights || running {
            page = page.child(div().flex().child(ui::button(
                "toggle-effect",
                toggle_label,
                true,
                theme,
                move |_, _, cx| lueur.update(cx, |l, cx| l.toggle_effect(cx)),
            )));
        }

        let heading = div()
            .text_size(px(size::T_SM))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text_3)
            .child(t(cx, "lights"));
        let rows: Vec<AnyElement> = if lights.is_empty() {
            vec![
                div()
                    .p(px(size::S4))
                    .text_color(theme.text_2)
                    .text_size(px(size::T_MD))
                    .child(t(cx, "no_lights"))
                    .into_any_element(),
            ]
        } else {
            lights
                .into_iter()
                .enumerate()
                .map(|(i, (id, name, bound, on, driven))| {
                    self.light_row(i, id, name, bound, on, driven, theme, cx)
                })
                .collect()
        };
        page.child(
            div()
                .flex()
                .flex_col()
                .gap(px(size::S2))
                .child(heading)
                .child(ui::card(theme).children(rows)),
        )
        .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn light_row(
        &self,
        index: usize,
        id: String,
        name: String,
        bound: bool,
        on: bool,
        driven: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = SharedString::from(name);
        // What the light does, in words as well as by its switch: amber is
        // never the only sign that it plays.
        let state = if !bound {
            t(cx, "no_bulb")
        } else if driven {
            t(cx, "mode_ambilight")
        } else if on {
            SharedString::default()
        } else {
            t(cx, "off")
        };
        let lueur = self.lueur.clone();
        let switch_label = SharedString::from(format!(
            "{name}: {}",
            t(cx, if on { "turn_off" } else { "turn_on" })
        ));
        div()
            .flex()
            .items_center()
            .gap(px(size::S3))
            .h(px(48.0))
            .px(px(size::S4))
            .when(index > 0, |d| d.border_t_1().border_color(theme.separator))
            .child(ui::icon(
                "bulb",
                18.0,
                false,
                if driven || (bound && on) {
                    theme.active
                } else {
                    theme.text_3
                },
            ))
            .child(
                div()
                    .flex_1()
                    .text_size(px(size::T_MD))
                    .text_color(theme.text)
                    .child(name),
            )
            .child(
                div()
                    .text_size(px(size::T_SM))
                    .text_color(if driven {
                        theme.active_text
                    } else {
                        theme.text_3
                    })
                    .child(state),
            )
            .when(bound && !driven, |d| {
                d.child(ui::switch(
                    SharedString::from(format!("power-{id}")),
                    switch_label,
                    on,
                    theme,
                    move |_, _, cx| {
                        let id = id.clone();
                        lueur.update(cx, |l, cx| l.set_light_power(id, !on, cx));
                    },
                ))
            })
            .into_any_element()
    }

    fn notice(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.lueur.read(cx).notice.clone()?;
        let lueur = self.lueur.clone();
        let close = i18n::t(cx, "dismiss");
        Some(
            div()
                .id("notice")
                .role(if notice.error {
                    Role::Alert
                } else {
                    Role::Status
                })
                .absolute()
                .bottom(px(size::S5))
                .right(px(size::S5))
                .max_w(px(420.0))
                .flex()
                .items_center()
                .gap(px(size::S3))
                .p(px(size::S3))
                .rounded(px(size::R_MD))
                .bg(theme.surface)
                .border_1()
                .border_color(if notice.error {
                    theme.danger
                } else {
                    theme.line
                })
                .shadow_lg()
                .child(ui::icon(
                    if notice.error { "alert" } else { "info" },
                    16.0,
                    false,
                    if notice.error {
                        theme.danger
                    } else {
                        theme.text_2
                    },
                ))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(size::T_MD))
                        .text_color(theme.text)
                        .child(notice.text),
                )
                .child(
                    ui::pressable("notice-close", theme)
                        .role(Role::Button)
                        .aria_label(close)
                        .p(px(size::S1))
                        .rounded(px(size::R_SM))
                        .hover(|s| s.bg(theme.surface_hover))
                        .child(ui::icon("close", 14.0, false, theme.text_2))
                        .on_click(move |_, _, cx| {
                            lueur.update(cx, |l, cx| {
                                l.notice = None;
                                cx.notify();
                            })
                        }),
                )
                .into_any_element(),
        )
    }
}

impl Render for MainWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let sidebar = self.sidebar(&theme, cx);
        let page = self.page(&theme, cx);
        let notice = self.notice(&theme, cx);
        div()
            .id("root")
            .role(Role::Application)
            .aria_label("Lueur")
            .track_focus(&self.focus)
            .key_context("Lueur")
            .on_action(cx.listener(|this, _: &GoLighting, _, cx| this.go(View::Lighting, cx)))
            .on_action(cx.listener(|this, _: &GoRoom, _, cx| this.go(View::Room, cx)))
            .on_action(cx.listener(|this, _: &GoBulbs, _, cx| this.go(View::Bulbs, cx)))
            .on_action(cx.listener(|this, _: &GoSettings, _, cx| this.go(View::Settings, cx)))
            .on_action(cx.listener(|this, _: &ToggleEffect, _, cx| {
                this.lueur.update(cx, |l, cx| l.toggle_effect(cx));
            }))
            .on_action(|_: &NextControl, window, cx| window.focus_next(cx))
            .on_action(|_: &PreviousControl, window, cx| window.focus_prev(cx))
            .size_full()
            .relative()
            .flex()
            .bg(theme.bg)
            .text_color(theme.text)
            .font_family(".SystemUIFont")
            .child(sidebar)
            .child(page)
            .children(notice)
    }
}

/// The window's title for the OS: the app's name.
pub fn title(_cx: &App) -> SharedString {
    SharedString::new_static("Lueur")
}
