//! Persistent configuration.
//!
//! The shell decides where the data directory is (see `src-tauri/src/paths.rs`);
//! this module only reads and writes `config.json` inside it.

use crate::device::{DeviceKind, Endpoint, Pairings};
use crate::engine::EngineSettings;
use crate::geometry::Room;
use crate::scene::LightState;
use crate::show::ShowSettings;
use crate::yeelight::Effect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Bumped when the on-disk format changes incompatibly.
pub const CONFIG_VERSION: u32 = 1;

/// Links a light placed in the room ([`crate::geometry::Light::id`]) to a
/// physical bulb.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceBinding {
    /// Matches `Light.id` in the room layout.
    pub light_id: String,
    /// Device id (stable across IP changes; family-prefixed for every family
    /// but Yeelight).
    pub device_id: String,
    pub ip: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub name: String,
    /// Bindings saved before other families existed have none: Yeelight.
    #[serde(default)]
    pub kind: DeviceKind,
}

impl DeviceBinding {
    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            id: self.device_id.clone(),
            ip: self.ip.clone(),
            port: self.port,
            kind: self.kind.clone(),
        }
    }
}

fn default_port() -> u16 {
    crate::yeelight::DEFAULT_CONTROL_PORT
}

/// Which colour transition to push to the bulbs and how long it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransitionSettings {
    pub effect: Effect,
    /// Duration in ms for smooth transitions. Kept short for ambilight.
    pub duration_ms: u32,
}

impl Default for TransitionSettings {
    fn default() -> Self {
        // Smooth with a very short duration hides Yeelight's per-command steps
        // without adding noticeable lag.
        TransitionSettings {
            effect: Effect::Smooth,
            duration_ms: 80,
        }
    }
}

/// Capture-related settings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CaptureSettings {
    /// Index into the monitor list reported by the shell.
    pub monitor_index: usize,
    /// Target updates per second. The bulbs and network cap the useful value
    /// around 40-60; higher just wastes CPU.
    pub target_fps: u32,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        CaptureSettings {
            monitor_index: 0,
            target_fps: 45,
        }
    }
}

/// OS-integration behaviour toggles.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BehaviorSettings {
    /// Register the app to launch on user login.
    pub run_on_startup: bool,
    /// Hide to the system tray instead of quitting on window close.
    pub minimize_to_tray: bool,
    /// Start hidden in the tray (pairs well with `run_on_startup`).
    pub start_minimized: bool,
    /// Automatically start the ambilight effect when the app launches.
    pub autostart_effect: bool,
    /// Start the ambilight when a game or video goes fullscreen, and stop it
    /// when that ends (Windows).
    #[serde(default = "crate::default_true")]
    pub auto_fullscreen: bool,
}

impl Default for BehaviorSettings {
    fn default() -> Self {
        BehaviorSettings {
            run_on_startup: false,
            minimize_to_tray: true,
            start_minimized: false,
            autostart_effect: false,
            auto_fullscreen: true,
        }
    }
}

/// Colour scheme of the window. `System` follows the OS setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// Interface language. `System` follows the OS locale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    En,
    Fr,
}

/// Preferences for the desktop shell that do not affect the light.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct InterfaceSettings {
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub language: Language,
    /// The user has arranged the room plan at least once. Until then the
    /// first-run guide keeps pointing at it.
    #[serde(default)]
    pub room_arranged: bool,
}

/// The full persisted configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub room: Room,
    #[serde(default)]
    pub bindings: Vec<DeviceBinding>,
    /// What bridges and accounts granted Lueur, so their bulbs stay usable.
    #[serde(default)]
    pub pairings: Pairings,
    #[serde(default)]
    pub capture: CaptureSettings,
    #[serde(default)]
    pub engine: EngineSettings,
    #[serde(default)]
    pub transition: TransitionSettings,
    #[serde(default)]
    pub behavior: BehaviorSettings,
    #[serde(default)]
    pub interface: InterfaceSettings,
    /// What each light was last set to by hand, keyed by room light id, so
    /// the controls show it and switching a light back on restores it.
    #[serde(default)]
    pub states: BTreeMap<String, LightState>,
    /// How the light shows (fade, candle, music) play.
    #[serde(default)]
    pub show: ShowSettings,
    /// The lights follow the time of day (see [`crate::scene::natural_at`])
    /// until something else is asked of them.
    #[serde(default)]
    pub natural: bool,
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            version: CONFIG_VERSION,
            room: Room::default(),
            bindings: Vec::new(),
            pairings: Pairings::default(),
            capture: CaptureSettings::default(),
            engine: EngineSettings::default(),
            transition: TransitionSettings::default(),
            behavior: BehaviorSettings::default(),
            interface: InterfaceSettings::default(),
            states: BTreeMap::new(),
            show: ShowSettings::default(),
            natural: false,
        }
    }
}

