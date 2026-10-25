//! Updates from the latest GitHub release, the way Discord does them.
//!
//! Every launch opens a small splash window that checks for a new version
//! before anything else starts. A new version is downloaded, its minisign
//! signature verified by the official plugin, installed in place, and the app
//! restarts into it. Without one (or without network) the splash closes and
//! the app starts. While the app runs, it checks again every hour; a version
//! found then is installed right away and the window offers a restart in its
//! title bar. On Windows the installed `.exe` is swapped in place instead of
//! running an installer. AppImage and `.app` bundles use the plugin's own
//! installer.
//!
//! A version that downloads but fails to install (no write access to the
//! install folder, say) is remembered, also on disk: later launches start
//! without downloading it again, and the running app tries it again only
//! after a few hours, not after the short network retry.
//!
//! Never an older version, nor the same one again. `latest.json` itself is
//! not signed, only each update file is, so the version it announces proves
//! nothing on its own: a doctored manifest could pair a high number with an
//! old, validly signed file. Each signature therefore carries the version it
//! was made for in its trusted comment (the release signs with
//! `--app-version`), and `requireSignedVersion` in `tauri.conf.json` makes
//! the plugin refuse a file whose signed version is not the announced one.
//! The announced version must be strictly newer than this one
//! ([`is_upgrade`]), so the signed one is too.
//!
//! The only network traffic this adds is HTTPS to github.com.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lueur_core::config::{Appearance, Language};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::state::AppState;

pub const SPLASH: &str = "splash";
/// Event the frontend listens to, with the installed version as payload.
pub const UPDATE_READY: &str = "update-ready";
/// Often enough that a running app offers a new version the day it is out;
/// each check is one small HTTPS request.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// A version that downloads but does not install is tens of megabytes:
/// not every hour.
const INSTALL_RETRY: Duration = Duration::from_secs(6 * 60 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
/// No network at login must not hold the app for long.
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Keeps the splash from flashing when the check is instant.
const SPLASH_MIN: Duration = Duration::from_millis(700);
/// Holds the version that last failed to install, next to the settings.
const FAILED_FILE: &str = "update-failed.txt";

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    #[default]
    Checking,
    Downloading,
    Installing,
    Restarting,
}

/// What the splash window shows, polled by `src/splash/main.ts`.
#[derive(Clone, Copy, Default, Serialize)]
pub struct Progress {
    stage: Stage,
    /// Download progress, when the size is known.
    percent: Option<u8>,
}

#[derive(Default)]
pub struct UpdateState {
    progress: Mutex<Progress>,
    /// The version that failed to install in this process.
    failed: Mutex<Option<String>>,
    /// Version already installed and waiting for a restart.
    ready: Mutex<Option<String>>,
}

#[tauri::command]
pub fn update_progress(state: State<'_, UpdateState>) -> Progress {
    state.progress.lock().map(|p| *p).unwrap_or_default()
}

/// The version installed while the app was running, if any.
#[tauri::command]
pub fn update_ready(state: State<'_, UpdateState>) -> Option<String> {
    state.ready.lock().ok().and_then(|r| r.clone())
}

/// [`update_ready`], for the tray.
pub fn ready(app: &AppHandle) -> Option<String> {
    app.try_state::<UpdateState>()?
        .ready
        .lock()
        .ok()
        .and_then(|r| r.clone())
}

/// Restarts into the installed version.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    // The bulbs are in music mode while the effect runs: hand them back.
    if let Some(state) = app.try_state::<AppState>() {
        crate::lights::suspend(&state, false);
    }
    // Goes through RunEvent::Exit so the single-instance lock is released
    // before the new process starts; `restart()` would skip it.
    app.request_restart();
}

fn set_progress(app: &AppHandle, stage: Stage, percent: Option<u8>) {
    if let Ok(mut p) = app.state::<UpdateState>().progress.lock() {
        *p = Progress { stage, percent };
    }
}

/// Why no update was installed.
enum Failure {
    /// Checking or downloading failed: worth trying again soon.
    Fetch(String),
    /// The download is fine but could not be installed: trying again soon
    /// would only download it again for nothing.
    Install(String),
}

impl Failure {
    /// How long to wait before the next check.
    fn wait(&self) -> Duration {
        match self {
            Failure::Fetch(e) => {
                log::warn!("update: {e}");
                RETRY_AFTER
            }
            Failure::Install(e) => {
                log::error!("update install: {e}");
                INSTALL_RETRY
            }
        }
    }
}

fn failed_path(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().data_dir.join(FAILED_FILE)
}

fn failed_version(app: &AppHandle) -> Option<String> {
    if let Some(v) = app
        .state::<UpdateState>()
        .failed
        .lock()
        .ok()
        .and_then(|f| f.clone())
    {
        return Some(v);
    }
    let text = std::fs::read_to_string(failed_path(app)).ok()?;
    Some(text.trim().to_owned()).filter(|v| !v.is_empty())
}

/// Remembers the version that failed to install, or forgets it with `None`.
fn set_failed(app: &AppHandle, version: Option<&str>) {
    if let Ok(mut f) = app.state::<UpdateState>().failed.lock() {
        *f = version.map(str::to_owned);
    }
    let path = failed_path(app);
    let result = match version {
        Some(v) => path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, v)),
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        },
    };
    if let Err(e) = result {
        log::warn!("update: {}: {e}", path.display());
    }
}

/// Whether `offered` may replace `current`: only a strictly newer version.
/// (The plugin's default, kept here so no setting can turn it into "any
/// other version".)
fn is_upgrade<V: PartialOrd>(current: &V, offered: &V) -> bool {
    offered > current
}

/// Development builds never replace themselves and start right away.
pub fn enabled() -> bool {
    !cfg!(debug_assertions)
}

