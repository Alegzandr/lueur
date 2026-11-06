//! Lueur, drawn with Slint.
//!
//! The app itself lives in `lueur_shell`: capture, the bulbs, the live loops,
//! the workers, the updates and the commands. This crate draws its window,
//! its splash and its tray.

// Prevent an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The anti-cheat bans in `clippy.toml` are hard errors, not suggestions.
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

slint::include_modules!();

mod app;
mod host;
mod i18n;
mod images;
mod models;
mod pages;
mod rt;
mod single;
mod state;
mod theme;
mod tray;
mod update;
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
    let (second_tx, second_launches) = std::sync::mpsc::channel();
    let second_tx = std::sync::Mutex::new(second_tx);
    let Some(instance) = single::acquire(move || {
        if let Ok(tx) = second_tx.lock() {
            let _ = tx.send(());
        }
    }) else {
        return;
    };

    let shell = Shell::load(data_dir);
    let (host, events) = host::channel();
    shell.attach(Arc::new(host));
    shell.spawn_workers();

    // The splash, the window and the tray are made once the event loop
    // runs: a Mac makes its tray icon only then.
    let started = app::Started {
        shell,
        events,
        second_launches,
        instance,
    };
    let start = std::cell::RefCell::new(Some(started));
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        if let Some(started) = start.borrow_mut().take() {
            update::launch(started);
        }
    });
    if let Err(e) = slint::run_event_loop_until_quit() {
        log::error!("the window's loop stopped: {e}");
    }
}
