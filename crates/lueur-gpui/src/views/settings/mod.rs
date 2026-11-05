//! The Settings page: how the ambilight reads the screen, what the music
//! show listens to, and the app's own preferences. The photosensitivity
//! protection and the black-bar trim have no row: they are always on.

mod actions;
mod choices;

use crate::i18n::{self, Lang, fill, t};
use crate::state::Lueur;
use crate::theme::{Theme, size, theme};
use crate::ui::{self, choice};
use choices::{SoundWords, chosen_sound, fps_options, monitor_label, nearest_curve, sound_choices};
use gpui::{
    AnyElement, App, Context, Entity, IntoElement, Render, Role, SharedString, Window, div,
    prelude::*, px,
};
use lueur_core::config::{AppConfig, Appearance, AudioSource, Language};
use lueur_shell::audio::AudioSources;
use lueur_shell::capture::MonitorInfo;
use lueur_shell::service;

/// Whether fullscreen can be detected, which only Windows offers (see
/// `crates/lueur-shell/src/auto.rs`): automatic mode exists nowhere else.
const DETECTS_FULLSCREEN: bool = cfg!(windows);

pub struct SettingsPage {
    lueur: Entity<Lueur>,
    monitors: Vec<MonitorInfo>,
    /// What the music show could listen to; none until first listed.
    sound: Option<AudioSources>,
    /// True while the sound sources are being listed, so a second press
    /// on the picker does not ask again.
    listing_sound: bool,
    /// False when no keyring is available: the credentials are then kept
    /// unencrypted in the config, and the page says so.
    sealed: bool,
}

