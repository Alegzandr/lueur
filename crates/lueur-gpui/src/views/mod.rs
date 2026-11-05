//! The four pages of the sidebar, one module each, after `src/lib/views/`.
//!
//! Each page is an entity of its own: it keeps what only it needs (a search
//! running, a field being typed in, a drag on the plan) and reads the rest
//! from the app's one [`crate::state::Lueur`], whose actions it calls. The
//! main window holds the four and shows the current one in its scrolling
//! column.

pub mod bulbs;
pub mod lighting;
pub mod room;
pub mod settings;

use crate::state::{Lueur, model};
use gpui::{App, AppContext, Context, Entity, Window};

pub use bulbs::BulbsPage;
pub use lighting::LightingPage;
pub use room::RoomPage;
pub use settings::SettingsPage;

/// The pages, made once with the window.
pub struct Pages {
    pub lighting: Entity<LightingPage>,
    pub room: Entity<RoomPage>,
    pub bulbs: Entity<BulbsPage>,
    pub settings: Entity<SettingsPage>,
}

impl Pages {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Pages {
            lighting: cx.new(|cx| LightingPage::new(window, cx)),
            room: cx.new(|cx| RoomPage::new(window, cx)),
            bulbs: cx.new(|cx| BulbsPage::new(window, cx)),
            settings: cx.new(|cx| SettingsPage::new(window, cx)),
        }
    }
}

/// The app's [`Lueur`], watched: the page draws again whenever it changes.
pub fn watch<T: 'static>(cx: &mut Context<T>) -> Entity<Lueur> {
    let lueur = model(cx);
    cx.observe(&lueur, |_, _, cx| cx.notify()).detach();
    lueur
}
