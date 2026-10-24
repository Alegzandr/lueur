//! Shared application state managed by Tauri.

use crate::auto;
use crate::error::{AppError, AppResult};
use crate::manual::Manual;
use crate::runner;
use crate::sleep;
use lueur_core::config::{self, AppConfig};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// Everything the IPC commands need, behind interior mutability so Tauri can
/// share it across threads.
pub struct AppState {
    pub data_dir: PathBuf,
    pub config: Mutex<AppConfig>,
    /// The running effect, if any.
    pub effect: Mutex<Option<runner::Handle>>,
    /// Lights being set by hand.
    pub manual: Manual,
    /// Who started the running effect, for automatic mode.
    pub auto_owner: Mutex<auto::Owner>,
    /// When the lights go off by themselves, if the user asked.
    pub sleep: sleep::Timer,
    /// Language the interface resolved to ("en" or "fr"), reported by the
    /// frontend so the tray menu can match it.
    pub ui_language: Mutex<&'static str>,
}

impl AppState {
    /// Load state from disk (or defaults) for the given data directory.
    pub fn load(data_dir: PathBuf) -> Self {
        let config = config::load(&data_dir).unwrap_or_else(|e| {
            eprintln!("lueur: failed to load config ({e}); using defaults");
            AppConfig::default()
        });
        AppState {
            data_dir,
            config: Mutex::new(config),
            effect: Mutex::new(None),
            manual: Manual::default(),
            auto_owner: Mutex::new(auto::Owner::default()),
            sleep: sleep::Timer::default(),
            ui_language: Mutex::new("en"),
        }
    }

    /// The config, locked.
    pub fn lock_config(&self) -> AppResult<MutexGuard<'_, AppConfig>> {
        self.config.lock().map_err(|_| AppError::poisoned())
    }

    /// Persist the current in-memory config to disk.
    pub fn persist(&self) -> AppResult<()> {
        let cfg = self.lock_config()?;
        config::save(&self.data_dir, &cfg).map_err(|e| AppError::new("save_failed").detail(e))
    }

    /// Is the ambilight effect currently running?
    pub fn is_running(&self) -> bool {
        self.effect
            .lock()
            .is_ok_and(|g| g.as_ref().is_some_and(|h| h.is_running()))
    }
}
