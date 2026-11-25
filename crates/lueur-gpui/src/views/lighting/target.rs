//! Who the settings below go to, named over them: the whole room, or the one
//! light whose card was clicked, with a way back to the whole room. As on
//! the bulbs' own apps, the settings go to one light or to all of them,
//! never to a handful picked by hand.

use super::LightingPage;
use super::controls::{Kind, btn, lamp, light_dot};
use super::model::Room;
use crate::i18n::t;
use crate::theme::{Theme, size};
use crate::ui;
use gpui::{AnyElement, Context, FontWeight, IntoElement, Role, SharedString, div, prelude::*, px};

impl LightingPage {
    /// The settings' heading: what they go to, what it does now, and, for a
    /// single light, the way back to the whole room.
    pub(super) fn targets(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let l = self.lueur.read(cx);
        let room = Room::of(l);
        let lang = l.lang();
        let bound: Vec<(String, SharedString)> = l
            .config
            .room
            .lights
            .iter()
            .filter(|x| room.bound(&x.id))
            .map(|x| (x.id.clone(), x.name.clone().into()))
            .collect();
        let open = room
            .picked()
            .and_then(|p| bound.iter().find(|(id, _)| p.contains(id)).cloned());
        let ids = room.mood_targets();
        let summary = room.summary(&ids, lang);
        let now = t(cx, "mode_now");

        // With a single light, its name; otherwise the light opened or the
        // whole room.
        let (name, dot): (SharedString, Option<String>) = match (&open, bound.len()) {
            (Some((id, name)), _) => (name.clone(), Some(id.clone())),
            (None, 1) => (bound[0].1.clone(), Some(bound[0].0.clone())),
            (None, _) => (t(cx, "whole_room"), None),
        };

        let doing = summary.map(|(text, live)| {
            div()
                .id("targets-now")
                .role(Role::Status)
                .aria_label(SharedString::from(format!("{now}{text}")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(size::T_SM))
                .text_color(if live { theme.glow_text } else { theme.text_2 })
                .child(
                    div()
                        .text_color(theme.text_3)
                        .child(SharedString::from(now.trim_end().to_string())),
                )
                .when(live, |d| {
                    d.font_weight(FontWeight::MEDIUM).child(lamp(6.0, theme))
                })
                .child(text)
        });
        // The whole room with several lights: say once how to set one alone.
        let hint = (open.is_none() && bound.len() > 1).then(|| {
            div()
                .text_size(px(size::T_SM))
                .text_color(theme.text_3)
                .child(t(cx, "light_settings_hint"))
        });
        let back = open.is_some().then(|| {
            let label = t(cx, "whole_room");
            let lueur = self.lueur.clone();
            btn("back-to-room", Kind::Plain, true, false, theme)
                .ml(px(-size::S2))
                .aria_label(t(cx, "back_to_room_tip"))
                .gap(px(6.0))
                .child(ui::icon("back", 13.0, false, theme.text_2))
                .child(label)
                .on_click(move |_, _, cx| lueur.update(cx, |l, cx| l.pick_room(cx)))
        });

        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(size::S1))
            .px(px(size::S1))
            .mb(px(size::S5))
            .children(back)
            .child(
                div()
                    .id("targets")
                    .role(Role::Heading)
                    .aria_level(2)
                    .aria_label(name.clone())
                    .flex()
                    .items_center()
                    .gap(px(size::S2))
                    .min_w_0()
                    .children(dot.map(|id| light_dot(&room, &id, 18.0, true, theme)))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(size::T_LG))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.text)
                            .child(name),
                    ),
            )
            .children(doing)
            .children(hint)
            .into_any_element()
    }
}
