//! Resolve where Lueur keeps its data.

use std::path::PathBuf;

/// Resolve the data directory.
///
/// Order of preference:
/// 1. `LUEUR_DATA_DIR` env var, if set (handy for testing / power users).
/// 2. A `lueur` folder in the per-user config dir (`%APPDATA%`, `~/.config`,
///    `~/Library/Application Support`).
///
/// Never next to the executable: the install folder is replaced by updates.
pub fn resolve_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LUEUR_DATA_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    user_config_dir().join("lueur")
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
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.is_empty()
        {
            return PathBuf::from(xdg);
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config");
        }
    }
    PathBuf::from(".")
}
