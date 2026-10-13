//! Lueur desktop shell.
//!
//! Wires the pure [`lueur_core`] engine to real screen capture, the Yeelight
//! network and a Tauri webview, and adds the OS niceties gamers expect:
//! system-tray minimise, launch-on-startup and single-instance behaviour.

mod capture;
mod commands;
mod paths;
mod runner;
mod state;
mod tray;

use state::AppState;
use tauri::{Manager, WindowEvent};

/// Build and run the application.
pub fn run() {
    let data_dir = paths::resolve_data_dir();
    let app_state = AppState::load(data_dir);
    // Grab the startup behaviour before we hand the state to Tauri.
    let behavior = app_state
        .config
        .lock()
        .map(|c| c.behavior)
        .unwrap_or_default();

    tauri::Builder::default()
        // Only one instance may run; a second launch just focuses the window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::focus_main(app);
        }))
        // Launch-on-login. `--minimized` lets a startup launch open hidden.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(app_state)
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
        ])
        .setup(move |app| {
            tray::setup(app.handle())?;

            // Decide whether to show the window at launch. We start hidden if the
            // user asked to start minimised, or if we were relaunched by the OS
            // autostart with `--minimized`.
            let launched_minimized = std::env::args().any(|a| a == "--minimized");
            let start_hidden = behavior.start_minimized || launched_minimized;
            if let Some(win) = app.get_webview_window("main") {
                if start_hidden {
                    let _ = win.hide();
                } else {
                    let _ = win.show();
                }
            }

            // Optionally fire up the effect right away.
            if behavior.autostart_effect {
                let handle = app.handle().clone();
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
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray: hide instead of quitting when the user closes the
            // window and has tray-minimise enabled.
            if let WindowEvent::CloseRequested { api, .. } = event {
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
