//! Lueur desktop shell, drawn by Tauri.
//!
//! The app itself lives in `lueur_shell`: capture, the bulbs, the live loops,
//! the workers and the commands. This crate puts it in a Tauri webview and
//! adds what Tauri draws: the window, the tray, the splash and its updater.

// The anti-cheat bans in `clippy.toml` are hard errors, not suggestions.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

mod commands;
mod titlebar;
mod tray;
mod updater;

use lueur_core::config::Appearance;
use lueur_shell::{Event, Host, MINIMIZED_ARG, Shell, logging, paths};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// Label of the main window (see `tauri.conf.json`).
pub(crate) const MAIN_WINDOW: &str = "main";

/// Hands the shell's events to the webview, the tray and the window.
struct TauriHost(AppHandle);

impl Host for TauriHost {
    fn emit(&self, event: Event) {
        let app = &self.0;
        let name = event.name();
        let _ = match event {
            Event::EffectChanged(error) => app.emit(name, error),
            Event::LightsChanged | Event::AutoStarted => app.emit(name, ()),
            Event::DevicesUnverified(list) => app.emit(name, list),
            Event::SleepChanged(status) => app.emit(name, status),
            Event::UpdateReady(version) => app.emit(name, version),
            Event::TrayStale { always } => {
                tray::update(app, always);
                Ok(())
            }
            Event::FocusMain => {
                tray::focus_main(app);
                Ok(())
            }
        };
    }
}

/// Starts the tray, the window and the effect, once the startup update check
/// is done.
pub(crate) fn start(app: &AppHandle) {
    let shell = Arc::clone(&app.state::<Arc<Shell>>());
    if let Err(e) = tray::setup(app) {
        log::error!("tray: {e}");
    }

    let (behavior, appearance) = shell
        .lock_config()
        .map(|c| (c.behavior, c.interface.appearance))
        .unwrap_or_default();

    // A forced theme applies before the window first paints, or it would
    // open in the OS colours and switch once the page reads the config.
    if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
        titlebar::own(&win);
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

    shell.start();
}

/// Build and run the application.
pub fn run() {
    let data_dir = paths::resolve_data_dir();
    logging::init(&data_dir);

    #[cfg(windows)]
    if !lueur_shell::install::prepare() {
        return;
    }

    let shell = Shell::load(data_dir);

    tauri::Builder::default()
        // Only one instance may run; a second launch just focuses the window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // During the startup update check, the splash is already showing.
            if app.get_webview_window(updater::SPLASH).is_none() {
                tray::focus_main(app);
            }
        }))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        // Opens the logs folder, from Rust only: no capability grants it to
        // the page, and no script is injected to open links.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .manage(Arc::clone(&shell))
        .manage(updater::UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::export_config,
            commands::import_config,
            commands::list_monitors,
            commands::list_audio_sources,
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
            commands::light_glows,
            commands::take_light,
            commands::set_sleep_timer,
            commands::sleep_timer,
            commands::set_run_on_startup,
            commands::set_ui_language,
            commands::set_window_theme,
            commands::unverified_devices,
            commands::confirm_device,
            commands::forget_device,
            commands::credentials_sealed,
            commands::open_logs_folder,
            updater::update_progress,
            updater::update_ready,
            updater::restart_app,
        ])
        .setup(move |app| {
            shell.attach(Arc::new(TauriHost(app.handle().clone())));
            shell.spawn_workers();
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
            let shell = window.app_handle().state::<Arc<Shell>>();
            let to_tray = || {
                shell
                    .lock_config()
                    .map(|c| c.behavior.minimize_to_tray)
                    .unwrap_or(true)
            };
            match event {
                // Close-to-tray: hide instead of quitting. A sleep timer
                // keeps Lueur running too, whatever the setting: the tray
                // sets one without asking to keep running, and quitting
                // would leave the lights on.
                WindowEvent::CloseRequested { api, .. }
                    if to_tray() || lueur_shell::sleep::minutes(&shell).is_some() =>
                {
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
            // session ending), the bulbs are handed back.
            if let tauri::RunEvent::Exit = event
                && let Some(shell) = app.try_state::<Arc<Shell>>()
            {
                shell.quit();
            }
        });
}
