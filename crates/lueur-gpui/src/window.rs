//! The main window: the sidebar, the title bar and the page it shows, and
//! the toasts over them, after `src/App.svelte`.

use crate::app::{
    GoBulbs, GoLighting, GoRoom, GoSettings, NextControl, PreviousControl, ToggleEffect,
};
use crate::i18n::{self, t};
use crate::state::{Lueur, Toast, View, model};
use crate::theme::{Theme, size, theme};
use crate::titlebar;
use crate::ui;
use crate::views::Pages;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, IntoElement, Render, Role,
    SharedString, Window, WindowControlArea, div, img, prelude::*, px,
};

pub struct MainWindow {
    lueur: Entity<Lueur>,
    pages: Pages,
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
        MainWindow {
            lueur,
            pages: Pages::new(window, cx),
            focus,
        }
    }

    fn go(&mut self, view: View, cx: &mut Context<Self>) {
        self.lueur.update(cx, |l, cx| l.show(view, cx));
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let l = self.lueur.read(cx);
        let current = l.view;
        // Lights drawn in the room with no bulb yet, counted on Bulbs.
        let unbound = l
            .config
            .room
            .lights
            .iter()
            .filter(|light| l.config.binding_for(&light.id).is_none())
            .count();
        let mod_key = if cfg!(target_os = "macos") {
            "Meta"
        } else {
            "Control"
        };
        let unbound_label = t(cx, "unbound_badge");
        let nav = View::ALL.iter().enumerate().map(|(i, &view)| {
            let selected = view == current;
            let label = t(cx, view.label_key());
            let badge = (view == View::Bulbs && unbound > 0).then_some(unbound);
            let fg = if selected { theme.text } else { theme.text_2 };
            // The current page's icon is amber, like the light it is about.
            let icon_color = if selected { theme.active } else { fg };
            ui::pressable(
                SharedString::from(format!("nav-{}", view.label_key())),
                theme,
            )
            .role(Role::Tab)
            .aria_label(match badge {
                Some(n) => SharedString::from(format!("{label}, {unbound_label}: {n}")),
                None => label.clone(),
            })
            .aria_selected(selected)
            .aria_keyshortcuts(SharedString::from(format!("{mod_key}+{}", i + 1)))
            .h(px(36.0))
            .px(px(size::S3))
            .flex()
            .items_center()
            .gap(px(size::S3))
            .rounded(px(7.0))
            .when(selected, |d| {
                d.bg(theme.surface)
                    .shadow_sm()
                    .border_1()
                    .border_color(theme.separator)
            })
            .when(!selected, |d| d.hover(|s| s.bg(theme.surface_hover)))
            .text_color(fg)
            .text_size(px(size::T_MD))
            .when(selected, |d| d.font_weight(FontWeight::MEDIUM))
            .child(ui::icon(view.icon(), 18.0, selected, icon_color))
            .child(div().flex_1().child(label))
            .when_some(badge, |d, n| {
                d.child(
                    div()
                        .px(px(6.0))
                        .rounded_full()
                        .bg(theme.surface_hover)
                        .text_size(px(size::T_XS))
                        .text_color(theme.text_2)
                        .child(n.to_string()),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.go(view, cx)))
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
            .px(px(size::S3))
            .pb(px(size::S3))
            .pt(px(if titlebar::OWN { 6.0 } else { size::S3 }))
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.separator)
            .child(
                div()
                    .id("brand")
                    // On Windows the brand is part of the bar the window is
                    // dragged by.
                    .when(titlebar::OWN, |d| {
                        d.window_control_area(WindowControlArea::Drag)
                    })
                    .flex()
                    .items_center()
                    .gap(px(size::S2))
                    .px(px(size::S2))
                    .h(px(40.0))
                    .mb(px(size::S3))
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

    /// The button that restarts into an installed update, when there is one.
    fn update_button(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let version = self.lueur.read(cx).update_ready.clone()?;
        let label = t(cx, "update_restart");
        let tip = SharedString::from(i18n::fill(
            &t(cx, "update_ready"),
            &[("version", version.as_str())],
        ));
        Some(
            ui::pressable("update", theme)
                .role(Role::Button)
                .aria_label(SharedString::from(format!("{label}. {tip}")))
                .tooltip(ui::tip(tip))
                .h(px(24.0))
                .px(px(size::S3))
                .flex()
                .items_center()
                .gap(px(size::S2))
                .rounded_full()
                .bg(theme.active_soft)
                .text_size(px(size::T_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.active_text)
                .child(div().size(px(7.0)).rounded_full().bg(theme.glow))
                .child(label)
                .on_click(|_, _, cx| crate::app::restart_into_update(cx))
                .into_any_element(),
        )
    }

    fn page(&self, cx: &App) -> AnyElement {
        match self.lueur.read(cx).view {
            View::Lighting => self.pages.lighting.clone().into_any_element(),
            View::Room => self.pages.room.clone().into_any_element(),
            View::Bulbs => self.pages.bulbs.clone().into_any_element(),
            View::Settings => self.pages.settings.clone().into_any_element(),
        }
    }

    fn toasts(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let toasts = self.lueur.read(cx).toasts.clone();
        let dismiss_label = t(cx, "dismiss");
        let items = toasts
            .into_iter()
            .map(|toast| toast_view(toast, &dismiss_label, theme, &self.lueur));
        div()
            .id("toasts")
            .absolute()
            .bottom(px(size::S5))
            .right(px(size::S5))
            .w(px(380.0))
            .flex()
            .flex_col()
            .gap(px(size::S2))
            .children(items)
            .into_any_element()
    }
}

fn toast_view(
    toast: Toast,
    dismiss_label: &SharedString,
    theme: &Theme,
    lueur: &Entity<Lueur>,
) -> AnyElement {
    let id = toast.id;
    let text = if toast.count > 1 {
        SharedString::from(format!("{} (×{})", toast.text, toast.count))
    } else {
        toast.text.clone()
    };
    let hold = lueur.clone();
    let close = lueur.clone();
    div()
        .id(("toast", id))
        .role(if toast.error {
            Role::Alert
        } else {
            Role::Status
        })
        .aria_label(text.clone())
        .flex()
        .items_start()
        .gap(px(size::S3))
        .p(px(size::S3))
        .rounded(px(size::R_MD))
        .bg(theme.surface)
        .border_1()
        .border_color(if toast.error {
            theme.danger
        } else {
            theme.line
        })
        .shadow_lg()
        // Pointed at, an information toast waits.
        .on_hover(move |hovered, _, cx| {
            hold.update(cx, |l, cx| l.hold_toast(id, *hovered, cx));
        })
        .child(ui::icon(
            if toast.error { "alert" } else { "info" },
            16.0,
            false,
            if toast.error {
                theme.danger
            } else {
                theme.text_2
            },
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(size::S1))
                .child(
                    div()
                        .text_size(px(size::T_MD))
                        .text_color(theme.text)
                        .child(text),
                )
                .when_some(toast.detail.clone(), |d, detail| {
                    d.child(
                        div()
                            .text_size(px(size::T_XS))
                            .text_color(theme.text_3)
                            .child(detail),
                    )
                })
                .when_some(toast.action.clone(), |d, (label, run)| {
                    let done = lueur.clone();
                    d.child(
                        div().flex().child(
                            ui::pressable(("toast-action", id), theme)
                                .role(Role::Button)
                                .aria_label(label.clone())
                                .text_size(px(size::T_SM))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(label)
                                .on_click(move |_, window, cx| {
                                    run(window, cx);
                                    done.update(cx, |l, cx| l.dismiss(id, cx));
                                }),
                        ),
                    )
                }),
        )
        .child(
            ui::pressable(("toast-close", id), theme)
                .role(Role::Button)
                .aria_label(dismiss_label.clone())
                .p(px(size::S1))
                .rounded(px(size::R_SM))
                .hover(|s| s.bg(theme.surface_hover))
                .child(ui::icon("close", 14.0, false, theme.text_2))
                .on_click(move |_, _, cx| close.update(cx, |l, cx| l.dismiss(id, cx))),
        )
        .into_any_element()
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let sidebar = self.sidebar(&theme, cx);
        let update = self.update_button(&theme, cx);
        let toasts = self.toasts(&theme, cx);
        let page = self.page(cx);
        // Under the page, the glow of the lights behind Lighting, the
        // room's photograph behind the other pages.
        let backdrop = self.pages.lighting.read(cx).backdrop(cx);
        let top = if titlebar::OWN {
            titlebar::bar(div().children(update), &theme, window).into_any_element()
        } else {
            div()
                .flex()
                .justify_end()
                .px(px(size::S4))
                .pt(px(size::S3))
                .children(update)
                .into_any_element()
        };
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
            .font_family(crate::app::FONT)
            .child(sidebar)
            .child(
                // `min_w_0`: the column takes the window's width, never the width
                // of its longest line, which would push the caption buttons out.
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(backdrop)
                    .child(top)
                    .child(
                        div()
                            .id("page")
                            .flex_1()
                            .overflow_y_scroll()
                            .px(px(size::S6 + size::S2))
                            .pt(px(size::S3))
                            .pb(px(size::S6))
                            .child(page),
                    ),
            )
            .child(toasts)
    }
}

/// The window's title for the OS: the app's name.
pub fn title(_cx: &App) -> SharedString {
    SharedString::new_static("Lueur")
}
