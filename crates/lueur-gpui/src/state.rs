//! What the window shows, and the actions every page shares, after
//! `src/lib/state/`.
//!
//! One entity, [`Lueur`], holds the window's copy of the config and of what
//! plays; the pages read it and call its actions. An action runs the shell's
//! command on the background executor (bulbs and the network block), then
//! brings the copy up to date on the window's thread. The shell's events do
//! the same for changes made elsewhere: the tray, automatic mode, the sleep
//! timer, a bulb that dropped. What only one page needs lives in that page.

use crate::i18n::{self, Lang};
use crate::theme::Theme;
use gpui::{
    App, AppContext, Context, Entity, Global, SharedString, Task, Window, WindowAppearance,
};
use lueur_core::config::AppConfig;
use lueur_shell::error::{AppError, AppResult};
use lueur_shell::identity::Unverified;
use lueur_shell::service::{self, EffectStatus};
use lueur_shell::sleep::Status as SleepStatus;
use lueur_shell::{Event, Shell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// What a toast's button does.
pub type ToastAction = Rc<dyn Fn(&mut Window, &mut App)>;

/// A message the window shows for a moment, or until closed for an error.
#[derive(Clone)]
pub struct Toast {
    pub id: u64,
    pub error: bool,
    pub text: SharedString,
    /// The untranslated technical cause, shown on request.
    pub detail: Option<SharedString>,
    pub action: Option<(SharedString, ToastAction)>,
    /// How many times the same message came, when more than once.
    pub count: usize,
}

/// At most this many toasts show at once.
const MAX_TOASTS: usize = 4;
/// How long a slider waits before the config is written, so a drag does
/// not hammer the disk.
const SAVE_DELAY: Duration = Duration::from_millis(250);
/// How often what plays is asked again while the window is open.
const POLL: Duration = Duration::from_secs(4);

pub struct Lueur {
    pub shell: Arc<Shell>,
    /// The config, without its credentials.
    pub config: AppConfig,
    pub status: EffectStatus,
    pub view: View,
    /// The sleep timer, and when it was last heard of (to count down).
    pub sleep: Option<(SleepStatus, Instant)>,
    /// The version an update installed, a restart away.
    pub update_ready: Option<String>,
    /// Paired devices the last search could not vouch for.
    pub unverified: Vec<Unverified>,
    /// Unverified devices answered "not mine" this session.
    pub dismissed_devices: Vec<String>,
    pub toasts: Vec<Toast>,
    toast_seq: u64,
    toast_timers: Vec<(u64, Task<()>)>,
    /// True while the effect is being started or stopped.
    pub busy: bool,
    /// The mood or mode being switched to, while it starts.
    pub pending: Option<String>,
    /// The lights effects and moods go to; none picked means the whole room.
    pub target: Vec<String>,
    /// The light selected on the plan, shared so other pages can deep-link.
    pub selected_light: Option<String>,
    /// Lights whose power is being switched.
    pub power_busy: Vec<String>,
    save: Option<Task<()>>,
    _poll: Option<Task<()>>,
}

/// The app's one [`Lueur`].
pub struct Model(pub Entity<Lueur>);

impl Global for Model {}

/// The app's one [`Lueur`].
pub fn model(cx: &App) -> Entity<Lueur> {
    cx.global::<Model>().0.clone()
}

impl Lueur {
    pub fn new(shell: Arc<Shell>, cx: &mut Context<Self>) -> Self {
        let config = service::get_config(&shell).unwrap_or_default();
        let status = service::effect_status(&shell);
        let sleep = service::sleep_timer(&shell).map(|s| (s, Instant::now()));
        // What plays, asked again now and then: a bulb may drop without a
        // word. Only while a window shows it.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let alive = this.update(cx, |this, cx| {
                    if !cx.windows().is_empty() {
                        this.refresh_status(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        Lueur {
            update_ready: shell.update_ready(),
            shell,
            config,
            status,
            view: View::Lighting,
            sleep,
            unverified: Vec::new(),
            dismissed_devices: Vec::new(),
            toasts: Vec::new(),
            toast_seq: 0,
            toast_timers: Vec::new(),
            busy: false,
            pending: None,
            target: Vec::new(),
            selected_light: None,
            power_busy: Vec::new(),
            save: None,
            _poll: Some(poll),
        }
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(self.config.interface.language)
    }

    pub fn t(&self, key: &str) -> SharedString {
        i18n::text(self.lang(), key)
    }

    /// Run a shell command off the window's thread, then `done` with its
    /// result on it. A failure is shown as an error toast.
    pub fn run<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(&Arc<Shell>) -> AppResult<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        self.run_then(cx, job, move |this, result, cx| match result {
            Ok(value) => done(this, value, cx),
            Err(e) => this.fail(&e, cx),
        });
    }

    /// [`Lueur::run`], handing `done` the result whatever it is.
    pub fn run_then<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(&Arc<Shell>) -> AppResult<T> + Send + 'static,
        done: impl FnOnce(&mut Self, AppResult<T>, &mut Context<Self>) + 'static,
    ) {
        let shell = Arc::clone(&self.shell);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { job(&shell) })
                .await;
            let _ = this.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Change the config and save it a moment later, so a dragged slider
    /// saves once it rests. A running effect picks up what it can live.
    pub fn edit(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut AppConfig)) {
        let (lang, appearance) = (self.lang(), self.config.interface.appearance);
        change(&mut self.config);
        if self.lang() != lang {
            apply_language(self.lang(), &self.shell, cx);
        }
        if self.config.interface.appearance != appearance {
            apply_theme(self.config.interface.appearance, cx);
        }
        cx.notify();
        // Replacing the task drops, and so cancels, the save it supersedes.
        self.save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |this, cx| this.save_now(cx));
        }));
    }

    /// Write the config now.
    pub fn save_now(&mut self, cx: &mut Context<Self>) {
        self.save = None;
        let config = self.config.clone();
        self.run_then(
            cx,
            move |s| service::save_config(s, config),
            |this, result, cx| {
                if let Err(e) = result {
                    this.fail(&e, cx);
                }
            },
        );
    }

    /// Show an error from the shell.
    pub fn fail(&mut self, e: &AppError, cx: &mut Context<Self>) {
        log::warn!("{e}");
        let text = i18n::error_text(self.lang(), e).into();
        self.toast(true, text, e.detail.clone().map(Into::into), None, cx);
    }

    /// Show a piece of information for a few seconds.
    pub fn inform(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast(false, text.into(), None, None, cx);
    }

    /// [`Lueur::inform`] with a button, "Undo" say: it stays a while longer.
    pub fn inform_with(
        &mut self,
        text: impl Into<SharedString>,
        label: SharedString,
        action: ToastAction,
        cx: &mut Context<Self>,
    ) {
        self.toast(false, text.into(), None, Some((label, action)), cx);
    }

    /// Errors stay until closed; information goes by itself. The same
    /// message again counts up on the toast showing. Past the limit the
    /// oldest information goes first; information never pushes out an error.
    fn toast(
        &mut self,
        error: bool,
        text: SharedString,
        detail: Option<SharedString>,
        action: Option<(SharedString, ToastAction)>,
        cx: &mut Context<Self>,
    ) {
        let life = Duration::from_millis(if action.is_some() { 7000 } else { 3600 });
        if action.is_none()
            && let Some(same) = self.toasts.iter_mut().find(|t| {
                t.action.is_none() && t.error == error && t.text == text && t.detail == detail
            })
        {
            same.count += 1;
            let id = same.id;
            if !error {
                self.count_down(id, life, cx);
            }
            cx.notify();
            return;
        }
        while self.toasts.len() >= MAX_TOASTS {
            let oldest = self
                .toasts
                .iter()
                .find(|t| !t.error)
                .or(if error { self.toasts.first() } else { None })
                .map(|t| t.id);
            match oldest {
                Some(id) => self.dismiss(id, cx),
                None => break,
            }
        }
        self.toast_seq += 1;
        let id = self.toast_seq;
        self.toasts.push(Toast {
            id,
            error,
            text,
            detail,
            action,
            count: 1,
        });
        if !error {
            self.count_down(id, life, cx);
        }
        cx.notify();
    }

    fn count_down(&mut self, id: u64, life: Duration, cx: &mut Context<Self>) {
        self.toast_timers.retain(|(t, _)| *t != id);
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(life).await;
            let _ = this.update(cx, |this, cx| this.dismiss(id, cx));
        });
        self.toast_timers.push((id, task));
    }

    /// Hold a toast while it is pointed at, or let it go on with a moment
    /// left to finish reading.
    pub fn hold_toast(&mut self, id: u64, held: bool, cx: &mut Context<Self>) {
        let Some(toast) = self.toasts.iter().find(|t| t.id == id) else {
            return;
        };
        if toast.error {
            return;
        }
        if held {
            self.toast_timers.retain(|(t, _)| *t != id);
        } else {
            self.count_down(id, Duration::from_secs(2), cx);
        }
    }

    pub fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|t| t.id != id);
        self.toast_timers.retain(|(t, _)| *t != id);
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
    /// what the lights were set to. An edit not saved yet stays.
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        self.run(
            cx,
            |s| service::get_config(s),
            |this, config, cx| {
                if this.save.is_none() {
                    this.set_config(config, cx);
                } else {
                    this.config.states = config.states;
                    this.config.natural = config.natural;
                    this.config.modes = config.modes;
                    this.config.pairings = config.pairings;
                }
            },
        );
    }

    /// Take a config the shell sent back, and follow its language and
    /// appearance.
    pub fn set_config(&mut self, config: AppConfig, cx: &mut Context<Self>) {
        let (lang, appearance) = (self.lang(), self.config.interface.appearance);
        self.config = config;
        if self.lang() != lang {
            apply_language(self.lang(), &self.shell, cx);
        }
        if self.config.interface.appearance != appearance {
            apply_theme(self.config.interface.appearance, cx);
        }
        cx.notify();
    }

    /// The ambilight starts, or every effect stops: the tray's first item
    /// and Ctrl+Enter.
    pub fn toggle_effect(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let running = self.status.running;
        self.run_then(
            cx,
            move |s| {
                if running {
                    service::stop_effect(s, None)?;
                    Ok(service::effect_status(s))
                } else {
                    service::start_effect(s, None)
                }
            },
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(status) => this.status = status,
                    Err(e) => this.fail(&e, cx),
                }
                this.reload_config(cx);
            },
        );
    }

    /// Switch one light on or off.
    pub fn set_light_power(&mut self, light_id: String, on: bool, cx: &mut Context<Self>) {
        if self.power_busy.contains(&light_id) {
            return;
        }
        // Shown at once; the shell's copy is read again once the bulb has it.
        self.config.states.entry(light_id.clone()).or_default().on = on;
        self.power_busy.push(light_id.clone());
        cx.notify();
        let id = light_id.clone();
        self.run_then(
            cx,
            move |s| {
                service::set_light_power(s, &id, on)?;
                service::get_config(s)
            },
            move |this, result, cx| {
                this.power_busy.retain(|l| *l != light_id);
                match result {
                    Ok(config) => this.set_config(config, cx),
                    Err(e) => {
                        this.fail(&e, cx);
                        this.reload_config(cx);
                    }
                }
            },
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
                let text = self.t("auto_started");
                self.inform(text, cx);
            }
            Event::DevicesUnverified(list) => self.unverified = list,
            Event::SleepChanged(status) => self.sleep = status.map(|s| (s, Instant::now())),
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
