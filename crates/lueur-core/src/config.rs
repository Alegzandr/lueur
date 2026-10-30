//! Persistent configuration.
//!
//! The shell decides where the data directory is (see `src-tauri/src/paths.rs`);
//! this module only reads and writes `config.json` inside it.

use crate::device::{DeviceKind, Endpoint, Pairings};
use crate::engine::EngineSettings;
use crate::geometry::Room;
use crate::safety::SafetySettings;
use crate::scene::LightState;
use crate::show::ShowSettings;
use crate::yeelight::Effect;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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

/// Capture-related settings. A field an older config lacks takes its default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

/// What the music show listens to. Each choice it names by what outlives a
/// restart (a device id, an executable), with the name it showed when picked,
/// for while it is unplugged or closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AudioSource {
    /// The default output (on Windows; elsewhere the default input).
    #[default]
    Default,
    /// One output (elsewhere one input), by the audio backend's device id.
    Device { id: String, name: String },
    /// Only what one application plays (Windows), by its executable's file
    /// name, `spotify.exe` say.
    App { exe: String, name: String },
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
    /// Older versions' "start the ambilight on launch". Read once and turned
    /// into recorded modes (see [`AppConfig::from_json`]), never written.
    #[serde(default, rename = "autostart_effect", skip_serializing)]
    legacy_autostart: bool,
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
            legacy_autostart: false,
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
    /// What the music show listens to.
    #[serde(default)]
    pub audio: AudioSource,
    /// The lights that follow the time of day (see
    /// [`crate::scene::natural_at`]), by room light id, until something else
    /// is asked of them. Older versions kept one switch for the whole room;
    /// [`AppConfig::from_json`] turns it into every light with a bulb.
    #[serde(default, deserialize_with = "natural_lights")]
    pub natural: BTreeSet<String>,
    /// How much brighter or dimmer than the natural light's curve each of
    /// its lights was set by hand, as a factor, by room light id. A light
    /// with no entry follows the curve as is.
    #[serde(default)]
    pub natural_scale: BTreeMap<String, f32>,
    /// What each light plays (`ambilight` or a show's id), keyed by room
    /// light id, as the user last chose it: the next launch picks it up
    /// again. A light set by hand has no entry.
    #[serde(default)]
    pub modes: BTreeMap<String, String>,
    /// The photosensitivity protection, which every mode goes through.
    #[serde(default)]
    pub safety: SafetySettings,
    /// What the user saved of the whole room to bring back in one click,
    /// in the order they are shown. The interface records and recalls them.
    #[serde(default)]
    pub scenes: Vec<SavedScene>,
}

/// The whole room as the user saved it: what each light did then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedScene {
    pub id: String,
    pub name: String,
    /// What each light did, by room light id. A light added since has no
    /// entry, and is left as it is when the scene comes back.
    #[serde(default)]
    pub lights: BTreeMap<String, SceneLight>,
}

/// What one light did in a saved scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SceneLight {
    /// Played the ambilight or a show (its id, as in [`AppConfig::modes`]).
    Mode { mode: String },
    /// Followed the time of day.
    Natural,
    /// Was set by hand, or switched off.
    Hand { state: LightState },
}

/// Stands, while a config is read, for "every light" in an older config's
/// natural light switch.
const EVERY_LIGHT: &str = "*";

