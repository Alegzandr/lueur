//! System-tray icon and menu.
//!
//! The tray is how Lueur is used mid-game, and how the lights are switched
//! without opening the window: the menu starts and stops the ambilight and
//! the effects, applies the moods and turns every light on or off. Its
//! labels follow the interface language the frontend reports.

use crate::error::AppResult;
use crate::lights;
use crate::runner::Mode;
use crate::state::AppState;
use lueur_core::scene::MOODS;
use lueur_core::show::{SHOWS, ShowKind};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// Event the frontend listens to, so it updates at once when the effect is
/// started or stopped from the tray.
pub const EFFECT_CHANGED: &str = "effect-changed";

/// Event telling the frontend the tray changed what the lights show.
pub const LIGHTS_CHANGED: &str = "lights-changed";

const TRAY_ID: &str = "lueur-tray";
const SCENE_PREFIX: &str = "scene:";
const SHOW_PREFIX: &str = "show:";

struct Labels {
    start: &'static str,
    stop: &'static str,
    stop_show: &'static str,
    shows: &'static str,
    scenes: &'static str,
    all_on: &'static str,
    all_off: &'static str,
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
            stop_show: "Arrêter l'effet",
            shows: "Effets",
            scenes: "Ambiances",
            all_on: "Tout allumer",
            all_off: "Tout éteindre",
            open: "Ouvrir Lueur",
            quit: "Quitter",
            on: "Lueur : allumé",
            off: "Lueur : éteint",
        },
        _ => Labels {
            start: "Start ambilight",
            stop: "Stop ambilight",
            stop_show: "Stop effect",
            shows: "Effects",
            scenes: "Moods",
            all_on: "All on",
            all_off: "All off",
            open: "Open Lueur",
            quit: "Quit",
            on: "Lueur: on",
            off: "Lueur: off",
        },
    }
}

/// What the built-in moods and the shows are called (see
/// `lueur_core::scene::MOODS` and `lueur_core::show::SHOWS`).
fn name(lang: &str, id: &str) -> &'static str {
    match (lang, id) {
        ("fr", "daylight") => "Lumière du jour",
        ("fr", "reading") => "Lecture",
        ("fr", "relax") => "Détente",
        ("fr", "night") => "Veilleuse",
        ("fr", "music") => "Musique",
        ("fr", "rainbow") => "Arc-en-ciel",
        ("fr", "sunset") => "Coucher de soleil",
        ("fr", "ocean") => "Océan",
        ("fr", "candle") => "Bougie",
        (_, "daylight") => "Daylight",
        (_, "reading") => "Reading",
        (_, "relax") => "Relax",
        (_, "night") => "Night light",
        (_, "music") => "Music",
        (_, "rainbow") => "Rainbow",
        (_, "sunset") => "Sunset",
        (_, "ocean") => "Ocean",
        (_, "candle") => "Candle",
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

/// What runs now, if anything.
fn running_mode(state: &AppState) -> Option<Mode> {
    let guard = state.effect.lock().ok()?;
    guard.as_ref().filter(|h| h.is_running()).map(|h| h.mode)
}

/// Build the menu for the current state and language.
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let lang = language(&state);
    let l = labels(&lang);
    let running = running_mode(&state);
    let has_lights = state
        .config
        .lock()
        .map(|c| !c.bindings.is_empty())
        .unwrap_or(false);

    let toggle_label = match running {
        None => l.start,
        Some(Mode::Ambilight) => l.stop,
        Some(Mode::Show(_)) => l.stop_show,
    };
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        toggle_label,
        has_lights || running.is_some(),
        None::<&str>,
    )?;

    let shows = Submenu::with_id(app, "shows", l.shows, has_lights)?;
    for kind in SHOWS {
        shows.append(&CheckMenuItem::with_id(
            app,
            format!("{SHOW_PREFIX}{}", kind.id()),
            name(&lang, kind.id()),
            true,
            running == Some(Mode::Show(kind)),
            None::<&str>,
        )?)?;
    }
    let scenes = Submenu::with_id(app, "scenes", l.scenes, has_lights)?;
    for m in MOODS {
        scenes.append(&MenuItem::with_id(
            app,
            format!("{SCENE_PREFIX}{}", m.id),
            name(&lang, m.id),
            true,
            None::<&str>,
        )?)?;
    }

    let all_on = MenuItem::with_id(app, "all_on", l.all_on, has_lights, None::<&str>)?;
    let all_off = MenuItem::with_id(app, "all_off", l.all_off, has_lights, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &toggle,
            &shows,
            &scenes,
            &PredefinedMenuItem::separator(app)?,
            &all_on,
            &all_off,
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
                } else if let Some(kind) = id.strip_prefix(SHOW_PREFIX).and_then(ShowKind::from_id)
                {
                    in_background(app, move |state| {
                        crate::auto::user_took_over(state);
                        // Picking the show that plays stops it, like its tick.
                        if running_mode(state) == Some(Mode::Show(kind)) {
                            lights::stop_effect(state, true);
                            Ok(())
                        } else {
                            lights::start(state, Mode::Show(kind)).map(drop)
                        }
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

/// Bring the menu and tooltip in line with the effect state and the
/// language.
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
