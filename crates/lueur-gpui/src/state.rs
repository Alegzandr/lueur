//! What the window shows, and every action it can take.
//!
//! One entity, [`Lueur`], holds the window's copy of the config and of what
//! plays; the views read it and call its actions. An action runs the shell's
//! command on the background executor (bulbs and the network block), then
//! brings the copy up to date on the window's thread. The shell's events do
//! the same for changes made elsewhere: the tray, automatic mode, the sleep
//! timer, a bulb that dropped.

use crate::i18n::{self, Lang};
use crate::theme::Theme;
use gpui::{App, AppContext, Context, Entity, Global, WindowAppearance};
use lueur_core::config::AppConfig;
use lueur_shell::error::{AppError, AppResult};
use lueur_shell::identity::Unverified;
use lueur_shell::service::{self, EffectStatus};
use lueur_shell::sleep::Status as SleepStatus;
use lueur_shell::{Event, Shell};
use std::sync::Arc;

/// The pages of the sidebar, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Lighting,
    Room,
    Bulbs,
    Settings,
}

impl View {
    pub const ALL: [View; 4] = [View::Lighting, View::Room, View::Bulbs, View::Settings];

    /// The key of its name in the catalogs.
    pub fn label_key(self) -> &'static str {
        match self {
            View::Lighting => "nav_lighting",
            View::Room => "nav_room",
            View::Bulbs => "nav_bulbs",
            View::Settings => "nav_settings",
        }
    }

    /// Its icon, by the name `assets/icons` gives it.
    pub fn icon(self) -> &'static str {
        match self {
            View::Lighting => "home",
            View::Room => "plan",
            View::Bulbs => "bulb",
            View::Settings => "sliders",
        }
    }
}

/// A message the window shows for a moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub error: bool,
    pub text: String,
}

pub struct Lueur {
    pub shell: Arc<Shell>,
    /// The config, without its credentials.
    pub config: AppConfig,
    pub status: EffectStatus,
    pub view: View,
    pub sleep: Option<SleepStatus>,
    /// The version an update installed, a restart away.
    pub update_ready: Option<String>,
    pub unverified: Vec<Unverified>,
    pub notice: Option<Notice>,
}

/// The app's one [`Lueur`].
pub struct Model(pub Entity<Lueur>);

impl Global for Model {}

/// The app's one [`Lueur`].
pub fn model(cx: &App) -> Entity<Lueur> {
    cx.global::<Model>().0.clone()
}

impl Lueur {
    pub fn new(shell: Arc<Shell>) -> Self {
        let config = service::get_config(&shell).unwrap_or_default();
        let status = service::effect_status(&shell);
        let sleep = service::sleep_timer(&shell);
        Lueur {
            update_ready: shell.update_ready(),
            shell,
            config,
            status,
            view: View::Lighting,
            sleep,
            unverified: Vec::new(),
            notice: None,
        }
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(self.config.interface.language)
    }

    /// Run a shell command off the window's thread, then `done` with its
    /// result on it. A failure is shown as a notice.
    pub fn run<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(&Arc<Shell>) -> AppResult<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let shell = Arc::clone(&self.shell);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { job(&shell) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(value) => done(this, value, cx),
                    Err(e) => this.fail(&e, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn fail(&mut self, e: &AppError, cx: &mut Context<Self>) {
        log::warn!("{e}");
        self.notice = Some(Notice {
            error: true,
            text: i18n::error_text(self.lang(), e),
        });
        cx.notify();
    }

    pub fn inform(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice = Some(Notice { error: false, text });
        cx.notify();
    }

    pub fn show(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            cx.notify();
        }
    }

    /// Ask the shell again what plays.
    pub fn refresh_status(&mut self, cx: &mut Context<Self>) {
        self.run(
            cx,
            |s| Ok(service::effect_status(s)),
            |this, status, _| this.status = status,
        );
    }

    /// Read the config again: the tray, a mood or the natural light changed
    /// what the lights were set to.
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        self.run(
            cx,
            |s| service::get_config(s),
            |this, config, cx| this.set_config(config, cx),
        );
    }

    /// Take a config the shell sent back, and follow its language and
    /// appearance.
    fn set_config(&mut self, config: AppConfig, cx: &mut Context<Self>) {
        let (lang, appearance) = (self.lang(), self.config.interface.appearance);
        self.config = config;
        if self.lang() != lang {
            apply_language(self.lang(), &self.shell, cx);
        }
        if self.config.interface.appearance != appearance {
            apply_theme(self.config.interface.appearance, cx);
        }
    }

    /// The ambilight starts, or every effect stops: the main button and
    /// Ctrl+Enter.
    pub fn toggle_effect(&mut self, cx: &mut Context<Self>) {
        if self.status.running {
            self.run(
                cx,
                |s| {
                    service::stop_effect(s, None)?;
                    Ok(service::effect_status(s))
                },
                |this, status, _| this.status = status,
            );
        } else {
            self.run(
                cx,
                |s| service::start_effect(s, None),
                |this, status, _| this.status = status,
            );
        }
    }

    /// Switch one light on or off.
    pub fn set_light_power(&mut self, light_id: String, on: bool, cx: &mut Context<Self>) {
        // Shown at once; the shell's copy is read again once the bulb has it.
        self.config.states.entry(light_id.clone()).or_default().on = on;
        cx.notify();
        self.run(
            cx,
            move |s| {
                service::set_light_power(s, &light_id, on)?;
                service::get_config(s)
            },
            |this, config, cx| this.set_config(config, cx),
        );
    }

    /// An event from the shell, on the window's thread.
    pub fn handle(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::EffectChanged(error) => {
                if let Some(e) = error {
                    self.fail(&e, cx);
                }
                self.refresh_status(cx);
            }
            Event::LightsChanged => self.reload_config(cx),
            Event::AutoStarted => {
                let text = i18n::text(self.lang(), "auto_started").to_string();
                self.inform(text, cx);
            }
            Event::DevicesUnverified(list) => self.unverified = list,
            Event::SleepChanged(status) => self.sleep = status,
            Event::UpdateReady(version) => self.update_ready = Some(version),
            // The app's, not the window's: see `app::handle`.
            Event::TrayStale { .. } | Event::FocusMain => {}
        }
        cx.notify();
    }
}

/// Speak `lang`, and tell the shell so the tray does too.
pub fn apply_language(lang: Lang, shell: &Arc<Shell>, cx: &mut App) {
    cx.set_global(lang);
    let shell = Arc::clone(shell);
    cx.background_spawn(async move {
        if let Err(e) = service::set_ui_language(&shell, lang.code()) {
            log::warn!("cannot tell the tray the language: {e}");
        }
    })
    .detach();
    cx.refresh_windows();
}

/// Show the appearance the settings force, or the system's.
pub fn apply_theme(appearance: lueur_core::config::Appearance, cx: &mut App) {
    let system: WindowAppearance = cx.window_appearance();
    cx.set_global(Theme::resolve(appearance, system));
    cx.refresh_windows();
}
