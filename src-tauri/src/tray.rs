//! The system-tray icon, drawn by Tauri. What its menu shows and what each
//! item does are `lueur_shell::tray_model`'s; this only builds Tauri's menu
//! from it and hands the picks back.

use lueur_shell::Shell;
use lueur_shell::tray_model::{self, Item, Outcome, Shown, Tracker};
use std::sync::{Arc, LazyLock};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "lueur-tray";

/// What the tray showed when it was last brought up to date.
static TRACKER: LazyLock<Tracker> = LazyLock::new(Tracker::default);

/// One Tauri menu entry for an item of the model.
fn entry(app: &AppHandle, item: &Item) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    Ok(match item {
        Item::Action { id, label, enabled } => Box::new(MenuItem::with_id(
            app,
            id.as_str(),
            label,
            *enabled,
            None::<&str>,
        )?),
        Item::Check {
            id,
            label,
            enabled,
            checked,
        } => Box::new(CheckMenuItem::with_id(
            app,
            id.as_str(),
            label,
            *enabled,
            *checked,
            None::<&str>,
        )?),
        Item::Submenu {
            label,
            enabled,
            items,
        } => {
            let sub = Submenu::new(app, label, *enabled)?;
            for item in items {
                sub.append(entry(app, item)?.as_ref())?;
            }
            Box::new(sub)
        }
        Item::Separator => Box::new(PredefinedMenuItem::separator(app)?),
    })
}

fn build_menu(app: &AppHandle, shown: &Shown) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for item in tray_model::menu(shown) {
        menu.append(entry(app, &item)?.as_ref())?;
    }
    Ok(menu)
}

/// Build the tray icon and its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let shell = Arc::clone(&app.state::<Arc<Shell>>());
    let first = TRACKER
        .next(&shell, true)
        .unwrap_or_else(|| tray_model::shown(&shell));
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tray_model::tooltip(&first))
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .menu(&build_menu(app, &first)?)
        .show_menu_on_left_click(false)
        .on_menu_event(
            move |app, event| match tray_model::act(&shell, event.id.as_ref()) {
                Outcome::Done => {}
                Outcome::Restart => app.request_restart(),
                Outcome::Quit => app.exit(0),
            },
        )
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                focus_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Bring the menu and tooltip in line with what the lights show and the
/// language; with `always` false, only if that changed.
pub fn update(app: &AppHandle, always: bool) {
    let (Some(tray), Some(shell)) = (app.tray_by_id(TRAY_ID), app.try_state::<Arc<Shell>>()) else {
        return;
    };
    let Some(shown) = TRACKER.next(&shell, always) else {
        return;
    };
    if let Ok(menu) = build_menu(app, &shown) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tray_model::tooltip(&shown)));
}

/// Show and focus the main window.
pub fn focus_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
