//! What the window shows, and the actions every page shares.
//!
//! One value, [`Lueur`], holds the window's copy of the config and of what
//! plays; the pages read it and call its actions. It lives on the window's
//! thread (`rt.rs`). An action runs the shell's command on a thread of its
//! own (bulbs and the network block), then brings the copy up to date on
//! the window's thread. The shell's events do the same for changes made
//! elsewhere: the tray, automatic mode, the sleep timer, a bulb that
//! dropped. What only one page needs lives in that page's part of
//! [`crate::pages::Locals`].

use crate::i18n::{self, Lang};
use crate::rt;
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

    /// Its place in the sidebar, as the window's `Nav.view` counts it.
    pub fn index(self) -> i32 {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0) as i32
    }

    pub fn from_index(i: i32) -> Option<View> {
        usize::try_from(i).ok().and_then(|i| View::ALL.get(i).copied())
    }
}

/// What a toast's button does.
pub type ToastAction = Rc<dyn Fn()>;

/// The page the window opens on: Lighting, or in a development build the
/// one `LUEUR_START_VIEW` names (lighting, room, bulbs, settings), to picture
/// a page with `scripts/dev/snap.ps1`.
fn start_view() -> View {
    let wanted = cfg!(debug_assertions)
        .then(|| std::env::var("LUEUR_START_VIEW").ok())
        .flatten();
    match wanted.as_deref() {
        Some("room") => View::Room,
        Some("bulbs") => View::Bulbs,
        Some("settings") => View::Settings,
        _ => View::Lighting,
    }
}

/// A message the window shows for a moment, or until closed for an error.
#[derive(Clone)]
pub struct Toast {
    pub id: u64,
    pub error: bool,
    pub text: String,
    /// The untranslated technical cause, shown on request.
    pub detail: Option<String>,
    pub action: Option<(String, ToastAction)>,
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
    toast_timers: Vec<(u64, slint::Timer)>,
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
    /// True while something on the room's plan is being dragged. The
    /// effects restart on a changed room only once the drag ends.
    pub arranging: bool,
    /// Whether the window is on screen: what plays is asked again, and the
    /// backdrop moves, only then.
    pub window_shown: bool,
    /// What only one page needs.
    pub pages: crate::pages::Locals,
    save: Option<slint::Timer>,
    _poll: slint::Timer,
}

impl Lueur {
    pub fn new(shell: Arc<Shell>) -> Self {
        let config = service::get_config(&shell).unwrap_or_default();
        let status = service::effect_status(&shell);
        let sleep = service::sleep_timer(&shell).map(|s| (s, Instant::now()));
        // What plays, asked again now and then: a bulb may drop without a
        // word. Only while the window shows it.
        let poll = slint::Timer::default();
        poll.start(slint::TimerMode::Repeated, POLL, || {
            if rt::ready() && rt::read(|l| l.window_shown) {
                rt::with(|l| l.refresh_status());
            }
        });
        Lueur {
            update_ready: shell.update_ready(),
            shell,
            config,
            status,
            view: start_view(),
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
            arranging: false,
            window_shown: false,
            pages: crate::pages::Locals::default(),
            save: None,
            _poll: poll,
        }
    }

    pub fn lang(&self) -> Lang {
        Lang::resolve(self.config.interface.language)
    }

    pub fn t(&self, key: &str) -> String {
        i18n::text(self.lang(), key)
    }

