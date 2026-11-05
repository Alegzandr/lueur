//! Updates, the way Discord does them, over `lueur_shell::update`.
//!
//! Every launch of a release build opens a small splash window that checks
//! for a new version before the app starts. A new version is downloaded, its
//! signature verified, installed in place, and the app restarts into it.
//! Without one (or without network) the splash closes and the app starts.
//! While the app runs, it checks again every hour; a version found then is
//! installed right away, and the window and the tray offer a restart.
//! Development builds never replace themselves and start right away.
//!
//! What the splash shows is what the Tauri versions' splash showed.

use crate::app::{self, FONT, Started};
use crate::i18n::{Lang, text};
use crate::theme::{Theme, size};
use gpui::{
    App, AsyncApp, Bounds, Context, FontWeight, IntoElement, QuitMode, Render, Role, SharedString,
    Window, WindowBounds, WindowDecorations, WindowHandle, WindowKind, WindowOptions, div, img,
    prelude::*, px, relative,
};
use lueur_shell::Shell;
use lueur_shell::update::{self, Failure, Progress, Stage, Updater};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Keeps the splash from flashing when the check is instant.
const SPLASH_MIN: Duration = Duration::from_millis(700);

/// Development builds never update. A debug build started with
/// `LUEUR_SPLASH_DEMO` set shows the splash going through its stages over a
/// pretend update, to look at it; it downloads and installs nothing.
pub fn enabled() -> bool {
    !cfg!(debug_assertions) || demo()
}

fn demo() -> bool {
    cfg!(debug_assertions) && std::env::var_os("LUEUR_SPLASH_DEMO").is_some()
}

/// What the check thread reports.
enum Report {
    Progress(Progress),
    Done(Result<Option<String>, Failure>),
}

/// Shows the splash, applies a pending release, then starts the app with
/// [`app::run`]. In a development build, starts it right away.
pub fn launch(started: Started, cx: &mut App) {
    app::load_fonts(cx);
    app::follow_reduced_motion(cx);
    // An AppImage runs from a folder that goes away with it: a restart must
    // start the AppImage itself, where the update was installed.
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        cx.set_restart_path(appimage.into());
    }
    if !enabled() {
        app::run(started, cx);
        return;
    }
    // The splash closes before the main window opens, and the app may start
    // in the tray: no window for a moment must not end it.
    cx.set_quit_mode(QuitMode::Explicit);
    let updater = Arc::new(Updater::new(started.shell.data_dir.clone()));
    let splash = open_splash(&started.shell, cx);
    let reports = check_in_background(Arc::clone(&updater), true);

    cx.spawn(async move |cx| {
        let shown = Instant::now();
        let mut result = Err(Failure::Fetch("the update check stopped".into()));
        while let Ok(report) = reports.recv().await {
            match report {
                Report::Progress(p) => show(splash, p, cx),
                Report::Done(r) => {
                    result = r;
                    break;
                }
            }
        }
        let wait = match result {
            Ok(Some(_)) => {
                show(splash, stage(Stage::Restarting), cx);
                cx.update(|cx| {
                    // Nothing has started yet but the workers: hand the
                    // network back and let the next process take over.
                    started.shell.quit();
                    started.instance.release();
                    cx.restart();
                });
                return;
            }
            Ok(None) => update::CHECK_EVERY,
            Err(e) => e.wait(),
        };
        if let Some(rest) = SPLASH_MIN.checked_sub(shown.elapsed()) {
            cx.background_executor().timer(rest).await;
        }
        let shell = Arc::clone(&started.shell);
        cx.update(|cx| {
            // Closed first: the app brings up the first window it finds.
            if let Some(splash) = splash {
                let _ = splash.update(cx, |_, window, _| window.remove_window());
            }
            app::run(started, cx);
        });
        watch(updater, shell, wait, cx).await;
    })
    .detach();
}

/// Checks again while the app runs; a found version waits for a restart.
/// The wait is a timer, and each check a thread of its own while it runs,
/// so an app that mostly sits in the tray holds no thread for the hour.
async fn watch(updater: Arc<Updater>, shell: Arc<Shell>, mut wait: Duration, cx: &mut AsyncApp) {
    loop {
        cx.background_executor().timer(wait).await;
        let reports = check_in_background(Arc::clone(&updater), false);
        let mut result = Err(Failure::Fetch("the update check stopped".into()));
        while let Ok(report) = reports.recv().await {
            if let Report::Done(r) = report {
                result = r;
            }
        }
        wait = match result {
            Ok(Some(version)) => {
                // Tells the window, which offers a restart, and the tray.
                shell.set_update_ready(version);
                // Installed: nothing more to do until the next restart.
                return;
            }
            Ok(None) => update::CHECK_EVERY,
            Err(e) => e.wait(),
        };
    }
}

/// Runs one check, and the download and install it leads to, on a thread of
/// its own: it blocks on the network and the disk for as long as they take.
fn check_in_background(updater: Arc<Updater>, splash: bool) -> async_channel::Receiver<Report> {
    let (tx, rx) = async_channel::unbounded();
    std::thread::spawn(move || {
        let progress = |p| {
            let _ = tx.send_blocking(Report::Progress(p));
        };
        let result = if demo() {
            pretend(splash, progress)
        } else {
            updater.check_and_install(splash, progress)
        };
        let _ = tx.send_blocking(Report::Done(result));
    });
    rx
}

