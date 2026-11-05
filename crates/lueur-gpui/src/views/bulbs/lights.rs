//! The room's lights, as the same cards as on the Lighting page, one per
//! row: the light it gives, its name, which can be changed in place, and
//! its bulb.

use super::BulbsPage;
use super::actions::{self, MAX_LIGHT_NAME};
use super::brands::bulb_label;
use super::controls::{Look, btn, group_head, icon_btn, light_dot};
use crate::i18n::{fill, t};
use crate::state::View;
use crate::theme::{size, theme};
use crate::ui::{InputEvent, TextInput};
use gpui::{
    AnyElement, App, Context, ElementId, Entity, Focusable, FontWeight, Role, SharedString,
    Subscription, Window, div, prelude::*, px,
};

/// A light whose name is being typed.
pub struct Renaming {
    light_id: String,
    /// Its name as the field was entered, to give back if it is left empty.
    before: String,
    input: Entity<TextInput>,
    _subs: Vec<Subscription>,
}

impl BulbsPage {
    /// Turn a light's name into a field, the keyboard in it.
    fn start_rename(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_rename(cx);
        let label = t(cx, "light_name");
        let input = cx.new(|cx| TextInput::new(cx, name.clone(), label, "", MAX_LIGHT_NAME));
        let focus = input.read(cx).focus_handle(cx);
        let light = id.clone();
        let typed = cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
            match event {
                // The name changes everywhere as it is typed.
                InputEvent::Changed => {
                    let text = input.read(cx).text().to_string();
                    let light = light.clone();
                    this.lueur
                        .update(cx, |l, cx| actions::rename_light(l, &light, &text, cx));
                }
                // Enter leaves the field, the keyboard back on the name.
                InputEvent::Submit => {
                    let back = this.renaming.as_ref().map(|r| r.light_id.clone());
                    this.finish_rename(cx);
                    this.focus_next = back.map(|id| format!("name-{id}"));
                    cx.notify();
                }
            }
        });
        let left = cx.on_blur(&focus, window, |this, _, cx| this.finish_rename(cx));
        focus.focus(window, cx);
        self.renaming = Some(Renaming {
            light_id: id,
            before: name,
            input,
            _subs: vec![typed, left],
        });
        cx.notify();
    }

    /// Leave the field: a light left without a name gets one back.
    fn finish_rename(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.renaming.take() else {
            return;
        };
        self.lueur.update(cx, |l, cx| {
            actions::settle_name(l, &r.light_id, &r.before, cx)
        });
        cx.notify();
    }

    pub(super) fn lights_column(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *theme(cx);
        let lights: Vec<(String, String)> = self
            .lueur
            .read(cx)
            .config
            .room
            .lights
            .iter()
            .map(|l| (l.id.clone(), l.name.clone()))
            .collect();
        let lueur = self.lueur.clone();
        let open_plan = btn(
            "open-plan",
            t(cx, "step2_btn"),
            Look::Plain,
            true,
            false,
            None,
            &theme,
            move |_, _, cx| lueur.update(cx, |l, cx| l.show(View::Room, cx)),
        )
        .into_any_element();
        let cards: Vec<AnyElement> = lights
            .into_iter()
            .map(|(id, name)| self.light_card(id, name, cx))
            .collect();
        div()
            .child(group_head(
                "room-title",
                t(cx, "in_room"),
                None,
                Some(open_plan),
                &theme,
            ))
            .child(div().flex().flex_col().gap(px(size::S2)).children(cards))
            .into_any_element()
    }

    /// The labels the bound bulbs show, to find those two share.
    fn bulb_labels(&self, cx: &App) -> Vec<String> {
        let c = &self.lueur.read(cx).config;
        c.bindings
            .iter()
            .map(|b| {
                let light = c.room.lights.iter().find(|l| l.id == b.light_id);
                bulb_label(b, light.map(|l| l.name.as_str()))
            })
            .collect()
    }

    fn light_card(&mut self, id: String, name: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = *theme(cx);
        let key = |what: &str| ElementId::Name(SharedString::from(format!("{what}-{id}")));
        let labelled = |what: SharedString, cx: &App| -> SharedString {
            fill(&t(cx, "labelled"), &[("name", &name), ("what", &what)]).into()
        };
        let labels = self.bulb_labels(cx);
        let l = self.lueur.read(cx);
        let binding = l.config.binding_for(&id).cloned();
        let dot = light_dot(l, &id, 28.0, &theme);
        // Addresses only show to tell apart two bulbs described alike.
        let meta = match &binding {
            Some(b) => {
                let label = bulb_label(b, Some(&name));
                let shared = labels.iter().filter(|x| **x == label).count() > 1;
                div().text_color(theme.text_3).child(if shared {
                    format!("{label}\u{a0}· {}", b.ip)
                } else {
                    label
                })
            }
            None => div().text_color(theme.glow_text).child(t(cx, "pick_above")),
        };
        let editing = self
            .renaming
            .as_ref()
            .filter(|r| r.light_id == id)
            .map(|r| r.input.clone());
        let name_focus = self.handle(&format!("name-{id}"), true, cx);
        let name_el: AnyElement = match editing {
            Some(input) => div().ml(px(-9.0)).child(input).into_any_element(),
            // Plain text until it is pointed at or chosen, then a field.
            None => {
                let (light, current) = (id.clone(), name.clone());
                crate::ui::pressable(key("name"), &theme)
                    .track_focus(&name_focus)
                    .role(Role::TextInput)
                    .aria_label(t(cx, "light_name"))
                    .aria_value(SharedString::from(name.clone()))
                    .cursor_text()
                    .ml(px(-9.0))
                    // The field's own height, so the card keeps its size
                    // when the name turns into one.
                    .h(px(28.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .hover(|s| s.bg(theme.surface).border_color(theme.line_strong))
                    .text_size(px(size::T_MD))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(div().truncate().child(name.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_rename(light.clone(), current.clone(), window, cx)
                    }))
                    .into_any_element()
            }
        };
        let lueur = self.lueur.clone();
        let mut buttons: Vec<AnyElement> = Vec::new();
        {
            let (lueur, light) = (lueur.clone(), id.clone());
            buttons.push(
                icon_btn(
                    key("plan"),
                    "plan",
                    labelled(t(cx, "show_on_plan"), cx),
                    t(cx, "show_on_plan"),
                    Look::Plain,
                    &theme,
                    move |_, _, cx| lueur.update(cx, |l, cx| actions::show_on_plan(l, &light, cx)),
                )
                .into_any_element(),
            );
        }
        match &binding {
            Some(b) => {
                let (identify, endpoint) = (lueur.clone(), b.endpoint());
                buttons.push(
                    icon_btn(
                        key("identify"),
                        "flash",
                        labelled(t(cx, "identify"), cx),
                        t(cx, "identify_tip"),
                        Look::Plain,
                        &theme,
                        move |_, _, cx| {
                            let endpoint = endpoint.clone();
                            identify.update(cx, |l, cx| actions::identify(l, endpoint, cx));
                        },
                    )
                    .into_any_element(),
                );
                let light = id.clone();
                buttons.push(
                    icon_btn(
                        key("unassign"),
                        "unlink",
                        labelled(t(cx, "unassign"), cx),
                        t(cx, "unassign_tip"),
                        Look::Plain,
                        &theme,
                        move |_, _, cx| lueur.update(cx, |l, cx| actions::unassign(l, &light, cx)),
                    )
                    .into_any_element(),
                );
            }
            // A light left without its bulb, say one paired again from
            // another account and added anew, can go from here too.
            None => {
                let light = id.clone();
                buttons.push(
                    icon_btn(
                        key("remove"),
                        "trash",
                        labelled(t(cx, "remove_light"), cx),
                        t(cx, "remove_light"),
                        Look::Danger,
                        &theme,
                        move |_, _, cx| {
                            lueur.update(cx, |l, cx| actions::remove_light(l, &light, cx))
                        },
                    )
                    .into_any_element(),
                );
            }
        }
        div()
            .flex()
            .items_center()
            .gap(px(size::S3))
            .min_w_0()
            .min_h(px(60.0))
            .py(px(size::S2))
            .pr(px(size::S2))
            .pl(px(size::S3))
            .rounded(px(size::R_LG))
            .border_1()
            .map(|d| {
                if binding.is_some() {
                    d.bg(theme.material).border_color(theme.separator)
                } else {
                    // No bulb yet: an outline waiting for one, not a card.
                    d.border_color(theme.line_strong)
                }
            })
            .child(dot)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(name_el)
                    .child(meta.text_size(px(size::T_SM)).truncate()),
            )
            .child(div().flex().flex_none().gap(px(2.0)).children(buttons))
            .into_any_element()
    }
}
