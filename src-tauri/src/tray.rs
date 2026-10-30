//! System-tray icon and menu.
//!
//! The tray is how Lueur is used mid-game, and how the lights are switched
//! without opening the window: the menu says what the lights show, starts
//! and stops following the screen and the effects, applies the moods, sets the
//! sleep timer, turns every light on or off and, once an update is
//! installed, restarts into it. What plays and what the
//! room shows are ticked. Its labels follow the interface language the
//! frontend reports, in the interface's words.

use crate::error::{AppError, AppResult};
use crate::lights;
use crate::runner::Mode;
use crate::state::AppState;
use lueur_core::config::AppConfig;
use lueur_core::scene::{LightState, MOODS, NATURAL};
use lueur_core::show::{SHOWS, ShowKind};
use std::sync::Mutex;
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
const SLEEP_PREFIX: &str = "sleep:";

/// The sleep timers the tray offers, in minutes.
const SLEEP_PRESETS: [u32; 4] = [15, 30, 60, 90];

struct Labels {
    start: &'static str,
    stop: &'static str,
    stop_show: &'static str,
    stop_all: &'static str,
    shows: &'static str,
    scenes: &'static str,
    sleep: &'static str,
    sleep_cancel: &'static str,
    all_on: &'static str,
    all_off: &'static str,
    open: &'static str,
    quit: &'static str,
    /// The update installed while the app runs, a restart away.
    update: &'static str,
    /// Leads the status line while something plays.
    playing: &'static str,
    by_hand: &'static str,
    dark: &'static str,
}

fn labels(lang: &str) -> Labels {
    match lang {
        "fr" => Labels {
            start: "Suivre l'écran",
            stop: "Ne plus suivre l'écran",
            stop_show: "Arrêter l'effet",
            stop_all: "Tout arrêter",
            shows: "Effets",
            scenes: "Ambiances",
            sleep: "Minuteur d'extinction",
            sleep_cancel: "Annuler le minuteur",
            all_on: "Tout allumer",
            all_off: "Tout éteindre",
            open: "Ouvrir Lueur",
            quit: "Quitter Lueur",
            update: "Redémarrer pour mettre à jour",
            playing: "En cours\u{a0}: ",
            by_hand: "Lumières réglées à la main",
            dark: "Toutes les lumières sont éteintes",
        },
        _ => Labels {
            start: "Follow the screen",
            stop: "Stop following the screen",
            stop_show: "Stop the effect",
            stop_all: "Stop everything",
            shows: "Effects",
            scenes: "Moods",
            sleep: "Sleep timer",
            sleep_cancel: "Cancel the timer",
            all_on: "All on",
            all_off: "All off",
            open: "Open Lueur",
            quit: "Quit Lueur",
            update: "Restart to update",
            playing: "Playing: ",
            by_hand: "Lights set by hand",
            dark: "All lights are off",
        },
    }
}

/// What the screen mode, the built-in moods and the shows are called (see
/// `lueur_core::scene::MOODS` and `lueur_core::show::SHOWS`), as in the
/// interface.
fn name(lang: &str, id: &str) -> &'static str {
    match (lang, id) {
        ("fr", "daylight") => "Lumière du jour",
        ("fr", "reading") => "Lecture",
        ("fr", "relax") => "Détente",
        ("fr", "night") => "Veilleuse",
        ("fr", "natural") => "Lumière naturelle",
        ("fr", "music") => "Musique",
        ("fr", "rainbow") => "Arc-en-ciel",
        ("fr", "sunset") => "Coucher de soleil",
        ("fr", "ocean") => "Océan",
        ("fr", "candle") => "Bougie",
        ("fr", "ambilight") => "Écran",
        (_, "ambilight") => "Screen",
        (_, "daylight") => "Daylight",
        (_, "reading") => "Reading",
        (_, "relax") => "Relax",
        (_, "night") => "Night light",
        (_, "natural") => "Natural light",
        (_, "music") => "Music",
        (_, "rainbow") => "Rainbow",
        (_, "sunset") => "Sunset",
        (_, "ocean") => "Ocean",
        (_, "candle") => "Candle",
        _ => "",
    }
}

fn language(state: &AppState) -> &'static str {
    state.ui_language.lock().map_or("en", |l| *l)
}

/// The modes playing now, one per effect.
fn running_modes(state: &AppState) -> Vec<Mode> {
    state.effects.modes()
}

/// What the lights show, as the tray tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Now {
    /// The effects playing: the screen mode first, then the shows in menu
    /// order.
    modes: Vec<Mode>,
    /// The mood every light no effect drives shows, when they all show the
    /// same one.
    mood: Option<&'static str>,
    /// Whether a light no effect drives is on.
    lit: bool,
}

