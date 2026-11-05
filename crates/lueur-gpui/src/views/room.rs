//! The Room page, after `src/lib/views/Room.svelte` and `room/`.

use crate::i18n::t;
use crate::state::Lueur;
use crate::theme::theme;
use crate::ui;
use gpui::{Context, Entity, IntoElement, Render, Window, div, prelude::*};

pub struct RoomPage {
    lueur: Entity<Lueur>,
}

impl RoomPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        RoomPage {
            lueur: super::watch(cx),
        }
    }
}

impl Render for RoomPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.lueur;
        let theme = *theme(cx);
        div().child(ui::page_title("page-title", t(cx, "nav_room"), &theme))
    }
}
