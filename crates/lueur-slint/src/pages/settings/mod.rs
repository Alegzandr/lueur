//! The Settings page: how the ambilight reads the screen, what the music
//! show listens to, and the app's own preferences. The photosensitivity
//! protection and the black-bar trim have no row: they are always on.
//!
//! Each row is a [`Setting`] worded here and drawn by `settings.slint`; its
//! control reports back by the row's id.

mod actions;
mod choices;

use crate::i18n::{self, Lang, fill, t};
use crate::models::{keep, old, reuse};
use crate::rt;
use crate::state::Lueur;
use crate::{AppWindow, Choice, Setting, SettingGroup, SettingKind, SettingsState};
use choices::{SoundWords, chosen_sound, fps_options, monitor_label, nearest_curve, sound_choices};
use lueur_core::config::{AppConfig, Appearance, AudioSource, Language};
use lueur_shell::audio::AudioSources;
use lueur_shell::capture::MonitorInfo;
use lueur_shell::service;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

/// Whether fullscreen can be detected, which only Windows offers (see
/// `crates/lueur-shell/src/auto.rs`): automatic mode exists nowhere else.
const DETECTS_FULLSCREEN: bool = cfg!(windows);

/// What the page keeps for itself.
#[derive(Default)]
pub struct Local {
    /// The displays, once listed.
    monitors: Vec<MonitorInfo>,
    listed_monitors: bool,
    /// What the music show could listen to; none until first listed.
    sound: Option<AudioSources>,
    /// True while the sound sources are being listed, so a second press
    /// on the picker does not ask again.
    listing_sound: bool,
    /// False when no keyring is available: the credentials are then kept
    /// unencrypted in the config, and the page says so.
    sealed: Option<bool>,
}

pub fn bind(ui: &AppWindow) {
    let g = ui.global::<SettingsState>();
    g.on_toggled(|id| rt::with(|l| toggled(l, &id)));
    g.on_chosen(|id, i| rt::with(|l| chosen(l, &id, i.max(0) as usize)));
    g.on_slid(|id, v| rt::with(|l| slid(l, &id, v)));
    g.on_pressed(|id, i| rt::with(|l| pressed(l, &id, i)));
    g.on_opening(|id| {
        if id == "sound-source" {
            rt::with(refresh_sound);
        }
    });
}

/// List the displays, once, off the window's thread: listing them asks the
/// system for each one.
fn list_monitors(l: &mut Lueur) {
    if l.pages.settings.listed_monitors {
        return;
    }
    l.pages.settings.listed_monitors = true;
    l.pages.settings.sealed = Some(service::credentials_sealed(&l.shell));
    l.run(
        |_| service::list_monitors(),
        |l, monitors| l.pages.settings.monitors = monitors,
    );
    refresh_sound(l);
}

/// List the sound sources again. Applications come and go, so this is
/// asked again whenever the picker is pressed.
fn refresh_sound(l: &mut Lueur) {
    if l.pages.settings.listing_sound {
        return;
    }
    l.pages.settings.listing_sound = true;
    l.run(
        |_| Ok(service::list_audio_sources()),
        |l, sources| {
            l.pages.settings.listing_sound = false;
            l.pages.settings.sound = Some(sources);
        },
    );
}

fn toggled(l: &mut Lueur, id: &str) {
    let b = l.config.behavior;
    match id {
        "auto-mode" => l.edit(|x| x.behavior.auto_fullscreen = !b.auto_fullscreen),
        "open-login" => l.set_run_on_startup(!b.run_on_startup),
        "start-tray" => l.edit(|x| x.behavior.start_minimized = !b.start_minimized),
        "keep-running" => l.edit(|x| x.behavior.minimize_to_tray = !b.minimize_to_tray),
        _ => {}
    }
}

fn chosen(l: &mut Lueur, id: &str, i: usize) {
    match id {
        "display" => {
            if let Some(m) = l.pages.settings.monitors.get(i) {
                let index = m.index;
                l.edit(|x| x.capture.monitor_index = index);
            }
        }
        "frame-rate" => {
            if let Some(&n) = fps_options(l.config.capture.target_fps).get(i) {
                l.edit(|x| x.capture.target_fps = n);
            }
        }
        "dark-scenes" => {
            if let Some((curve, _)) = choices::CURVES.get(i) {
                let curve = *curve;
                l.edit(|x| x.engine.brightness_curve = curve);
            }
        }
        "sound-source" => {
            let Some(sound) = l.pages.settings.sound.clone() else {
                return;
            };
            let list = sound_list(&sound, &l.config.audio);
            let Some(next) = list.get(i).map(|s| s.source.clone()) else {
                return;
            };
            // A choice gone missing keeps the name it was saved with.
            if choices::same_source(&next, &l.config.audio) {
                return;
            }
            l.edit(|x| x.audio = next);
        }
        "language" => {
            let v = [Language::System, Language::En, Language::Fr];
            if let Some(&v) = v.get(i) {
                l.edit(|x| x.interface.language = v);
            }
        }
        "appearance" => {
            let v = [Appearance::System, Appearance::Light, Appearance::Dark];
            if let Some(&v) = v.get(i) {
                l.edit(|x| x.interface.appearance = v);
            }
        }
        _ => {}
    }
}

