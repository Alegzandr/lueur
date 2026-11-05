//! The system-tray icon. What its menu shows and what each item does are
//! `lueur_shell::tray_model`'s; this draws it with `tray-icon` (the crate
//! Tauri's tray is built on) and hands the picks to the app's thread.

#[cfg(not(any(windows, target_os = "macos")))]
use async_channel::Sender;

/// What the tray asks of the app. Without a tray (Linux, for now) nothing
/// asks it.
#[derive(Debug, Clone)]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub enum Pick {
    /// A menu item, by its id in the model.
    Item(String),
    /// A left click on the icon: bring the window up.
    Open,
}

/// Whether this system gets a tray icon. Linux needs a GTK loop of its own
/// for it, which Lueur does not run yet; there, closing the window quits.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use super::Pick;
    use async_channel::Sender;
    use gpui::{App, Global};
    use lueur_shell::Shell;
    use lueur_shell::tray_model::{self, Item, Shown, Tracker};
    use tray_icon::menu::{
        CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
    };
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    struct Tray {
        icon: TrayIcon,
        tracker: Tracker,
    }

    impl Global for Tray {}

    fn entry(item: &Item) -> Box<dyn IsMenuItem> {
        match item {
            Item::Action { id, label, enabled } => Box::new(MenuItem::with_id(
                id.as_str(),
                escape(label),
                *enabled,
                None,
            )),
            Item::Check {
                id,
                label,
                enabled,
                checked,
            } => Box::new(CheckMenuItem::with_id(
                id.as_str(),
                escape(label),
                *enabled,
                *checked,
                None,
            )),
            Item::Submenu {
                label,
                enabled,
                items,
            } => {
                let sub = Submenu::new(escape(label), *enabled);
                for item in items {
                    let _ = sub.append(entry(item).as_ref());
                }
                Box::new(sub)
            }
            Item::Separator => Box::new(PredefinedMenuItem::separator()),
        }
    }

    /// A label as it reads: `&` would otherwise mark a mnemonic.
    fn escape(label: &str) -> String {
        label.replace('&', "&&")
    }

    fn menu(shown: &Shown) -> Menu {
        let menu = Menu::new();
        for item in tray_model::menu(shown) {
            let _ = menu.append(entry(&item).as_ref());
        }
        menu
    }

    fn icon() -> Option<Icon> {
        let png = include_bytes!("../assets/icon-32.png");
        let image = image::load_from_memory(png).ok()?.into_rgba8();
        let (w, h) = image.dimensions();
        Icon::from_rgba(image.into_raw(), w, h).ok()
    }

    /// Put the icon in the tray. Picks arrive on `picks`.
    pub fn setup(shell: &Shell, picks: Sender<Pick>, cx: &mut App) {
        let tracker = Tracker::default();
        let shown = tracker
            .next(shell, true)
            .unwrap_or_else(|| tray_model::shown(shell));
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu(&shown)))
            .with_tooltip(tray_model::tooltip(&shown))
            .with_menu_on_left_click(false);
        if let Some(icon) = icon() {
            builder = builder.with_icon(icon);
        }
        let icon = match builder.build() {
            Ok(icon) => icon,
            Err(e) => {
                log::error!("tray: {e}");
                return;
            }
        };
        let items = picks.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = items.try_send(Pick::Item(e.id.0));
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                let _ = picks.try_send(Pick::Open);
            }
        }));
        cx.set_global(Tray { icon, tracker });
    }

    /// Bring the menu and tooltip in line with what the lights show and the
    /// language; with `always` false, only if that changed.
    pub fn update(shell: &Shell, always: bool, cx: &mut App) {
        let Some(tray) = cx.try_global::<Tray>() else {
            return;
        };
        let Some(shown) = tray.tracker.next(shell, always) else {
            return;
        };
        tray.icon.set_menu(Some(Box::new(menu(&shown))));
        let _ = tray.icon.set_tooltip(Some(tray_model::tooltip(&shown)));
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub use imp::{setup, update};

#[cfg(not(any(windows, target_os = "macos")))]
pub fn setup(_shell: &lueur_shell::Shell, _picks: Sender<Pick>, _cx: &mut gpui::App) {}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn update(_shell: &lueur_shell::Shell, _always: bool, _cx: &mut gpui::App) {}
