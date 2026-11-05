//! The Lighting page, after `src/lib/views/Lighting.svelte` and `lighting/`.

use crate::i18n::t;
use crate::state::Lueur;
use crate::theme::{Theme, size, theme};
use crate::ui;
use gpui::{
    AnyElement, Context, Entity, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};

pub struct LightingPage {
    lueur: Entity<Lueur>,
}

impl LightingPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        LightingPage {
            lueur: super::watch(cx),
        }
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
}

impl Render for LightingPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
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
                    self.light_row(i, id, name, bound, on, driven, &theme, cx)
                })
                .collect()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(size::S5))
            .child(ui::page_title("page-title", t(cx, "nav_lighting"), &theme))
            .when(has_lights || running, |d| {
                d.child(div().flex().child(ui::button(
                    "toggle-effect",
                    toggle_label,
                    true,
                    &theme,
                    move |_, _, cx| lueur.update(cx, |l, cx| l.toggle_effect(cx)),
                )))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(size::S2))
                    .child(ui::section(t(cx, "lights"), &theme))
                    .child(ui::card(&theme).children(rows)),
            )
    }
}