    /// Run a shell command off the window's thread, then `done` with its
    /// result on it. A failure is shown as an error toast.
    pub fn run<T: Send + 'static>(
        &mut self,
        job: impl FnOnce(&Arc<Shell>) -> AppResult<T> + Send + 'static,
        done: impl FnOnce(&mut Self, T) + 'static,
    ) {
        self.run_then(job, move |this, result| match result {
            Ok(value) => done(this, value),
            Err(e) => this.fail(&e),
        });
    }

    /// [`Lueur::run`], handing `done` the result whatever it is.
    pub fn run_then<T: Send + 'static>(
        &mut self,
        job: impl FnOnce(&Arc<Shell>) -> AppResult<T> + Send + 'static,
        done: impl FnOnce(&mut Self, AppResult<T>) + 'static,
    ) {
        let shell = Arc::clone(&self.shell);
        rt::spawn(
            move || job(&shell),
            move |result| rt::with(|this| done(this, result)),
        );
    }

    /// Change the config and save it a moment later, so a dragged slider
    /// saves once it rests. A running effect picks up what it can live.
    pub fn edit(&mut self, change: impl FnOnce(&mut AppConfig)) {
        let (lang, appearance) = (self.lang(), self.config.interface.appearance);
        change(&mut self.config);
        if self.lang() != lang {
            apply_language(self.lang(), &self.shell);
        }
        if self.config.interface.appearance != appearance {
            apply_theme(self.config.interface.appearance);
        }
        // Replacing the timer drops, and so cancels, the save it supersedes.
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::SingleShot, SAVE_DELAY, || {
            rt::with(|this| this.save_now());
        });
        self.save = Some(timer);
    }

    /// Write the config now.
    pub fn save_now(&mut self) {
        self.save = None;
        let config = self.config.clone();
        self.run_then(
            move |s| service::save_config(s, config),
            |this, result| {
                if let Err(e) = result {
                    this.fail(&e);
                }
            },
        );
    }

    /// Show an error from the shell.
    pub fn fail(&mut self, e: &AppError) {
        log::warn!("{e}");
        let text = i18n::error_text(self.lang(), e);
        self.toast(true, text, e.detail.clone(), None);
    }

    /// Show a piece of information for a few seconds.
    pub fn inform(&mut self, text: impl Into<String>) {
        self.toast(false, text.into(), None, None);
    }

    /// [`Lueur::inform`] with a button, "Undo" say: it stays a while longer.
    pub fn inform_with(&mut self, text: impl Into<String>, label: String, action: ToastAction) {
        self.toast(false, text.into(), None, Some((label, action)));
    }

    /// Errors stay until closed; information goes by itself. The same
    /// message again counts up on the toast showing. Past the limit the
    /// oldest information goes first; information never pushes out an error.
    fn toast(
        &mut self,
        error: bool,
        text: String,
        detail: Option<String>,
        action: Option<(String, ToastAction)>,
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
                self.count_down(id, life);
            }
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
                Some(id) => self.dismiss(id),
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
            self.count_down(id, life);
        }
    }

    fn count_down(&mut self, id: u64, life: Duration) {
        self.toast_timers.retain(|(t, _)| *t != id);
        let timer = slint::Timer::default();
        timer.start(slint::TimerMode::SingleShot, life, move || {
            rt::with(|this| this.dismiss(id));
        });
        self.toast_timers.push((id, timer));
    }

    /// Hold a toast while it is pointed at, or let it go on with a moment
    /// left to finish reading.
    pub fn hold_toast(&mut self, id: u64, held: bool) {
        let Some(toast) = self.toasts.iter().find(|t| t.id == id) else {
            return;
        };
        if toast.error {
            return;
        }
        if held {
            self.toast_timers.retain(|(t, _)| *t != id);
        } else {
            self.count_down(id, Duration::from_secs(2));
        }
    }

    pub fn dismiss(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
        self.toast_timers.retain(|(t, _)| *t != id);
    }

    pub fn show(&mut self, view: View) {
        self.view = view;
    }

    /// Ask the shell again what plays.
    pub fn refresh_status(&mut self) {
        self.run(
            |s| Ok(service::effect_status(s)),
            |this, status| this.status = status,
        );
    }

    /// Read the config again: the tray, a mood or the natural light changed
    /// what the lights were set to. An edit not saved yet stays.
    pub fn reload_config(&mut self) {
        self.run(
            |s| service::get_config(s),
            |this, config| {
                if this.save.is_none() {
                    this.set_config(config);
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
    pub fn set_config(&mut self, config: AppConfig) {
        let (lang, appearance) = (self.lang(), self.config.interface.appearance);
        self.config = config;
        if self.lang() != lang {
            apply_language(self.lang(), &self.shell);
        }
        if self.config.interface.appearance != appearance {
            apply_theme(self.config.interface.appearance);
        }
    }

    /// The ambilight starts, or every effect stops: the tray's first item
    /// and Ctrl+Enter.
    pub fn toggle_effect(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        let running = self.status.running;
        self.run_then(
            move |s| {
                if running {
                    service::stop_effect(s, None)?;
                    Ok(service::effect_status(s))
                } else {
                    service::start_effect(s, None)
                }
            },
            |this, result| {
                this.busy = false;
                match result {
                    Ok(status) => this.status = status,
                    Err(e) => this.fail(&e),
                }
                this.reload_config();
            },
        );
    }

    /// Switch one light on or off.
    pub fn set_light_power(&mut self, light_id: String, on: bool) {
        if self.power_busy.contains(&light_id) {
            return;
        }
        // Shown at once; the shell's copy is read again once the bulb has it.
        self.config.states.entry(light_id.clone()).or_default().on = on;
        self.power_busy.push(light_id.clone());
        let id = light_id.clone();
        self.run_then(
            move |s| {
                service::set_light_power(s, &id, on)?;
                service::get_config(s)
            },
            move |this, result| {
                this.power_busy.retain(|l| *l != light_id);
                match result {
                    Ok(config) => this.set_config(config),
                    Err(e) => {
                        this.fail(&e);
                        this.reload_config();
                    }
                }
            },
        );
    }

    /// An event from the shell, on the window's thread.
    pub fn handle(&mut self, event: Event) {
        match event {
            Event::EffectChanged(error) => {
                if let Some(e) = error {
                    self.fail(&e);
                }
                self.refresh_status();
            }
            Event::LightsChanged => self.reload_config(),
            Event::AutoStarted => {
                let text = self.t("auto_started");
                self.inform(text);
            }
            Event::DevicesUnverified(list) => self.unverified = list,
            Event::SleepChanged(status) => self.sleep = status.map(|s| (s, Instant::now())),
            Event::UpdateReady(version) => self.update_ready = Some(version),
            // The app's, not the window's: see `app::handle`.
            Event::TrayStale { .. } | Event::FocusMain => {}
        }
    }
}

/// Speak `lang`, and tell the shell so the tray does too.
pub fn apply_language(lang: Lang, shell: &Arc<Shell>) {
    if let Some(ui) = rt::ui() {
        i18n::set_current(lang, &ui);
    }
    let shell = Arc::clone(shell);
    std::thread::spawn(move || {
        if let Err(e) = service::set_ui_language(&shell, lang.code()) {
            log::warn!("cannot tell the tray the language: {e}");
        }
    });
}

/// Show the appearance the settings force, or the system's.
pub fn apply_theme(appearance: lueur_core::config::Appearance) {
    use slint::ComponentHandle;
    if let Some(ui) = rt::ui() {
        crate::theme::Theme::resolve(appearance, crate::theme::system_dark())
            .apply(&ui.global::<crate::Theme>());
    }
}