impl AppConfig {
    /// The binding for a given room light, if any.
    pub fn binding_for(&self, light_id: &str) -> Option<&DeviceBinding> {
        self.bindings.iter().find(|b| b.light_id == light_id)
    }

    /// Serialise to pretty JSON.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Parse from JSON, tolerating missing fields (they take their defaults).
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

/// File name used inside the data directory.
pub const CONFIG_FILE_NAME: &str = "config.json";
/// Load config from `data_dir/config.json`, returning defaults if it's missing.
/// A corrupt file surfaces as an error so the shell can warn rather than
/// silently wiping the user's layout.
pub fn load(data_dir: &Path) -> Result<AppConfig, ConfigError> {
    let path = data_dir.join(CONFIG_FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(text) => AppConfig::from_json(&text).map_err(ConfigError::Parse),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(e) => Err(ConfigError::Io(e)),
    }
}

/// Atomically save config to `data_dir/config.json` (write temp, then rename) so
/// a crash mid-write can't corrupt the existing file.
///
/// The file holds the bulbs' credentials, so on Unix only its owner may read
/// it (on Windows the per-user `%APPDATA%` already sees to that).
pub fn save(data_dir: &Path, config: &AppConfig) -> Result<(), ConfigError> {
    use std::io::Write;

    std::fs::create_dir_all(data_dir).map_err(ConfigError::Io)?;
    let json = config.to_json().map_err(ConfigError::Parse)?;
    let final_path = data_dir.join(CONFIG_FILE_NAME);
    let tmp_path = data_dir.join(format!("{CONFIG_FILE_NAME}.tmp"));
    // A leftover temp file would keep its old permissions.
    let _ = std::fs::remove_file(&tmp_path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(&tmp_path)
        .and_then(|mut f| f.write_all(json.as_bytes()))
        .map_err(ConfigError::Io)?;
    std::fs::rename(&tmp_path, &final_path).map_err(ConfigError::Io)?;
    Ok(())
}

/// Errors from config I/O.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "config I/O error: {e}"),
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips() {
        let cfg = AppConfig::default();
        let json = cfg.to_json().unwrap();
        let back = AppConfig::from_json(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn missing_fields_take_defaults() {
        // Only a room is provided; everything else should default.
        let json = r#"{ "version": 1 }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert_eq!(cfg.capture, CaptureSettings::default());
        assert_eq!(cfg.behavior, BehaviorSettings::default());
        assert_eq!(cfg.transition, TransitionSettings::default());
        assert_eq!(cfg.interface.appearance, Appearance::System);
    }

    #[test]
    fn appearance_serialises_lowercase() {
        let json = r#"{ "version": 1, "interface": { "appearance": "dark" } }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert_eq!(cfg.interface.appearance, Appearance::Dark);
        assert!(cfg.to_json().unwrap().contains("\"dark\""));
        assert_eq!(cfg.interface.language, Language::System);
        assert!(!cfg.interface.room_arranged);
    }

    #[test]
    fn language_serialises_lowercase() {
        let json = r#"{ "interface": { "language": "fr" } }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert_eq!(cfg.interface.language, Language::Fr);
    }

    #[test]
    fn binding_lookup_works() {
        let mut cfg = AppConfig::default();
        cfg.bindings.push(DeviceBinding {
            light_id: "l1".into(),
            device_id: "0x1".into(),
            ip: "192.168.1.5".into(),
            port: 55443,
            model: "color".into(),
            name: "desk".into(),
            kind: DeviceKind::Yeelight,
        });
        assert!(cfg.binding_for("l1").is_some());
        assert!(cfg.binding_for("nope").is_none());
    }

    #[test]
    fn old_bindings_read_as_yeelight() {
        let json =
            r#"{ "bindings": [{ "light_id": "l1", "device_id": "0x1", "ip": "192.168.1.5" }] }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        let ep = cfg.bindings[0].endpoint();
        assert_eq!(ep.kind, DeviceKind::Yeelight);
        assert_eq!(ep.port, crate::yeelight::DEFAULT_CONTROL_PORT);
        assert!(cfg.pairings.hue.is_empty());
    }

    #[test]
    fn save_then_load_preserves_config() {
        // Use a unique temp dir under the OS temp location.
        let base = std::env::temp_dir().join(format!("lueur-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        let mut cfg = AppConfig::default();
        cfg.capture.target_fps = 30;
        save(&base, &cfg).unwrap();
        let loaded = load(&base).unwrap();
        assert_eq!(loaded.capture.target_fps, 30);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn load_missing_returns_default() {
        let base = std::env::temp_dir().join("lueur-does-not-exist-xyz");
        let _ = std::fs::remove_dir_all(&base);
        let cfg = load(&base).unwrap();
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn transition_effect_serialises_lowercase() {
        let t = TransitionSettings {
            effect: Effect::Sudden,
            duration_ms: 0,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"sudden\""), "got {json}");
    }
}
