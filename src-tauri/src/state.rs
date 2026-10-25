//! Shared application state managed by Tauri.

use crate::auto;
use crate::error::{AppError, AppResult};
use crate::manual::Manual;
use crate::runner;
use crate::sleep;
use crate::vault::{self, Vault};
use lueur_core::config::{self, AppConfig};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// Everything the IPC commands need, behind interior mutability so Tauri can
/// share it across threads.
pub struct AppState {
    pub data_dir: PathBuf,
    pub config: Mutex<AppConfig>,
    /// Where the credentials are kept on disk.
    vault: Mutex<Vault>,
    /// The running effects, one per mode.
    pub effects: runner::Effects,
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
            // The next save would overwrite it: keep the unreadable file
            // beside, where the room and the keys can still be recovered.
            if matches!(e, config::ConfigError::Parse(_)) {
                let path = data_dir.join(config::CONFIG_FILE_NAME);
                let _ = std::fs::rename(&path, path.with_extension("json.bad"));
            }
            AppConfig::default()
        });
        // Credentials still in the config (an older version, or a session
        // without the keyring) move into the vault right away.
        let plain = vault::holds_credentials(&config);
        let mut config = config;
        let vault = Vault::open(&data_dir, &mut config);
        let migrate = plain && matches!(vault, Vault::Ready { .. });
        let state = AppState {
            data_dir,
            config: Mutex::new(config),
            vault: Mutex::new(vault),
            effects: runner::Effects::default(),
            manual: Manual::default(),
            auto_owner: Mutex::new(auto::Owner::default()),
            sleep: sleep::Timer::default(),
            ui_language: Mutex::new("en"),
        };
        if migrate && let Err(e) = state.persist() {
            eprintln!("lueur: could not move the credentials into the vault ({e:?})");
        }
        state
    }

    /// The config, locked.
    pub fn lock_config(&self) -> AppResult<MutexGuard<'_, AppConfig>> {
        self.config.lock().map_err(|_| AppError::poisoned())
    }

    /// Persist the current in-memory config to disk.
    pub fn persist(&self) -> AppResult<()> {
        let cfg = self.lock_config()?;
        let on_disk = self
            .vault
            .lock()
            .map_err(|_| AppError::poisoned())?
            .seal(&self.data_dir, &cfg);
        config::save(&self.data_dir, &on_disk).map_err(|e| AppError::new("save_failed").detail(e))
    }

    /// Does any effect drive some light?
    pub fn is_running(&self) -> bool {
        self.effects.is_running()
    }
}
