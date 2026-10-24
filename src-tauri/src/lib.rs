//! Lueur desktop shell.
//!
//! Wires the pure [`lueur_core`] engine to real screen capture, the Yeelight
//! network and a Tauri webview, and adds the OS niceties gamers expect:
//! system-tray minimise, launch-on-startup, single-instance behaviour, a
//! silent per-user install on Windows and updates from GitHub releases.

// The anti-cheat bans in `clippy.toml` are hard errors, not suggestions.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

mod capture;
mod commands;
mod error;
#[cfg(windows)]
mod install;
mod paths;
mod runner;
mod state;
mod tray;
mod updater;

use state::AppState;
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// Launch argument the OS autostart passes, so a login launch opens hidden.
const MINIMIZED_ARG: &str = "--minimized";

/// Starts the tray, the window and the effect, once the startup update check
/// is done.
pub(crate) fn start(app: &AppHandle) {
    // An install moved the executable: point the login item at the new path.
    let launcher = app.autolaunch();
    if launcher.is_enabled().unwrap_or(false) {
        let _ = launcher.enable();
    }

    if let Err(e) = tray::setup(app) {
        eprintln!("lueur: tray: {e}");
    }

    let behavior = app
        .state::<AppState>()
        .config
        .lock()
        .map(|c| c.behavior)
        .unwrap_or_default();

    // We stay hidden if the user asked to start minimised, or if we were
    // launched by the OS autostart with `--minimized`.
    let launched_minimized = std::env::args().any(|a| a == MINIMIZED_ARG);
    if !(behavior.start_minimized || launched_minimized) {
        tray::focus_main(app);
    }

    // Optionally fire up the effect right away.
    if behavior.autostart_effect {
        let handle = app.clone();
        std::thread::spawn(move || {
            let state = handle.state::<AppState>();
            let cfg = match state.config.lock() {
                Ok(c) => c.clone(),
                Err(_) => return,
            };
            if let Ok(h) = runner::start(&cfg)
                && let Ok(mut guard) = state.effect.lock()
            {
                *guard = Some(h);
            }
            tray::refresh(&handle);
        });
    }
}

/// Build and run the application.
pub fn run() {
    #[cfg(windows)]
    if !install::prepare() {
        return;
    }

    let data_dir = paths::resolve_data_dir();
    let app_state = AppState::load(data_dir);

    tauri::Builder::default()
        // Only one instance may run; a second launch just focuses the window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // During the startup update check, the splash is already showing.
            if app.get_webview_window(updater::SPLASH).is_none() {
                tray::focus_main(app);
            }
        }))
        // Launch-on-login. `--minimized` lets a startup launch open hidden.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_ARG]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .manage(updater::UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::list_monitors,
            commands::discover_devices,
            commands::test_device,
            commands::set_device_power,
            commands::set_light_power,
            commands::set_all_power,
            commands::start_effect,
            commands::stop_effect,
            commands::effect_status,
            commands::set_run_on_startup,
            commands::show_main_window,
            commands::set_ui_language,
            updater::update_progress,
            updater::update_ready,
            updater::restart_app,
        ])
        .setup(|app| {
            // The main window is created hidden; `start` shows it.
            if updater::enabled() {
                updater::launch(app.handle())?;
            } else {
                start(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray: hide instead of quitting when the user closes the
            // window and has tray-minimise enabled.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                let app = window.app_handle();
                let minimize_to_tray = app
                    .state::<AppState>()
                    .config
                    .lock()
                    .map(|c| c.behavior.minimize_to_tray)
                    .unwrap_or(true);
                if minimize_to_tray {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Lueur");
}