fn slid(l: &mut Lueur, id: &str, v: f32) {
    match id {
        "colour-strength" => l.edit(|x| x.engine.saturation = v),
        "sound-delay" => l.edit(|x| x.show.delay_ms = v.round() as i16),
        _ => {}
    }
}

fn pressed(l: &mut Lueur, id: &str, i: i32) {
    match (id, i) {
        ("config-file", 0) => l.export_config(),
        ("config-file", 1) => l.import_config(),
        ("logs", _) => l.open_logs_folder(),
        _ => {}
    }
}

/// The sound sources as the picker lists them.
fn sound_list(sound: &AudioSources, current: &AudioSource) -> Vec<choices::SoundChoice> {
    let default = t(if sound.loopback {
        "audio_default_output"
    } else {
        "audio_default_input"
    });
    let (missing, closed) = (t("audio_missing"), t("audio_closed"));
    sound_choices(
        sound,
        current,
        &SoundWords {
            default: &default,
            missing: &missing,
            closed: &closed,
        },
    )
}

fn options(labels: impl IntoIterator<Item = String>) -> ModelRc<Choice> {
    let list: Vec<Choice> = labels
        .into_iter()
        .map(|label| Choice {
            label: label.into(),
            tip: SharedString::new(),
        })
        .collect();
    ModelRc::new(VecModel::from(list))
}

fn row(id: &str, kind: SettingKind, label: &str, tip: Option<&str>) -> Setting {
    Setting {
        id: id.into(),
        kind,
        label: t(label).into(),
        tip: tip.map(t).unwrap_or_default().into(),
        default_value: -1.0,
        current: -1,
        ..Default::default()
    }
}

fn switch(id: &str, label: &str, tip: Option<&str>, on: bool) -> Setting {
    Setting {
        on,
        ..row(id, SettingKind::Switch, label, tip)
    }
}

fn pick(
    id: &str,
    kind: SettingKind,
    label: &str,
    tip: Option<&str>,
    labels: Vec<String>,
    current: Option<usize>,
) -> Setting {
    Setting {
        options: options(labels),
        current: current.map_or(-1, |c| c as i32),
        ..row(id, kind, label, tip)
    }
}

/// The words of a slider's reset mark: "Reset to 35%".
fn reset_label(default: &str) -> String {
    fill(&t("reset_to"), &[("v", default)])
}

/// The sound delay as it reads: "+40 ms", "0 ms", "-20 ms".
fn delay_text(ms: &str, v: f32) -> String {
    let n = v.round() as i32;
    let n = if n > 0 {
        format!("+{n}")
    } else {
        n.to_string()
    };
    fill(ms, &[("n", &n)])
}

fn ambilight(local: &Local, c: &AppConfig, lang: Lang) -> Vec<Setting> {
    let mut rows = Vec::new();
    if DETECTS_FULLSCREEN {
        rows.push(switch(
            "auto-mode",
            "auto_mode",
            Some("auto_mode_tip"),
            c.behavior.auto_fullscreen,
        ));
    }
    if local.monitors.len() > 1 {
        let main = t("display_main");
        rows.push(Setting {
            wide: true,
            ..pick(
                "display",
                SettingKind::Select,
                "display",
                None,
                local
                    .monitors
                    .iter()
                    .map(|m| monitor_label(m, &main))
                    .collect(),
                local
                    .monitors
                    .iter()
                    .position(|m| m.index == c.capture.monitor_index),
            )
        });
    }
    let fps = fps_options(c.capture.target_fps);
    rows.push(pick(
        "frame-rate",
        SettingKind::Segmented,
        "frame_rate",
        Some("frame_rate_tip"),
        fps.iter().map(u32::to_string).collect(),
        fps.iter().position(|n| *n == c.capture.target_fps),
    ));
    let def = AppConfig::default().engine.saturation;
    rows.push(Setting {
        value: c.engine.saturation,
        minimum: 0.0,
        maximum: 1.0,
        step: 0.05,
        default_value: def,
        value_text: i18n::pct(lang, c.engine.saturation).into(),
        reset_label: reset_label(&i18n::pct(lang, def)).into(),
        low_end: t("colour_strength_true").into(),
        high_end: t("colour_strength_vivid").into(),
        ..row(
            "colour-strength",
            SettingKind::Slider,
            "colour_strength",
            Some("colour_strength_tip"),
        )
    });
    rows.push(Setting {
        wide: true,
        ..pick(
            "dark-scenes",
            SettingKind::Segmented,
            "dark_scenes",
            Some("dark_scenes_tip"),
            choices::CURVES.iter().map(|(_, key)| t(key)).collect(),
            Some(nearest_curve(c.engine.brightness_curve)),
        )
    });
    rows
}

