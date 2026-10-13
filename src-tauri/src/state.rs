//! Shared application state managed by Tauri.

use crate::error::{AppError, AppResult};
use crate::runner;
use lueur_core::config::{self, AppConfig};
use std::path::PathBuf;
use std::sync::Mutex;

/// Everything the IPC commands need, behind interior mutability so Tauri can
/// share it across threads.
pub struct AppState {
    pub data_dir: PathBuf,
    pub config: Mutex<AppConfig>,
    /// The running effect, if any.
    pub effect: Mutex<Option<runner::Handle>>,
    /// Language the interface resolved to ("en" or "fr"), reported by the
    /// frontend so the tray menu can match it.
    pub ui_language: Mutex<String>,
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
            ui_language: Mutex::new("en".into()),
        }
    }

    /// Persist the current in-memory config to disk.
    pub fn persist(&self) -> AppResult<()> {
        let cfg = self
            .config
            .lock()
            .map_err(|_| AppError::internal("config lock poisoned"))?;
        config::save(&self.data_dir, &cfg).map_err(|e| AppError::new("save_failed").detail(e))
    }

    /// Is the ambilight effect currently running?
    pub fn is_running(&self) -> bool {
        self.effect
            .lock()
            .map(|g| g.as_ref().map(|h| h.is_running()).unwrap_or(false))
            .unwrap_or(false)
    }
}
