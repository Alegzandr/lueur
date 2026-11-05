//! What the system-tray menu shows and what its items do, whatever draws it.
//!
//! The tray is how Lueur is used mid-game, and how the lights are switched
//! without opening the window: the menu says what the lights show, starts
//! and stops following the screen and the effects, applies the moods, sets the
//! sleep timer, turns every light on or off and, once an update is
//! installed, restarts into it. What plays and what the
//! room shows are ticked. Its labels follow the interface language the
//! frontend reports, in the interface's words.

use crate::Shell;
use crate::error::AppResult;
use crate::lights;
use crate::runner::Mode;
use crate::state::AppState;
use lueur_core::config::AppConfig;
use lueur_core::scene::{LightState, MOODS, NATURAL};
use lueur_core::show::{SHOWS, ShowKind};
use std::sync::{Arc, Mutex};

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
pub struct Shown {
    lang: &'static str,
    now: Now,
    has_lights: bool,
    timer: Option<u32>,
    /// The version installed and waiting for a restart, if any.
    update: Option<String>,
}

/// What the tray shows now.
pub fn shown(shell: &Shell) -> Shown {
    let state: &AppState = shell;
    Shown {
        lang: language(state),
        now: now(state),
        has_lights: state.lock_config().is_ok_and(|c| !c.bindings.is_empty()),
        timer: crate::sleep::minutes(state),
        update: shell.update_ready(),
    }
}

/// What the tray showed when it was last brought up to date.
#[derive(Default)]
pub struct Tracker(Mutex<Option<Shown>>);

impl Tracker {
    /// What the tray must show now, or None when `always` is false and it
    /// shows that already.
    pub fn next(&self, shell: &Shell, always: bool) -> Option<Shown> {
        let shown = shown(shell);
        let mut last = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if !always && last.as_ref() == Some(&shown) {
            return None;
        }
        *last = Some(shown.clone());
        Some(shown)
    }
}

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// An item that does something when picked.
    Action {
        id: String,
        label: String,
        enabled: bool,
    },
    /// An item ticked while what it names plays or shows.
    Check {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
    },
    Submenu {
        label: String,
        enabled: bool,
        items: Vec<Item>,
    },
    Separator,
}

fn action(id: impl Into<String>, label: impl Into<String>, enabled: bool) -> Item {
    Item::Action {
        id: id.into(),
        label: label.into(),
        enabled,
    }
}

fn check(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Item {
    Item::Check {
        id: id.into(),
        label: label.into(),
        enabled: true,
        checked,
    }
}

/// The menu for what it shows.
pub fn menu(shown: &Shown) -> Vec<Item> {
    let (lang, now, has_lights, timer) = (shown.lang, &shown.now, shown.has_lights, shown.timer);
    let l = labels(lang);
    let running = &now.modes;

    let toggle_label = match running.as_slice() {
        [] => l.start,
        [Mode::Ambilight] => l.stop,
        [Mode::Show(_)] => l.stop_show,
        _ => l.stop_all,
    };

    let shows = SHOWS
        .iter()
        .map(|kind| {
            check(
                format!("{SHOW_PREFIX}{}", kind.id()),
                name(lang, kind.id()),
                running.contains(&Mode::Show(*kind)),
            )
        })
        .collect();

    // A mood is ticked while every light no effect drives shows it.
    let mut scenes = vec![
        check(
            format!("{SCENE_PREFIX}{NATURAL}"),
            name(lang, NATURAL),
            now.mood == Some(NATURAL),
        ),
        Item::Separator,
    ];
    scenes.extend(MOODS.iter().map(|m| {
        check(
            format!("{SCENE_PREFIX}{}", m.id),
            name(lang, m.id),
            now.mood == Some(m.id),
        )
    }));

    let mut sleep: Vec<Item> = SLEEP_PRESETS
        .iter()
        .map(|m| {
            check(
                format!("{SLEEP_PREFIX}{m}"),
                format!("{m}\u{a0}min"),
                timer == Some(*m),
            )
        })
        .collect();
    sleep.push(Item::Separator);
    sleep.push(action("sleep_cancel", l.sleep_cancel, timer.is_some()));

    // What the lights show, as a line that cannot be clicked.
    let mut items = vec![action("status", now.line(lang), false), Item::Separator];
    // An update installed while the app runs is offered right under the
    // status, as the title bar offers it.
    if let Some(version) = &shown.update {
        items.push(action("update", format!("{} (v{version})", l.update), true));
        items.push(Item::Separator);
    }
    items.extend([
        action("toggle", toggle_label, has_lights || !running.is_empty()),
        Item::Submenu {
            label: l.shows.into(),
            enabled: has_lights,
            items: shows,
        },
        Item::Submenu {
            label: l.scenes.into(),
            enabled: has_lights,
            items: scenes,
        },
        Item::Submenu {
            label: l.sleep.into(),
            enabled: has_lights,
            items: sleep,
        },
        Item::Separator,
        action("all_on", l.all_on, has_lights),
        action("all_off", l.all_off, has_lights),
        Item::Separator,
        action("open", l.open, true),
        action("quit", l.quit, true),
    ]);
    items
}

/// The tooltip: the app's name and what its lights show.
pub fn tooltip(shown: &Shown) -> String {
    format!("Lueur\n{}", shown.now.line(shown.lang))
}

/// What the front end must do after a menu item was picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing: the shell took care of it.
    Done,
    /// Restart into the update installed. The bulbs have been handed back.
    Restart,
    /// Quit. The bulbs have been handed back.
    Quit,
}

