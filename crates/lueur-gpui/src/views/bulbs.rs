//! The Bulbs page, after `src/lib/views/Bulbs.svelte`.

use crate::i18n::t;
use crate::state::Lueur;
use crate::theme::theme;
use crate::ui;
use gpui::{Context, Entity, IntoElement, Render, Window, div, prelude::*};

pub struct BulbsPage {
    lueur: Entity<Lueur>,
}

impl BulbsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        BulbsPage {
            lueur: super::watch(cx),
        }
    }
}

impl Render for BulbsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = &self.lueur;
        let theme = *theme(cx);
        div().child(ui::page_title("page-title", t(cx, "nav_bulbs"), &theme))
    }
}