impl Now {
    fn of(cfg: &AppConfig, mut modes: Vec<Mode>, driven: &[String]) -> Self {
        modes.sort_by_key(|m| match m {
            Mode::Ambilight => 0,
            Mode::Show(kind) => 1 + SHOWS.iter().position(|k| k == kind).unwrap_or(SHOWS.len()),
        });
        modes.dedup();
        // The lights no effect drives, as last set (a light never set is on).
        let free: Vec<(&String, LightState)> = cfg
            .bindings
            .iter()
            .map(|b| &b.light_id)
            .filter(|id| !driven.contains(id))
            .map(|id| (id, cfg.states.get(id).copied().unwrap_or_default()))
            .collect();
        let natural = |id: &String| cfg.natural.contains(id);
        let all_on = !free.is_empty() && free.iter().all(|(_, s)| s.on);
        let mood = if !all_on {
            None
        } else if free.iter().all(|(id, _)| natural(id)) {
            Some(NATURAL)
        } else {
            MOODS
                .iter()
                .find(|m| free.iter().all(|(id, s)| !natural(id) && *s == m.state()))
                .map(|m| m.id)
        };
        Now {
            modes,
            mood,
            lit: free.iter().any(|(_, s)| s.on),
        }
    }

    /// The menu's first line: what plays and the mood the rest of the room
    /// shows, or that the lights are set by hand, or off.
    fn line(&self, lang: &str) -> String {
        let l = labels(lang);
        let parts: Vec<&str> = self
            .modes
            .iter()
            .map(|m| name(lang, m.id()))
            .chain(self.mood.map(|m| name(lang, m)))
            .collect();
        if !parts.is_empty() {
            format!("{}{}", l.playing, parts.join(" + "))
        } else if self.lit {
            l.by_hand.to_string()
        } else {
            l.dark.to_string()
        }
    }
}

fn now(state: &AppState) -> Now {
    let modes = running_modes(state);
    let driven = state.effects.driven_ids();
    match state.lock_config() {
        Ok(cfg) => Now::of(&cfg, modes, &driven),
        Err(_) => Now {
            modes,
            mood: None,
            lit: false,
        },
    }
}

/// Everything the menu and the tooltip show, so they are rebuilt only when
/// it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    lang: &'static str,
    now: Now,
    has_lights: bool,
    timer: Option<u32>,
    /// The version installed and waiting for a restart, if any.
    update: Option<String>,
}

/// What the tray showed when it was last brought up to date.
static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

fn shown(app: &AppHandle, state: &AppState) -> Shown {
    Shown {
        lang: language(state),
        now: now(state),
        has_lights: state.lock_config().is_ok_and(|c| !c.bindings.is_empty()),
        timer: crate::sleep::minutes(state),
        update: crate::updater::ready(app),
    }
}