/// Carry out the menu item `id`.
pub fn act(shell: &Arc<Shell>, id: &str) -> Outcome {
    match id {
        "toggle" => in_background(shell, |state| {
            crate::auto::user_took_over(state);
            if state.is_running() {
                lights::stop_effect(state, true);
                Ok(())
            } else {
                lights::start(state, Mode::Ambilight).map(drop)
            }
        }),
        "all_on" => in_background(shell, |state| lights::set_all_power(state, true).map(drop)),
        "all_off" => in_background(shell, |state| lights::set_all_power(state, false).map(drop)),
        "sleep_cancel" => {
            if let Err(e) = crate::sleep::set(shell, None) {
                log::warn!("cannot cancel the sleep timer: {e}");
            }
        }
        "open" => shell.focus_main(),
        "update" => {
            // The bulbs are in music mode while the effect runs: hand them
            // back.
            lights::suspend(shell, false);
            return Outcome::Restart;
        }
        "quit" => {
            lights::quit(shell);
            return Outcome::Quit;
        }
        id => {
            if let Some(scene) = id.strip_prefix(SCENE_PREFIX) {
                let scene = scene.to_string();
                in_background(shell, move |state| {
                    lights::apply_scene(state, &scene, None).map(drop)
                });
            } else if let Some(m) = id.strip_prefix(SLEEP_PREFIX).and_then(|m| m.parse().ok()) {
                // Picking the timer that runs cancels it, like its tick.
                let again = crate::sleep::minutes(shell) == Some(m);
                if let Err(e) = crate::sleep::set(shell, (!again).then_some(m)) {
                    log::warn!("cannot set the sleep timer: {e}");
                }
            } else if let Some(kind) = id.strip_prefix(SHOW_PREFIX).and_then(ShowKind::from_id) {
                in_background(shell, move |state| {
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
    }
    Outcome::Done
}

/// Run a tray action off the event-loop thread (bulbs can take seconds to
/// answer, the music-mode handshake too), then tell the window what changed.
/// A failure brings the window up, since none may be visible to show it.
fn in_background(
    shell: &Arc<Shell>,
    job: impl FnOnce(&AppState) -> AppResult<()> + Send + 'static,
) {
    let shell = Arc::clone(shell);
    std::thread::spawn(move || {
        let result = job(&shell);
        if result.is_err() {
            shell.focus_main();
        }
        shell.lights_changed(result.err());
    });
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
