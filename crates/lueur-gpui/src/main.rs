//! Lueur, drawn with GPUI.
//!
//! The app itself lives in `lueur_shell`: capture, the bulbs, the live loops,
//! the workers and the commands. This crate draws its window and its tray,
//! and is replacing the Tauri interface (`src-tauri`, `src/`).

// Prevent an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The anti-cheat bans in `clippy.toml` are hard errors, not suggestions.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

mod app;
mod assets;
mod host;
mod i18n;
mod single;
mod state;
mod theme;
mod titlebar;
mod tray;
mod ui;
mod update;
mod views;
mod window;

use lueur_shell::{Shell, logging, paths};
use std::sync::Arc;

fn main() {
    let data_dir = paths::resolve_data_dir();
    logging::init(&data_dir);

    #[cfg(windows)]
    if !lueur_shell::install::prepare() {
        return;
    }

    // Only one instance may run; a second launch brings this one's window
    // up, and exits.
    let (second_tx, second_launches) = async_channel::unbounded();
    let Some(instance) = single::acquire(move || {
        let _ = second_tx.try_send(());
    }) else {
        return;
    };

    let shell = Shell::load(data_dir);
    let (host, events) = host::channel();
    shell.attach(Arc::new(host));
    shell.spawn_workers();

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            // A release build checks for an update behind a splash first;
            // then, or right away, `app::run` starts the app.
            update::launch(
                app::Started {
                    shell,
                    events,
                    second_launches,
                    instance,
                },
                cx,
            )
        });
}