impl SettingsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lueur = super::watch(cx);
        let sealed = service::credentials_sealed(&lueur.read(cx).shell);
        // Listing the displays asks the system for each one: off the
        // window's thread.
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_executor()
                .spawn(async { service::list_monitors() })
                .await;
            let _ = this.update(cx, |this, cx| match listed {
                Ok(monitors) => {
                    this.monitors = monitors;
                    cx.notify();
                }
                Err(e) => this.lueur.update(cx, |l, cx| l.fail(&e, cx)),
            });
        })
        .detach();
        let mut page = SettingsPage {
            lueur,
            monitors: Vec::new(),
            sound: None,
            listing_sound: false,
            sealed,
        };
        page.refresh_sound(cx);
        page
    }

    /// List the sound sources again. Applications come and go, so this is
    /// asked again whenever the picker is pressed.
    fn refresh_sound(&mut self, cx: &mut Context<Self>) {
        if self.listing_sound {
            return;
        }
        self.listing_sound = true;
        cx.spawn(async move |this, cx| {
            let sources = cx
                .background_executor()
                .spawn(async { service::list_audio_sources() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.listing_sound = false;
                this.sound = Some(sources);
                cx.notify();
            });
        })
        .detach();
    }

    fn ambilight(
        &self,
        c: &AppConfig,
        lang: Lang,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows: Vec<(SharedString, Option<SharedString>, AnyElement)> = Vec::new();

        if DETECTS_FULLSCREEN {
            let lueur = self.lueur.clone();
            let on = c.behavior.auto_fullscreen;
            rows.push((
                t(cx, "auto_mode"),
                Some(t(cx, "auto_mode_tip")),
                ui::switch(
                    "auto-mode",
                    t(cx, "auto_mode"),
                    on,
                    theme,
                    move |_, _, cx| {
                        lueur.update(cx, |l, cx| l.edit(cx, |x| x.behavior.auto_fullscreen = !on))
                    },
                )
                .into_any_element(),
            ));
        }

        if self.monitors.len() > 1 {
            let main = t(cx, "display_main");
            let options = self
                .monitors
                .iter()
                .map(|m| choice(m.index, monitor_label(m, &main).into()))
                .collect();
            let lueur = self.lueur.clone();
            rows.push((
                t(cx, "display"),
                None,
                wide(ui::select(
                    "display",
                    t(cx, "display"),
                    Some(&c.capture.monitor_index),
                    options,
                    theme,
                    window,
                    cx,
                    move |index, _, cx| {
                        lueur.update(cx, |l, cx| l.edit(cx, |x| x.capture.monitor_index = index))
                    },
                )),
            ));
        }

        let lueur = self.lueur.clone();
        rows.push((
            t(cx, "frame_rate"),
            Some(t(cx, "frame_rate_tip")),
            ui::segmented(
                "frame-rate",
                t(cx, "frame_rate"),
                Some(&c.capture.target_fps),
                fps_options(c.capture.target_fps)
                    .into_iter()
                    .map(|n| choice(n, n.to_string().into()))
                    .collect(),
                theme,
                move |n, _, cx| lueur.update(cx, |l, cx| l.edit(cx, |x| x.capture.target_fps = n)),
            ),
        ));

        let lueur = self.lueur.clone();
        let def = AppConfig::default().engine.saturation;
        rows.push((
            t(cx, "colour_strength"),
            Some(t(cx, "colour_strength_tip")),
            wide(
                ui::slider(
                    "colour-strength",
                    t(cx, "colour_strength"),
                    c.engine.saturation,
                    (0.0, 1.0, 0.05),
                    move |v, _, cx| {
                        lueur.update(cx, |l, cx| l.edit(cx, |x| x.engine.saturation = v))
                    },
                )
                .default_value(def, reset_label(cx, &i18n::pct(lang, def)))
                .format(move |v| i18n::pct(lang, v))
                .ends(
                    t(cx, "colour_strength_true"),
                    t(cx, "colour_strength_vivid"),
                )
                .render(theme, window, cx),
            ),
        ));

        let lueur = self.lueur.clone();
        rows.push((
            t(cx, "dark_scenes"),
            Some(t(cx, "dark_scenes_tip")),
            ui::segmented(
                "dark-scenes",
                t(cx, "dark_scenes"),
                Some(&nearest_curve(c.engine.brightness_curve)),
                choices::CURVES
                    .iter()
                    .enumerate()
                    .map(|(i, (_, key))| choice(i, t(cx, key)))
                    .collect(),
                theme,
                move |i, _, cx| {
                    let curve = choices::CURVES[i].0;
                    lueur.update(cx, |l, cx| {
                        l.edit(cx, |x| x.engine.brightness_curve = curve)
                    })
                },
            ),
        ));

        rows_of(rows, theme)
    }

    fn music(
        &self,
        sound: &AudioSources,
        c: &AppConfig,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let default = t(
            cx,
            if sound.loopback {
                "audio_default_output"
            } else {
                "audio_default_input"
            },
        );
        let (missing, closed) = (t(cx, "audio_missing"), t(cx, "audio_closed"));
        let list = sound_choices(
            sound,
            &c.audio,
            &SoundWords {
                default: &default,
                missing: &missing,
                closed: &closed,
            },
        );
        let chosen = chosen_sound(&list, &c.audio);
        let current = c.audio.clone();
        let lueur = self.lueur.clone();
        let picker = ui::select(
            "sound-source",
            t(cx, "sound_source"),
            chosen.as_ref(),
            list.iter()
                .enumerate()
                .map(|(i, s)| choice(i, s.label.clone().into()))
                .collect(),
            theme,
            window,
            cx,
            move |i, _, cx| {
                let next: AudioSource = list[i].source.clone();
                // A choice gone missing keeps the name it was saved with.
                if choices::same_source(&next, &current) {
                    return;
                }
                lueur.update(cx, |l, cx| l.edit(cx, |x| x.audio = next));
            },
        );
        // Pressed, the picker lists the sources again while it opens.
        let page = cx.entity();
        let picker = wide(
            div()
                .capture_any_mouse_down(move |_, _, cx| {
                    page.update(cx, |p, cx| p.refresh_sound(cx));
                })
                .child(picker),
        );

        let lueur = self.lueur.clone();
        let delay = c.show.delay_ms;
        let ms = t(cx, "ms");
        let rows = vec![
            (
                t(cx, "sound_source"),
                sound.loopback.then(|| t(cx, "sound_source_tip")),
                picker,
            ),
            (
                t(cx, "sound_delay"),
                Some(t(cx, "sound_delay_tip")),
                wide(
                    ui::slider(
                        "sound-delay",
                        t(cx, "sound_delay"),
                        f32::from(delay),
                        (-150.0, 500.0, 10.0),
                        move |v, _, cx| {
                            lueur.update(cx, |l, cx| {
                                l.edit(cx, |x| x.show.delay_ms = v.round() as i16)
                            })
                        },
                    )
                    .default_value(0.0, reset_label(cx, &delay_text(&ms, 0.0)))
                    .format(move |v| delay_text(&ms, v))
                    .ends(t(cx, "sound_delay_early"), t(cx, "sound_delay_late"))
                    .render(theme, window, cx),
                ),
            ),
        ];
        rows_of(rows, theme)
    }

    fn application(&self, c: &AppConfig, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows: Vec<(SharedString, Option<SharedString>, AnyElement)> = Vec::new();

        // Language names are written in their own language, so anyone can
        // find theirs.
        let lueur = self.lueur.clone();
        rows.push((
            t(cx, "language"),
            None,
            ui::segmented(
                "language",
                t(cx, "language"),
                Some(&c.interface.language),
                vec![
                    choice(Language::System, t(cx, "automatic")),
                    choice(Language::En, "English".into()),
                    choice(Language::Fr, "Français".into()),
                ],
                theme,
                move |v, _, cx| lueur.update(cx, |l, cx| l.edit(cx, |x| x.interface.language = v)),
            ),
        ));

        let lueur = self.lueur.clone();
        rows.push((
            t(cx, "appearance"),
            None,
            ui::segmented(
                "appearance",
                t(cx, "appearance"),
                Some(&c.interface.appearance),
                vec![
                    choice(Appearance::System, t(cx, "automatic")),
                    choice(Appearance::Light, t(cx, "theme_light")),
                    choice(Appearance::Dark, t(cx, "theme_dark")),
                ],
                theme,
                move |v, _, cx| {
                    lueur.update(cx, |l, cx| l.edit(cx, |x| x.interface.appearance = v))
                },
            ),
        ));

        let lueur = self.lueur.clone();
        let on = c.behavior.run_on_startup;
        rows.push((
            t(cx, "open_login"),
            None,
            ui::switch(
                "open-login",
                t(cx, "open_login"),
                on,
                theme,
                move |_, _, cx| lueur.update(cx, |l, cx| l.set_run_on_startup(!on, cx)),
            )
            .into_any_element(),
        ));

        let lueur = self.lueur.clone();
        let on = c.behavior.start_minimized;
        rows.push((
            t(cx, "start_tray"),
            None,
            ui::switch(
                "start-tray",
                t(cx, "start_tray"),
                on,
                theme,
                move |_, _, cx| {
                    lueur.update(cx, |l, cx| l.edit(cx, |x| x.behavior.start_minimized = !on))
                },
            )
            .into_any_element(),
        ));

        let lueur = self.lueur.clone();
        let on = c.behavior.minimize_to_tray;
        rows.push((
            t(cx, "keep_running"),
            None,
            ui::switch(
                "keep-running",
                t(cx, "keep_running"),
                on,
                theme,
                move |_, _, cx| {
                    lueur.update(cx, |l, cx| {
                        l.edit(cx, |x| x.behavior.minimize_to_tray = !on)
                    })
                },
            )
            .into_any_element(),
        ));

        let (export, import) = (self.lueur.clone(), self.lueur.clone());
        rows.push((
            t(cx, "config_file"),
            Some(t(cx, "config_file_tip")),
            div()
                .flex()
                .gap(px(size::S2))
                .child(ui::button(
                    "export",
                    t(cx, "export"),
                    false,
                    theme,
                    move |_, _, cx| export.update(cx, |l, cx| l.export_config(cx)),
                ))
                .child(ui::button(
                    "import",
                    t(cx, "import"),
                    false,
                    theme,
                    move |_, _, cx| import.update(cx, |l, cx| l.import_config(cx)),
                ))
                .into_any_element(),
        ));

        let lueur = self.lueur.clone();
        rows.push((
            t(cx, "logs"),
            Some(t(cx, "logs_tip")),
            ui::button(
                "open-logs",
                t(cx, "open_logs"),
                false,
                theme,
                move |_, _, cx| lueur.update(cx, |l, cx| l.open_logs_folder(cx)),
            )
            .into_any_element(),
        ));

        rows_of(rows, theme)
    }
}

