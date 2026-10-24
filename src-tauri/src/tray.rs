//! System-tray icon and menu.
//!
//! The tray is how Lueur is used mid-game, and how the lights are switched
//! without opening the window: the menu starts and stops the effect, turns
//! every light on or off and applies the moods and the saved scenes. Its
//! labels follow the interface language the frontend reports.

use crate::error::AppResult;
use crate::lights;
use crate::state::AppState;
use lueur_core::scene::MOODS;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// Event the frontend listens to, so it updates at once when the effect is
/// started or stopped from the tray.
pub const EFFECT_CHANGED: &str = "effect-changed";

/// Event telling the frontend the tray changed what the lights show.
pub const LIGHTS_CHANGED: &str = "lights-changed";

const TRAY_ID: &str = "lueur-tray";
const SCENE_PREFIX: &str = "scene:";

struct Labels {
    start: &'static str,
    stop: &'static str,
    all_on: &'static str,
    all_off: &'static str,
    scenes: &'static str,
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
            all_on: "Tout allumer",
            all_off: "Tout éteindre",
            scenes: "Ambiances",
            open: "Ouvrir Lueur",
            quit: "Quitter",
            on: "Lueur : ambilight allumé",
            off: "Lueur : ambilight éteint",
        },
        _ => Labels {
            start: "Start ambilight",
            stop: "Stop ambilight",
            all_on: "All on",
            all_off: "All off",
            scenes: "Scenes",
            open: "Open Lueur",
            quit: "Quit",
            on: "Lueur: ambilight on",
            off: "Lueur: ambilight off",
        },
    }
}

/// What the built-in moods are called (see `lueur_core::scene::MOODS`).
fn mood_label(lang: &str, id: &str) -> &'static str {
    match (lang, id) {
        ("fr", "daylight") => "Lumière du jour",
        ("fr", "reading") => "Lecture",
        ("fr", "relax") => "Détente",
        ("fr", "night") => "Veilleuse",
        (_, "daylight") => "Daylight",
        (_, "reading") => "Reading",
        (_, "relax") => "Relax",
        (_, "night") => "Night light",
        _ => "",
    }
}

fn language(state: &AppState) -> String {
    state
        .ui_language
        .lock()
        .map(|l| l.clone())
        .unwrap_or_default()
}

/// Build the menu for the current state, language and scenes.
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let lang = language(&state);
    let l = labels(&lang);
    let running = state.is_running();
    let (has_lights, scenes) = state
        .config
        .lock()
        .map(|c| {
            let scenes: Vec<(String, String)> = c
                .scenes
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect();
            (!c.bindings.is_empty(), scenes)
        })
        .unwrap_or_default();

    let toggle = MenuItem::with_id(
        app,
        "toggle",
        if running { l.stop } else { l.start },
        has_lights || running,
        None::<&str>,
    )?;
    let all_on = MenuItem::with_id(app, "all_on", l.all_on, has_lights, None::<&str>)?;
    let all_off = MenuItem::with_id(app, "all_off", l.all_off, has_lights, None::<&str>)?;

    let scene_menu = Submenu::with_id(app, "scenes", l.scenes, has_lights)?;
    for m in MOODS {
        scene_menu.append(&MenuItem::with_id(
            app,
            format!("{SCENE_PREFIX}{}", m.id),
            mood_label(&lang, m.id),
            true,
            None::<&str>,
        )?)?;
    }
    if !scenes.is_empty() {
        scene_menu.append(&PredefinedMenuItem::separator(app)?)?;
        for (id, name) in scenes {
            scene_menu.append(&MenuItem::with_id(
                app,
                format!("{SCENE_PREFIX}{id}"),
                name,
                true,
                None::<&str>,
            )?)?;
        }
    }

    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &toggle,
            &PredefinedMenuItem::separator(app)?,
            &all_on,
            &all_off,
            &scene_menu,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &quit,
        ],
    )
}

/// Build the tray icon and its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(labels("en").off)
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => in_background(app, |state| {
                crate::auto::user_took_over(state);
                if state.is_running() {
                    lights::stop_effect(state, true);
                    Ok(())
                } else {
                    lights::start_effect(state).map(drop)
                }
            }),
            "all_on" => in_background(app, |state| lights::set_all_power(state, true).map(drop)),
            "all_off" => in_background(app, |state| lights::set_all_power(state, false).map(drop)),
            "open" => focus_main(app),
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    lights::stop_effect(&state, false);
                }
                app.exit(0);
            }
            id => {
                if let Some(scene) = id.strip_prefix(SCENE_PREFIX) {
                    let scene = scene.to_string();
                    in_background(app, move |state| {
                        lights::apply_scene(state, &scene).map(drop)
                    });
                }
            }
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
    Ok(())
}

/// Bring the menu and tooltip in line with the effect state, the language
/// and the saved scenes.
pub fn refresh(app: &AppHandle) {
    let (Some(tray), Some(state)) = (app.tray_by_id(TRAY_ID), app.try_state::<AppState>()) else {
        return;
    };
    let l = labels(&language(&state));
    if let Ok(menu) = build_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(if state.is_running() { l.on } else { l.off }));
}

/// Run a tray action off the event-loop thread (bulbs can take seconds to
/// answer, the music-mode handshake too), then tell the window what changed.
/// A failure brings the window up, since none may be visible to show it.
fn in_background(app: &AppHandle, job: impl FnOnce(&AppState) -> AppResult<()> + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || {
        let result = job(&app.state::<AppState>());
        if result.is_err() {
            focus_main(&app);
        }
        let _ = app.emit(EFFECT_CHANGED, result.err());
        let _ = app.emit(LIGHTS_CHANGED, ());
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
