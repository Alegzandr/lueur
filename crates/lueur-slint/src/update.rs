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

use crate::app::{self, Started};
use crate::i18n::{Lang, fill, text};
use crate::theme::Theme;
use crate::{SplashState, SplashWindow};
use lueur_shell::Shell;
use lueur_shell::update::{self, Failure, Progress, Stage, Updater};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
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
/// [`app::run`]. In a development build, starts it right away. Runs on the
/// window's thread, inside the event loop.
pub fn launch(started: Started) {
    if !enabled() {
        app::run(started);
        return;
    }
    let updater = Arc::new(Updater::new(started.shell.data_dir.clone()));
    let splash = open_splash(&started.shell);
    let shown = Instant::now();
    let weak = splash.as_ref().map(|s| s.as_weak());
    let lang = splash_lang(&started.shell);
    check_in_background(
        Arc::clone(&updater),
        true,
        move |progress| {
            if let Some(splash) = weak.as_ref().and_then(|w| w.upgrade()) {
                show(&splash, lang, progress);
            }
        },
        move |result| {
            let wait = match result {
                Ok(Some(_)) => {
                    if let Some(splash) = &splash {
                        show(splash, lang, stage(Stage::Restarting));
                    }
                    // Nothing has started yet but the workers: hand the
                    // network back and let the next process take over.
                    started.shell.quit();
                    started.instance.release();
                    app::relaunch();
                    let _ = slint::quit_event_loop();
                    return;
                }
                Ok(None) => update::CHECK_EVERY,
                Err(e) => e.wait(),
            };
            let rest = SPLASH_MIN.saturating_sub(shown.elapsed());
            slint::Timer::single_shot(rest, move || {
                if let Some(splash) = &splash {
                    let _ = splash.hide();
                }
                let shell = Arc::clone(&started.shell);
                app::run(started);
                watch(updater, shell, wait);
            });
        },
    );
}

/// Checks again while the app runs; a found version waits for a restart.
/// The wait is a timer, and each check a thread of its own while it runs,
/// so an app that mostly sits in the tray holds no thread for the hour.
fn watch(updater: Arc<Updater>, shell: Arc<Shell>, wait: Duration) {
    slint::Timer::single_shot(wait, move || {
        let again = Arc::clone(&updater);
        check_in_background(
            updater,
            false,
            |_| {},
            move |result| {
                let wait = match result {
                    Ok(Some(version)) => {
                        // Tells the window, which offers a restart, and the
                        // tray. Installed: nothing more to do until the next
                        // restart.
                        shell.set_update_ready(version);
                        return;
                    }
                    Ok(None) => update::CHECK_EVERY,
                    Err(e) => e.wait(),
                };
                watch(again, shell, wait);
            },
        );
    });
}

/// Runs one check, and the download and install it leads to, on a thread of
/// its own: it blocks on the network and the disk for as long as they take.
/// `progress` and `done` are called on the window's thread.
fn check_in_background(
    updater: Arc<Updater>,
    splash: bool,
    progress: impl Fn(Progress) + 'static,
    done: impl FnOnce(Result<Option<String>, Failure>) + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Report>();
    std::thread::spawn(move || {
        let report = |p| {
            let _ = tx.send(Report::Progress(p));
        };
        let result = if demo() {
            pretend(splash, report)
        } else {
            updater.check_and_install(splash, report)
        };
        let _ = tx.send(Report::Done(result));
    });
    // The reports are read on the window's thread, a few times a second.
    // The timer is kept in a slot of its own, emptied once the check is
    // done.
    let slot: Rc<RefCell<Option<slint::Timer>>> = Rc::default();
    let held = Rc::clone(&slot);
    let done = RefCell::new(Some(done));
    let finish = move |result| {
        let held = Rc::clone(&held);
        // Dropped after its own call is over.
        slint::Timer::single_shot(Duration::ZERO, move || drop(held.borrow_mut().take()));
        if let Some(done) = done.borrow_mut().take() {
            done(result);
        }
    };
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(30),
        move || {
            loop {
                match rx.try_recv() {
                    Ok(Report::Progress(p)) => progress(p),
                    Ok(Report::Done(result)) => return finish(result),
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => {
                        return finish(Err(Failure::Fetch("the update check stopped".into())));
                    }
                }
            }
        },
    );
    *slot.borrow_mut() = Some(timer);
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

/// The splash speaks the language the settings force, or the system's, as
/// the main window does.
fn splash_lang(shell: &Shell) -> Lang {
    let interface = shell.lock_config().map(|c| c.interface).unwrap_or_default();
    Lang::resolve(interface.language)
}

fn show(splash: &SplashWindow, lang: Lang, progress: Progress) {
    let state = splash.global::<SplashState>();
    state.set_stage(words(lang, progress).into());
    state.set_percent(match progress {
        Progress {
            stage: Stage::Downloading,
            percent: Some(p),
        } => i32::from(p),
        _ => -1,
    });
}

/// The splash follows the appearance the settings force, or the system's.
fn open_splash(shell: &Shell) -> Option<SplashWindow> {
    let interface = shell.lock_config().map(|c| c.interface).unwrap_or_default();
    let lang = Lang::resolve(interface.language);
    let splash = match SplashWindow::new() {
        Ok(splash) => splash,
        Err(e) => {
            // The check goes on without anything to show it.
            log::error!("cannot open the splash: {e}");
            return None;
        }
    };
    let dark = splash.global::<crate::Palette>().get_color_scheme()
        == slint::language::ColorScheme::Dark;
    Theme::resolve(interface.appearance, dark).apply(&splash.global::<crate::Theme>());
    splash
        .global::<SplashState>()
        .set_meter_label(text(lang, "splash_download").into());
    show(&splash, lang, Progress::default());
    if let Err(e) = splash.show() {
        log::error!("cannot show the splash: {e}");
    }
    Some(splash)
}

/// What the splash says for a stage of the update.
fn words(lang: Lang, progress: Progress) -> String {
    let t = |key| text(lang, key);
    match (progress.stage, progress.percent) {
        (Stage::Downloading, Some(percent)) => fill(
            &t("splash_downloading_percent"),
            &[("percent", &percent.to_string())],
        ),
        (Stage::Checking, _) => t("splash_checking"),
        (Stage::Downloading, None) => t("splash_downloading"),
        (Stage::Installing, _) => t("splash_installing"),
        (Stage::Restarting, _) => t("splash_restarting"),
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
            for (stage, percent) in [
                (Stage::Checking, None),
                (Stage::Downloading, None),
                (Stage::Downloading, Some(42)),
                (Stage::Installing, None),
                (Stage::Restarting, None),
            ] {
                let words = words(lang, Progress { stage, percent });
                assert!(
                    !words.contains("splash_") && !words.contains('{'),
                    "{words}"
                );
                assert_eq!(words.contains("42"), percent.is_some(), "{words}");
            }
        }
    }
}
