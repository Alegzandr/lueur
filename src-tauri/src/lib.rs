//! Lueur desktop shell.
//!
//! Wires the pure [`lueur_core`] engine to real screen capture, the bulbs on
//! the network and a Tauri webview, and adds the OS niceties gamers expect:
//! system-tray minimise, launch-on-startup, single-instance behaviour, a
//! silent per-user install on Windows and updates from GitHub releases.

// The anti-cheat bans in `clippy.toml` are hard errors, not suggestions.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

mod audio;
mod auto;
mod capture;
mod commands;
mod devices;
mod error;
#[cfg(windows)]
mod install;
mod lights;
mod manual;
mod natural;
mod paths;
mod runner;
mod secrets;
mod sleep;
mod state;
mod titlebar;
mod tray;
mod tuya;
mod tuya_cloud;
mod updater;
mod vault;

use lueur_core::config::Appearance;
use state::AppState;
use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

/// Launch argument the OS autostart passes, so a login launch opens hidden.
const MINIMIZED_ARG: &str = "--minimized";

/// Label of the main window (see `tauri.conf.json`).
pub(crate) const MAIN_WINDOW: &str = "main";

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
    auto::spawn(app);
    natural::spawn(app);

    let (behavior, appearance) = app
        .state::<AppState>()
        .lock_config()
        .map(|c| (c.behavior, c.interface.appearance))
        .unwrap_or_default();

    // A forced theme applies before the window first paints, or it would
    // open in the OS colours and switch once the page reads the config.
    if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
        titlebar::set_theme(
            &win,
            match appearance {
                Appearance::System => None,
                Appearance::Light => Some(tauri::Theme::Light),
                Appearance::Dark => Some(tauri::Theme::Dark),
            },
        );
    }

    // We stay hidden if the user asked to start minimised, or if we were
    // launched by the OS autostart with `--minimized`.
    let launched_minimized = std::env::args().any(|a| a == MINIMIZED_ARG);
    if !(behavior.start_minimized || launched_minimized) {
        tray::focus_main(app);
    }

    // Each light picks up the mode it was left playing.
    let handle = app.clone();
    std::thread::spawn(move || {
        use tauri::Emitter;
        let _ = lights::resume(&handle.state::<AppState>());
        // The window may have read the state before the bulbs answered.
        let _ = handle.emit(tray::EFFECT_CHANGED, None::<error::AppError>);
        let _ = handle.emit(tray::LIGHTS_CHANGED, ());
        tray::refresh(&handle);
    });
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
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .manage(updater::UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::export_config,
            commands::import_config,
            commands::list_monitors,
            commands::discover_devices,
            commands::pair_device,
            commands::tuya_login_start,
            commands::tuya_login_poll,
            commands::test_device,
            commands::set_light_power,
            commands::set_all_power,
            commands::set_light_state,
            commands::apply_scene,
            commands::start_effect,
            commands::stop_effect,
            commands::set_lights_mode,
            commands::restart_effect,
            commands::effect_status,
            commands::set_sleep_timer,
            commands::sleep_timer,
            commands::set_run_on_startup,
            commands::set_ui_language,
            commands::set_window_theme,
            updater::update_progress,
            updater::update_ready,
            updater::restart_app,
        ])
        .setup(|app| {
            // An effect that ends by itself (every bulb gone) still shows as
            // playing until the window and the tray are told.
            let handle = app.handle().clone();
            runner::on_end(move || {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    use tauri::Emitter;
                    let _ = handle.emit(tray::EFFECT_CHANGED, None::<error::AppError>);
                    tray::refresh(&handle);
                });
            });
            // The main window is created hidden; `start` shows it.
            if updater::enabled() {
                updater::launch(app.handle())?;
            } else {
                start(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW {
                return;
            }
            let to_tray = || {
                window
                    .app_handle()
                    .state::<AppState>()
                    .lock_config()
                    .map(|c| c.behavior.minimize_to_tray)
                    .unwrap_or(true)
            };
            match event {
                // Close-to-tray: hide instead of quitting.
                WindowEvent::CloseRequested { api, .. } if to_tray() => {
                    let _ = window.hide();
                    api.prevent_close();
                }
                // Without close-to-tray, closing quits, so minimising is how
                // Lueur goes to the tray; with it, minimising stays ordinary.
                // A Mac keeps its Dock, where minimised windows belong.
                #[cfg(not(target_os = "macos"))]
                WindowEvent::Resized(_) if window.is_minimized().unwrap_or(false) && !to_tray() => {
                    let _ = window.hide();
                }
                // Following the OS, which just switched light or dark.
                WindowEvent::ThemeChanged(theme) => {
                    if let Some(win) = window.app_handle().get_webview_window(MAIN_WINDOW) {
                        titlebar::paint(&win, *theme);
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Lueur")
        .run(|app, event| {
            // However Lueur quits (the tray, closing the window, Cmd+Q, the
            // session ending), the bulbs leave music mode on a warm glow
            // rather than frozen on the last frame. A no-op when the tray's
            // Quit has done it already.
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app.try_state::<AppState>()
            {
                lights::suspend(&state, false);
            }
        });
}