/// The demo's pretend update: each stage in turn, then no update found, so
/// the app starts as usual.
fn pretend(splash: bool, mut progress: impl FnMut(Progress)) -> Result<Option<String>, Failure> {
    if !splash {
        return Ok(None);
    }
    let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
    pause(1500);
    for percent in 0..=100u8 {
        progress(Progress {
            stage: Stage::Downloading,
            percent: Some(percent),
        });
        pause(30);
    }
    progress(stage(Stage::Installing));
    pause(1200);
    Ok(None)
}

fn stage(stage: Stage) -> Progress {
    Progress {
        stage,
        percent: None,
    }
}

fn show(splash: Option<WindowHandle<Splash>>, progress: Progress, cx: &mut AsyncApp) {
    if let Some(splash) = splash {
        let _ = splash.update(cx, |s, _, cx| {
            s.progress = progress;
            cx.notify();
        });
    }
}

/// The splash follows the language and appearance the settings force, or
/// the system's, as the main window does.
fn open_splash(shell: &Shell, cx: &mut App) -> Option<WindowHandle<Splash>> {
    let interface = shell.lock_config().map(|c| c.interface).unwrap_or_default();
    let lang = Lang::resolve(interface.language);
    let theme = Theme::resolve(interface.appearance, cx.window_appearance());
    let bounds = Bounds::centered(None, gpui::size(px(300.0), px(110.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // No title bar nor frame, as the Tauri splash.
        titlebar: None,
        window_decorations: Some(WindowDecorations::Client),
        // Shown without taking the focus from what the user is doing, and
        // never kept above other windows.
        focus: false,
        show: true,
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: false,
        app_id: Some("com.lueur.app".into()),
        icon: app::window_icon(),
        ..Default::default()
    };
    let opened = cx.open_window(options, |_, cx| {
        cx.new(|_| Splash {
            lang,
            theme,
            progress: Progress::default(),
        })
    });
    match opened {
        Ok(handle) => Some(handle),
        Err(e) => {
            // The check goes on without anything to show it.
            log::error!("cannot open the splash: {e}");
            None
        }
    }
}

/// The app's name, what is happening, and a thin meter while a download
/// runs. Lit like the rest of Lueur: amber means light.
pub struct Splash {
    lang: Lang,
    theme: Theme,
    progress: Progress,
}

impl Splash {
    fn text(&self) -> SharedString {
        let t = |key| text(self.lang, key);
        match (self.progress.stage, self.progress.percent) {
            (Stage::Downloading, Some(percent)) => crate::i18n::fill(
                &t("splash_downloading_percent"),
                &[("percent", &percent.to_string())],
            )
            .into(),
            (Stage::Checking, _) => t("splash_checking"),
            (Stage::Downloading, None) => t("splash_downloading"),
            (Stage::Installing, _) => t("splash_installing"),
            (Stage::Restarting, _) => t("splash_restarting"),
        }
    }
}

impl Render for Splash {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let percent = match self.progress {
            Progress {
                stage: Stage::Downloading,
                percent: Some(p),
            } => Some(p),
            _ => None,
        };
        let stage = self.text();
        div()
            .id("splash")
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(size::S2))
            .px(px(size::S5))
            .bg(theme.bg)
            .font_family(FONT)
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::S2))
                    .child(img("logo-mark.svg").size(px(28.0)))
                    .child(
                        div()
                            .text_size(px(size::T_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Lueur"),
                    ),
            )
            .child(
                div()
                    .id("stage")
                    .role(Role::Status)
                    .aria_label(stage.clone())
                    .text_size(px(size::T_SM))
                    .text_color(theme.text_2)
                    .child(stage),
            )
            .when_some(percent, |d, percent| {
                d.child(
                    div()
                        .id("meter")
                        .role(Role::ProgressIndicator)
                        .aria_label(text(self.lang, "splash_download"))
                        .aria_min_numeric_value(0.0)
                        .aria_max_numeric_value(100.0)
                        .aria_numeric_value(percent as f64)
                        .h(px(3.0))
                        .w_full()
                        .rounded_full()
                        .bg(theme.line)
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(relative(percent as f32 / 100.0))
                                .bg(theme.glow),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stage reads as a sentence in both languages, the percent
    /// filled in.
    #[test]
    fn every_stage_has_its_words() {
        for lang in [Lang::En, Lang::Fr] {
            let mut splash = Splash {
                lang,
                theme: Theme::light(),
                progress: Progress::default(),
            };
            for (stage, percent) in [
                (Stage::Checking, None),
                (Stage::Downloading, None),
                (Stage::Downloading, Some(42)),
                (Stage::Installing, None),
                (Stage::Restarting, None),
            ] {
                splash.progress = Progress { stage, percent };
                let words = splash.text();
                assert!(
                    !words.contains("splash_") && !words.contains('{'),
                    "{words}"
                );
                assert_eq!(words.contains("42"), percent.is_some(), "{words}");
            }
        }
    }
}