fn music(sound: &AudioSources, c: &AppConfig) -> Vec<Setting> {
    let list = sound_list(sound, &c.audio);
    let chosen = chosen_sound(&list, &c.audio);
    let ms = t("ms");
    let delay = f32::from(c.show.delay_ms);
    vec![
        Setting {
            wide: true,
            ..pick(
                "sound-source",
                SettingKind::Select,
                "sound_source",
                sound.loopback.then_some("sound_source_tip"),
                list.iter().map(|s| s.label.clone()).collect(),
                chosen,
            )
        },
        Setting {
            value: delay,
            minimum: -150.0,
            maximum: 500.0,
            step: 10.0,
            default_value: 0.0,
            value_text: delay_text(&ms, delay).into(),
            reset_label: reset_label(&delay_text(&ms, 0.0)).into(),
            low_end: t("sound_delay_early").into(),
            high_end: t("sound_delay_late").into(),
            ..row(
                "sound-delay",
                SettingKind::Slider,
                "sound_delay",
                Some("sound_delay_tip"),
            )
        },
    ]
}

fn application(c: &AppConfig) -> Vec<Setting> {
    let language = [Language::System, Language::En, Language::Fr];
    let appearance = [Appearance::System, Appearance::Light, Appearance::Dark];
    vec![
        // Language names are written in their own language, so anyone can
        // find theirs.
        pick(
            "language",
            SettingKind::Segmented,
            "language",
            None,
            vec![t("automatic"), "English".into(), "Français".into()],
            language.iter().position(|v| *v == c.interface.language),
        ),
        pick(
            "appearance",
            SettingKind::Segmented,
            "appearance",
            None,
            vec![t("automatic"), t("theme_light"), t("theme_dark")],
            appearance.iter().position(|v| *v == c.interface.appearance),
        ),
        switch("open-login", "open_login", None, c.behavior.run_on_startup),
        switch("start-tray", "start_tray", None, c.behavior.start_minimized),
        switch(
            "keep-running",
            "keep_running",
            None,
            c.behavior.minimize_to_tray,
        ),
        pick(
            "config-file",
            SettingKind::Buttons,
            "config_file",
            Some("config_file_tip"),
            vec![t("export"), t("import")],
            None,
        ),
        pick(
            "logs",
            SettingKind::Buttons,
            "logs",
            Some("logs_tip"),
            vec![t("open_logs")],
            None,
        ),
    ]
}

fn group(title: &str, rows: Vec<Setting>, note: Option<String>) -> SettingGroup {
    SettingGroup {
        title: t(title).into(),
        rows: ModelRc::new(VecModel::from(rows)),
        note: note.unwrap_or_default().into(),
    }
}

/// `groups` with every list inside them moved into the models the window
/// already holds, so a row that did not change compares equal.
fn settle(current: &ModelRc<SettingGroup>, groups: Vec<SettingGroup>) -> Vec<SettingGroup> {
    use slint::Model;
    groups
        .into_iter()
        .enumerate()
        .map(|(i, mut g)| {
            let before = old(current, i);
            let rows: Vec<Setting> = g
                .rows
                .iter()
                .enumerate()
                .map(|(j, mut s)| {
                    let was = old(&before.rows, j);
                    s.options = reuse(&was.options, s.options.iter().collect());
                    s
                })
                .collect();
            g.rows = reuse(&before.rows, rows);
            g
        })
        .collect()
}

pub fn project(l: &Lueur, ui: &AppWindow) {
    if !l.pages.settings.listed_monitors {
        // Listed the first time the page shows, once the state is free.
        slint::Timer::single_shot(std::time::Duration::ZERO, || rt::with(list_monitors));
    }
    let local = &l.pages.settings;
    let c = &l.config;
    let mut groups = vec![group("mode_ambilight", ambilight(local, c, l.lang()), None)];
    if let Some(sound) = &local.sound {
        groups.push(group("mode_music", music(sound, c), None));
    }
    let unsealed = (local.sealed == Some(false)).then(|| t("credentials_unsealed"));
    groups.push(group("application", application(c), unsealed));

    let g = ui.global::<SettingsState>();
    let groups = settle(&g.get_groups(), groups);
    keep(g.get_groups(), |m| g.set_groups(m), groups);
    g.set_version(
        format!(
            "Lueur {} · {}",
            env!("CARGO_PKG_VERSION"),
            t("made_with_slint")
        )
        .into(),
    );
}