/// The natural light's lights, or an older config's switch for all of them.
fn natural_lights<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeSet<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Natural {
        Switch(bool),
        Lights(BTreeSet<String>),
    }
    Ok(match Natural::deserialize(d)? {
        Natural::Switch(true) => BTreeSet::from([EVERY_LIGHT.to_string()]),
        Natural::Switch(false) => BTreeSet::new(),
        Natural::Lights(ids) => ids,
    })
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
            audio: AudioSource::default(),
            natural: BTreeSet::new(),
            natural_scale: BTreeMap::new(),
            modes: BTreeMap::new(),
            safety: SafetySettings::default(),
            scenes: Vec::new(),
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
    ///
    /// A config that asked older versions to start the ambilight on launch
    /// records it as the mode of every enabled light instead: the launch now
    /// resumes each light's recorded mode. The wall colour older versions
    /// kept with the ambilight's settings moves to the room.
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        let mut cfg: AppConfig = serde_json::from_str(s)?;
        if let Some(wall) = cfg.engine.legacy_wall.take()
            && cfg.room.wall == crate::color::Rgb::WHITE
        {
            cfg.room.wall = wall;
        }
        if cfg.natural.remove(EVERY_LIGHT) {
            cfg.natural = cfg.bindings.iter().map(|b| b.light_id.clone()).collect();
        }
        if std::mem::take(&mut cfg.behavior.legacy_autostart) && cfg.modes.is_empty() {
            cfg.modes = cfg
                .room
                .lights
                .iter()
                .filter(|l| l.enabled && cfg.binding_for(&l.id).is_some())
                .map(|l| (l.id.clone(), "ambilight".to_string()))
                .collect();
        }
        Ok(cfg)
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
        // On disk before the rename, or a power cut can leave an empty file
        // where the old config was.
        .and_then(|mut f| f.write_all(json.as_bytes()).and_then(|()| f.sync_all()))
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
    fn saved_scenes_round_trip() {
        let json = r#"{ "scenes": [{ "id": "s1", "name": "Film", "lights": {
            "tv": { "kind": "mode", "mode": "ambilight" },
            "desk": { "kind": "hand", "state": { "on": true, "bright": 20,
                "tint": { "mode": "white", "kelvin": 2200 } } },
            "hall": { "kind": "natural" } } }] }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        let scene = &cfg.scenes[0];
        assert_eq!(scene.name, "Film");
        assert_eq!(
            scene.lights["tv"],
            SceneLight::Mode {
                mode: "ambilight".into()
            }
        );
        assert_eq!(scene.lights["hall"], SceneLight::Natural);
        assert!(matches!(scene.lights["desk"], SceneLight::Hand { state } if state.bright == 20));
        let back = AppConfig::from_json(&cfg.to_json().unwrap()).unwrap();
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
        assert!(cfg.scenes.is_empty());
    }

    #[test]
    fn a_capture_without_a_rate_keeps_the_default_rate() {
        let json = r#"{ "version": 1, "capture": { "monitor_index": 1 } }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert_eq!(cfg.capture.monitor_index, 1);
        assert_eq!(cfg.capture.target_fps, 45);
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
    fn light_modes_round_trip() {
        let json = r#"{ "modes": { "l1": "ambilight", "l2": "rainbow" } }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert_eq!(cfg.modes.get("l2").map(String::as_str), Some("rainbow"));
        let back = AppConfig::from_json(&cfg.to_json().unwrap()).unwrap();
        assert_eq!(back.modes, cfg.modes);
        // Configs saved before per-light modes have none.
        assert!(AppConfig::from_json("{}").unwrap().modes.is_empty());
    }

    #[test]
    fn launch_ambilight_becomes_recorded_modes() {
        // An older config: three lights, the second disabled, the third
        // without a bulb, and the ambilight asked for at launch.
        let mut old: serde_json::Value =
            serde_json::from_str(&AppConfig::default().to_json().unwrap()).unwrap();
        old["room"]["lights"] = serde_json::json!([
            { "id": "l1", "name": "a", "pos": { "x": 0, "y": 0 } },
            { "id": "l2", "name": "b", "pos": { "x": 0, "y": 0 }, "enabled": false },
            { "id": "l3", "name": "c", "pos": { "x": 0, "y": 0 } }
        ]);
        old["bindings"] = serde_json::json!([
            { "light_id": "l1", "device_id": "0x1", "ip": "192.168.1.5" },
            { "light_id": "l2", "device_id": "0x2", "ip": "192.168.1.6" }
        ]);
        old["behavior"]["autostart_effect"] = true.into();
        let cfg = AppConfig::from_json(&old.to_string()).unwrap();
        // Only the enabled light with a bulb follows the screen.
        assert_eq!(cfg.modes.len(), 1);
        assert_eq!(cfg.modes.get("l1").map(String::as_str), Some("ambilight"));
        // The old setting is not written back.
        assert!(!cfg.to_json().unwrap().contains("autostart_effect"));
    }

    #[test]
    fn the_natural_light_switch_becomes_its_lights() {
        let bindings = r#""bindings": [
            { "light_id": "l1", "device_id": "0x1", "ip": "192.168.1.5" },
            { "light_id": "l2", "device_id": "0x2", "ip": "192.168.1.6" }
        ]"#;
        let on = AppConfig::from_json(&format!(r#"{{ {bindings}, "natural": true }}"#)).unwrap();
        assert_eq!(on.natural, BTreeSet::from(["l1".into(), "l2".into()]));
        let off = AppConfig::from_json(&format!(r#"{{ {bindings}, "natural": false }}"#)).unwrap();
        assert!(off.natural.is_empty());
        let back = AppConfig::from_json(&on.to_json().unwrap()).unwrap();
        assert_eq!(back.natural, on.natural);
    }

    #[test]
    fn the_wall_colour_moves_to_the_room() {
        let json = r#"{ "engine": { "smoothing": 0.5, "brightness": 1, "saturation": 0,
            "sample_step": 4, "black_threshold": 6, "wall": { "r": 250, "g": 240, "b": 215 } } }"#;
        let cfg = AppConfig::from_json(json).unwrap();
        assert_eq!(cfg.room.wall, crate::color::Rgb::new(250, 240, 215));
        assert_eq!(cfg.engine.legacy_wall, None);
        let saved = cfg.to_json().unwrap();
        let back = AppConfig::from_json(&saved).unwrap();
        assert_eq!(back.room.wall, cfg.room.wall);
        // Written once, with the room.
        assert_eq!(saved.matches("\"wall\"").count(), 1);
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
