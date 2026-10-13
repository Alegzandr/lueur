//! System-tray icon and menu.
//!
//! The tray is how Lueur is used mid-game: the window is usually hidden, so
//! the menu carries the one action that matters (start or stop the effect)
//! and always shows the current state. Its labels follow the interface
//! language the frontend reports.

use crate::runner;
use crate::state::AppState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// Event the frontend listens to, so it updates at once when the effect is
/// started or stopped from the tray.
pub const EFFECT_CHANGED: &str = "effect-changed";

/// Handles to the menu items whose text changes.
pub struct TrayMenu {
    toggle: MenuItem<Wry>,
    open: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

struct Labels {
    start: &'static str,
    stop: &'static str,
    open: &'static str,
    quit: &'static str,
    on: &'static str,
    off: &'static str,
}

fn labels(lang: &str) -> Labels {
    match lang {
        "fr" => Labels {
            start: "Démarrer l'ambilight",
            stop: "Arrêter l'ambilight",
            open: "Ouvrir Lueur",
            quit: "Quitter",
            on: "Lueur : ambilight allumé",
            off: "Lueur : ambilight éteint",
        },
        _ => Labels {
            start: "Start ambilight",
            stop: "Stop ambilight",
            open: "Open Lueur",
            quit: "Quit",
            on: "Lueur: ambilight on",
            off: "Lueur: ambilight off",
        },
    }
}

/// Build the tray icon and its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let l = labels("en");
    let toggle = MenuItem::with_id(app, "toggle", l.start, true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &open, &separator, &quit])?;

    TrayIconBuilder::with_id("lueur-tray")
        .tooltip(l.off)
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => toggle_effect(app),
            "open" => focus_main(app),
            "quit" => {
                if let Some(state) = app.try_state::<AppState>()
                    && let Ok(mut guard) = state.effect.lock()
                    && let Some(mut h) = guard.take()
                {
                    h.stop();
                }
                app.exit(0);
            }
            _ => {}
        })
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

    app.manage(TrayMenu { toggle, open, quit });
    Ok(())
}

/// Bring the menu and tooltip in line with the effect state and language.
pub fn refresh(app: &AppHandle) {
    let (Some(menu), Some(state)) = (app.try_state::<TrayMenu>(), app.try_state::<AppState>())
    else {
        return;
    };
    let lang = state
        .ui_language
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default();
    let l = labels(&lang);
    let running = state.is_running();
    let _ = menu.toggle.set_text(if running { l.stop } else { l.start });
    let _ = menu.open.set_text(l.open);
    let _ = menu.quit.set_text(l.quit);
    if let Some(tray) = app.tray_by_id("lueur-tray") {
        let _ = tray.set_tooltip(Some(if running { l.on } else { l.off }));
    }
}

/// Start or stop the effect from the tray, off the event-loop thread (the
/// music-mode handshake blocks for up to a few seconds).
fn toggle_effect(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let running = {
            let Ok(mut guard) = state.effect.lock() else {
                return;
            };
            match guard.take() {
                Some(mut h) => {
                    h.stop();
                    true
                }
                None => false,
            }
        };
        if !running {
            let cfg = match state.config.lock() {
                Ok(c) => c.clone(),
                Err(_) => return,
            };
            match runner::start(&cfg) {
                Ok(h) => {
                    if let Ok(mut guard) = state.effect.lock() {
                        *guard = Some(h);
                    }
                }
                Err(e) => {
                    // No window may be visible: show it so the error is seen.
                    focus_main(&app);
                    let _ = app.emit(EFFECT_CHANGED, Some(e));
                    refresh(&app);
                    return;
                }
            }
        }
        let _ = app.emit(EFFECT_CHANGED, None::<String>);
        refresh(&app);
    });
}

/// Show and focus the main window.
pub fn focus_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