/// The words of a slider's reset mark: "Reset to 35%".
fn reset_label(cx: &App, default: &str) -> SharedString {
    fill(&t(cx, "reset_to"), &[("v", default)]).into()
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

/// Rows of a card, each with its label, its tip and its control.
fn rows_of(
    rows: Vec<(SharedString, Option<SharedString>, AnyElement)>,
    theme: &Theme,
) -> Vec<AnyElement> {
    rows.into_iter()
        .enumerate()
        .map(|(i, (label, tip, control))| ui::row(i, label, tip, control, theme).into_any_element())
        .collect()
}

/// A titled group of rows.
fn group(title: SharedString, rows: Vec<AnyElement>, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(size::S2))
        .child(ui::section(title, theme))
        .child(ui::card(theme).children(rows))
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let (config, lang) = {
            let l = self.lueur.read(cx);
            (l.config.clone(), l.lang())
        };

        let ambilight = self.ambilight(&config, lang, &theme, window, cx);
        let music = self
            .sound
            .clone()
            .map(|sound| self.music(&sound, &config, &theme, window, cx));
        let application = self.application(&config, &theme, cx);
        let unsealed = (!self.sealed).then(|| t(cx, "credentials_unsealed"));
        let version = SharedString::from(format!("Lueur {}", env!("CARGO_PKG_VERSION")));

        div()
            .flex()
            .flex_col()
            .gap(px(size::S5))
            .child(ui::page_title("page-title", t(cx, "nav_settings"), &theme))
            .child(group(t(cx, "mode_ambilight"), ambilight, &theme))
            .when_some(music, |d, rows| {
                d.child(group(t(cx, "mode_music"), rows, &theme))
            })
            .child(group(t(cx, "application"), application, &theme).when_some(
                unsealed,
                |d, text| {
                    d.child(
                        div()
                            .id("credentials-unsealed")
                            .role(Role::Status)
                            .aria_label(text.clone())
                            .px(px(size::S4))
                            .text_size(px(size::T_SM))
                            .text_color(theme.text_3)
                            .child(text),
                    )
                },
            ))
            .child(
                div()
                    .mt(px(size::S6 - size::S5))
                    .flex()
                    .justify_center()
                    .text_size(px(size::T_SM))
                    .text_color(theme.text_3)
                    .child(version),
            )
    }
}

/// A control that fills its column, as sliders and pickers do.
fn wide(control: impl IntoElement) -> AnyElement {
    div()
        .w(px(320.0))
        .flex_none()
        .flex()
        .flex_col()
        .child(control)
        .into_any_element()
}