/// Build the menu for what it shows.
fn build_menu(app: &AppHandle, shown: &Shown) -> tauri::Result<Menu<Wry>> {
    let (lang, now, has_lights, timer) = (shown.lang, &shown.now, shown.has_lights, shown.timer);
    let l = labels(lang);
    let running = &now.modes;

    // What the lights show, as a line that cannot be clicked.
    let status = MenuItem::with_id(app, "status", now.line(lang), false, None::<&str>)?;

    let toggle_label = match running.as_slice() {
        [] => l.start,
        [Mode::Ambilight] => l.stop,
        [Mode::Show(_)] => l.stop_show,
        _ => l.stop_all,
    };
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        toggle_label,
        has_lights || !running.is_empty(),
        None::<&str>,
    )?;

    let shows = Submenu::with_id(app, "shows", l.shows, has_lights)?;
    for kind in SHOWS {
        shows.append(&CheckMenuItem::with_id(
            app,
            format!("{SHOW_PREFIX}{}", kind.id()),
            name(lang, kind.id()),
            true,
            running.contains(&Mode::Show(kind)),
            None::<&str>,
        )?)?;
    }
    // A mood is ticked while every light no effect drives shows it.
    let scenes = Submenu::with_id(app, "scenes", l.scenes, has_lights)?;
    scenes.append(&CheckMenuItem::with_id(
        app,
        format!("{SCENE_PREFIX}{NATURAL}"),
        name(lang, NATURAL),
        true,
        now.mood == Some(NATURAL),
        None::<&str>,
    )?)?;
    scenes.append(&PredefinedMenuItem::separator(app)?)?;
    for m in MOODS {
        scenes.append(&CheckMenuItem::with_id(
            app,
            format!("{SCENE_PREFIX}{}", m.id),
            name(lang, m.id),
            true,
            now.mood == Some(m.id),
            None::<&str>,
        )?)?;
    }

    let sleep = Submenu::with_id(app, "sleep", l.sleep, has_lights)?;
    for m in SLEEP_PRESETS {
        sleep.append(&CheckMenuItem::with_id(
            app,
            format!("{SLEEP_PREFIX}{m}"),
            format!("{m}\u{a0}min"),
            true,
            timer == Some(m),
            None::<&str>,
        )?)?;
    }
    sleep.append(&PredefinedMenuItem::separator(app)?)?;
    sleep.append(&MenuItem::with_id(
        app,
        "sleep_cancel",
        l.sleep_cancel,
        timer.is_some(),
        None::<&str>,
    )?)?;

    let all_on = MenuItem::with_id(app, "all_on", l.all_on, has_lights, None::<&str>)?;
    let all_off = MenuItem::with_id(app, "all_off", l.all_off, has_lights, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", l.open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &shows,
            &scenes,
            &sleep,
            &PredefinedMenuItem::separator(app)?,
            &all_on,
            &all_off,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &quit,
        ],
    )?;
    // An update installed while the app runs is offered right under the
    // status, as the title bar offers it.
    if let Some(version) = &shown.update {
        let label = format!("{} (v{version})", l.update);
        let restart = MenuItem::with_id(app, "update", label, true, None::<&str>)?;
        menu.insert_items(&[&restart, &PredefinedMenuItem::separator(app)?], 2)?;
    }
    Ok(menu)
}

/// The tooltip: the app's name and what its lights show.
fn tooltip(shown: &Shown) -> String {
    format!("Lueur\n{}", shown.now.line(shown.lang))
}