/// The splash follows the language and appearance the settings force, or the
/// system's.
fn splash_url(app: &AppHandle) -> String {
    let interface = app
        .state::<AppState>()
        .lock_config()
        .map(|c| c.interface)
        .unwrap_or_default();
    let mut query = Vec::new();
    match interface.language {
        Language::En => query.push("lang=en"),
        Language::Fr => query.push("lang=fr"),
        Language::System => {}
    }
    match interface.appearance {
        Appearance::Light => query.push("theme=light"),
        Appearance::Dark => query.push("theme=dark"),
        Appearance::System => {}
    }
    if query.is_empty() {
        "splash.html".into()
    } else {
        format!("splash.html?{}", query.join("&"))
    }
}

/// Shows the splash, applies a pending release, then calls `crate::start`.
pub fn launch(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App(splash_url(app).into()))
        .title("Lueur")
        .inner_size(300.0, 110.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .shadow(true)
        .center()
        .focused(false)
        .build()?;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let shown = Instant::now();
        let wait = match check_and_install(&app, true).await {
            Ok(Some(_)) => {
                set_progress(&app, Stage::Restarting, None);
                app.request_restart();
                return;
            }
            Ok(None) => CHECK_EVERY,
            Err(e) => e.wait(),
        };
        if let Some(rest) = SPLASH_MIN.checked_sub(shown.elapsed()) {
            tokio::time::sleep(rest).await;
        }
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            crate::start(&handle);
            // Closed after the main window may have shown, so the app never
            // has zero windows (which would end it).
            if let Some(splash) = handle.get_webview_window(SPLASH) {
                let _ = splash.destroy();
            }
        });
        watch(app, wait).await;
    });
    Ok(())
}

/// Checks again while the app runs; a found version waits for a restart.
/// The wait is a timer, not a parked thread: `spawn_blocking` would hold a
/// pool thread for an hour in an app that mostly sits in the tray.
async fn watch(app: AppHandle, mut wait: Duration) {
    loop {
        tokio::time::sleep(wait).await;
        wait = match check_and_install(&app, false).await {
            Ok(Some(version)) => {
                if let Ok(mut r) = app.state::<UpdateState>().ready.lock() {
                    *r = Some(version.clone());
                }
                let _ = app.emit(UPDATE_READY, version);
                crate::tray::refresh(&app);
                // Installed: nothing more to do until the next restart.
                return;
            }
            Ok(None) => CHECK_EVERY,
            Err(e) => e.wait(),
        };
    }
}

/// Installs a newer release if there is one and returns its version.
/// `splash` reports each step to the splash window, and skips a version that
/// already failed to install: the launch must not wait on a download that
/// cannot be used. The running app still tries it, every `INSTALL_RETRY`.
async fn check_and_install(app: &AppHandle, splash: bool) -> Result<Option<String>, Failure> {
    let update = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        .version_comparator(|current, release| is_upgrade(&current, &release.version))
        .build()
        .map_err(|e| Failure::Fetch(e.to_string()))?
        .check()
        .await
        .map_err(|e| Failure::Fetch(e.to_string()))?;
    let Some(mut update) = update else {
        return Ok(None);
    };
    if splash && failed_version(app).as_deref() == Some(update.version.as_str()) {
        log::warn!("update: {} failed to install before", update.version);
        return Ok(None);
    }
    update.timeout = Some(DOWNLOAD_TIMEOUT);

    let mut received = 0usize;
    let bytes = update
        .download(
            |chunk, total| {
                if !splash {
                    return;
                }
                received += chunk;
                let percent = total
                    .filter(|&t| t > 0)
                    .map(|t| (received as u64 * 100 / t).min(100) as u8);
                set_progress(app, Stage::Downloading, percent);
            },
            || {},
        )
        .await
        .map_err(|e| Failure::Fetch(e.to_string()))?;
    if splash {
        set_progress(app, Stage::Installing, None);
    }
    // Writing tens of megabytes and swapping the executable in is disk work:
    // left on the runtime's thread it would hold up every other task.
    let version = update.version.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || install(&update, &bytes))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    if let Err(e) = installed {
        set_failed(app, Some(&version));
        return Err(Failure::Install(e));
    }
    if failed_version(app).is_some() {
        set_failed(app, None);
    }
    Ok(Some(version))
}

#[cfg(windows)]
fn install(update: &Update, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;

    // The verified bytes are staged next to the executable, in a file this
    // process creates itself, so nothing can swap them before the replace.
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no install folder")?;
    let tmp = dir.join(format!("lueur.update-{}.exe", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        });
    let result = written
        .map_err(|e| e.to_string())
        .and_then(|()| self_replace::self_replace(&tmp).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&tmp);
    if result.is_ok() {
        crate::install::updated(&update.version);
    }
    result
}

#[cfg(not(windows))]
fn install(update: &Update, bytes: &[u8]) -> Result<(), String> {
    update.install(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn only_a_newer_version_is_an_update() {
        assert!(is_upgrade(&v("0.7.4"), &v("0.8.0")));
        assert!(is_upgrade(&v("0.9.0"), &v("0.10.0")));
        assert!(!is_upgrade(&v("0.8.0"), &v("0.8.0")));
        assert!(!is_upgrade(&v("0.8.0"), &v("0.7.4")));
        assert!(!is_upgrade(&v("0.8.0"), &v("0.8.0-beta.1")));
    }

    /// The version a manifest announces only counts because the signature
    /// binds it: the config must keep requiring that, and never allow going
    /// back.
    #[test]
    fn updates_require_a_signed_version_and_never_go_back() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(updater["requireSignedVersion"], true);
        assert_ne!(updater["allowDowngrades"], true);
        assert!(
            updater["endpoints"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e.as_str().unwrap().starts_with("https://"))
        );
    }
}
