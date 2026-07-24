//! Resolve where Lueur keeps its data, preferring a portable layout.

use std::path::PathBuf;

/// Resolve the data directory.
///
/// Order of preference:
/// 1. `LUEUR_DATA_DIR` env var, if set (handy for testing / power users).
/// 2. A `lueur-data` folder next to the executable (portable mode).
/// 3. The per-user config dir (`%APPDATA%`, `~/.config`, `~/Library/...`).
pub fn resolve_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LUEUR_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let user_config = user_config_dir();
    lueur_core::config::resolve_data_dir(&exe_dir, &user_config)
}

/// The OS per-user config directory, without pulling in an extra crate.
fn user_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library").join("Application Support");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg);
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config");
        }
    }
    PathBuf::from(".")
}