/// Build the tray icon and its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let first = shown(app, &app.state::<AppState>());
    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(&first))
        .icon(app.default_window_icon().cloned().expect("bundled icon"))
        .menu(&build_menu(app, &first)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => in_background(app, |state| {
                crate::auto::user_took_over(state);
                if state.is_running() {
                    lights::stop_effect(state, true);
                    Ok(())
                } else {
                    lights::start(state, Mode::Ambilight).map(drop)
                }
            }),
            "all_on" => in_background(app, |state| lights::set_all_power(state, true).map(drop)),
            "all_off" => in_background(app, |state| lights::set_all_power(state, false).map(drop)),
            "sleep_cancel" => {
                if let Err(e) = crate::sleep::set(app, None) {
                    log::warn!("cannot cancel the sleep timer: {e}");
                }
            }
            "open" => focus_main(app),
            "update" => crate::updater::restart_app(app.clone()),
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    lights::quit(&state);
                }
                app.exit(0);
            }
            id => {
                if let Some(scene) = id.strip_prefix(SCENE_PREFIX) {
                    let scene = scene.to_string();
                    in_background(app, move |state| {
                        lights::apply_scene(state, &scene, None).map(drop)
                    });
                } else if let Some(m) = id.strip_prefix(SLEEP_PREFIX).and_then(|m| m.parse().ok()) {
                    // Picking the timer that runs cancels it, like its tick.
                    let again = crate::sleep::minutes(&app.state::<AppState>()) == Some(m);
                    if let Err(e) = crate::sleep::set(app, (!again).then_some(m)) {
                        log::warn!("cannot set the sleep timer: {e}");
                    }
                } else if let Some(kind) = id.strip_prefix(SHOW_PREFIX).and_then(ShowKind::from_id)
                {
                    in_background(app, move |state| {
                        crate::auto::user_took_over(state);
                        // Picking the show that plays stops it, like its tick.
                        if running_modes(state).contains(&Mode::Show(kind)) {
                            lights::stop_mode(state, Mode::Show(kind), true);
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

/// Bring the menu and tooltip in line with what the lights show and the
/// language.
pub fn refresh(app: &AppHandle) {
    update(app, true);
}

/// [`refresh`], but only if what the tray shows changed: cheap enough for
/// a light set by hand, which a slider does many times a second.
pub fn sync(app: &AppHandle) {
    update(app, false);
}

fn update(app: &AppHandle, always: bool) {
    let (Some(tray), Some(state)) = (app.tray_by_id(TRAY_ID), app.try_state::<AppState>()) else {
        return;
    };
    let shown = shown(app, &state);
    {
        let mut last = SHOWN.lock().unwrap_or_else(|p| p.into_inner());
        if !always && last.as_ref() == Some(&shown) {
            return;
        }
        *last = Some(shown.clone());
    }
    if let Ok(menu) = build_menu(app, &shown) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip(&shown)));
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
        lights_changed(&app, result.err());
    });
}

/// Tell the window that the effects and the lights changed, with the error
/// the change ran into if any, and bring the tray menu up to date.
pub fn lights_changed(app: &AppHandle, error: Option<AppError>) {
    let _ = app.emit(EFFECT_CHANGED, error);
    let _ = app.emit(LIGHTS_CHANGED, ());
    refresh(app);
}

/// Show and focus the main window.
pub fn focus_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(states: serde_json::Value, natural: &[&str]) -> AppConfig {
        AppConfig {
            bindings: serde_json::from_value(serde_json::json!([
                { "light_id": "a", "device_id": "d1", "ip": "192.168.1.5" },
                { "light_id": "b", "device_id": "d2", "ip": "192.168.1.6" },
            ]))
            .unwrap(),
            states: serde_json::from_value(states).unwrap(),
            natural: natural.iter().map(|s| s.to_string()).collect(),
            ..AppConfig::default()
        }
    }

    fn relax() -> serde_json::Value {
        serde_json::to_value(lueur_core::scene::mood("relax").unwrap().state()).unwrap()
    }

    fn off() -> serde_json::Value {
        serde_json::to_value(LightState {
            on: false,
            ..LightState::default()
        })
        .unwrap()
    }

    #[test]
    fn a_mood_the_whole_room_shows_is_named() {
        let cfg = room(serde_json::json!({ "a": relax(), "b": relax() }), &[]);
        let now = Now::of(&cfg, vec![], &[]);
        assert_eq!(now.mood, Some("relax"));
        assert_eq!(now.line("en"), "Playing: Relax");
        assert_eq!(now.line("fr"), "En cours\u{a0}: Détente");
    }

    #[test]
    fn effects_come_first_then_the_mood_of_the_rest() {
        let cfg = room(serde_json::json!({ "a": relax(), "b": relax() }), &[]);
        let modes = vec![Mode::Show(ShowKind::Rainbow), Mode::Ambilight];
        let now = Now::of(&cfg, modes, &["a".to_string()]);
        assert_eq!(now.modes, [Mode::Ambilight, Mode::Show(ShowKind::Rainbow)]);
        assert_eq!(now.line("en"), "Playing: Screen + Rainbow + Relax");
    }

    #[test]
    fn mixed_lights_are_set_by_hand_and_dark_ones_are_off() {
        let mixed = room(serde_json::json!({ "a": relax(), "b": off() }), &[]);
        assert_eq!(Now::of(&mixed, vec![], &[]).mood, None);
        assert_eq!(
            Now::of(&mixed, vec![], &[]).line("en"),
            "Lights set by hand"
        );

        let dark = room(serde_json::json!({ "a": off(), "b": off() }), &[]);
        assert_eq!(
            Now::of(&dark, vec![], &[]).line("fr"),
            "Toutes les lumières sont éteintes"
        );
    }

    #[test]
    fn the_natural_light_is_a_mood_of_its_own() {
        // Whatever white it shows now, a room that follows it shows it.
        let cfg = room(
            serde_json::json!({ "a": relax(), "b": relax() }),
            &["a", "b"],
        );
        assert_eq!(Now::of(&cfg, vec![], &[]).mood, Some(NATURAL));
        // Half of it does not: no mood at all.
        let half = room(serde_json::json!({ "a": relax(), "b": relax() }), &["a"]);
        assert_eq!(Now::of(&half, vec![], &[]).mood, None);
    }

    #[test]
    fn a_room_the_ambilight_drives_whole_has_no_mood() {
        let cfg = room(serde_json::json!({ "a": relax(), "b": relax() }), &[]);
        let all = ["a".to_string(), "b".to_string()];
        let now = Now::of(&cfg, vec![Mode::Ambilight], &all);
        assert_eq!(now.mood, None);
        assert_eq!(now.line("fr"), "En cours\u{a0}: Écran");
    }

    #[test]
    fn everything_the_menu_names_has_a_name() {
        for lang in ["en", "fr"] {
            for id in MOODS.iter().map(|m| m.id).chain([NATURAL, "ambilight"]) {
                assert!(!name(lang, id).is_empty(), "{lang} {id}");
            }
            for kind in SHOWS {
                assert!(!name(lang, kind.id()).is_empty(), "{lang} {}", kind.id());
            }
        }
    }
}
