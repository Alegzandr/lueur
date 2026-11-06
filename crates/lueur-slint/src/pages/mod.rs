//! The four pages of the sidebar, one module each.
//!
//! Each page has a global in its `.slint` file (`LightingState`, …) that
//! holds what it shows. Its module fills that global from the app's
//! [`Lueur`] (`project`) and answers its callbacks (`bind`). What only one
//! page needs (a search running, a field being typed in, a drag on the
//! plan) lives in that page's part of [`Locals`].

pub mod bulbs;
pub mod lighting;
pub mod room;
pub mod settings;

use crate::AppWindow;
use crate::state::{Lueur, View};

/// What each page keeps for itself.
#[derive(Default)]
pub struct Locals {
    pub lighting: lighting::Local,
    pub room: room::Local,
    pub bulbs: bulbs::Local,
    pub settings: settings::Local,
}

/// Wire every page's callbacks, once.
pub fn bind(ui: &AppWindow) {
    lighting::bind(ui);
    room::bind(ui);
    bulbs::bind(ui);
    settings::bind(ui);
}

/// Bring the page shown up to date, and what is drawn behind every page.
pub fn project(l: &Lueur, ui: &AppWindow) {
    lighting::project_backdrop(l, ui);
    match l.view {
        View::Lighting => lighting::project(l, ui),
        View::Room => room::project(l, ui),
        View::Bulbs => bulbs::project(l, ui),
        View::Settings => settings::project(l, ui),
    }
}
