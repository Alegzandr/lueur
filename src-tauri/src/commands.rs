//! Tauri IPC commands — the entire surface the frontend can call.
//!
//! Security notes:
//! * Every command validates its inputs (addresses, colour ranges, timeouts).
//! * No command takes a shell string, path to execute, or arbitrary URL; the
//!   network egress is LAN bulb control and discovery, the Smart Life sign-in
//!   while the user is importing Tuya bulbs (Tuya's own hosts only, see
//!   `tuya_cloud.rs`), and the update check in `updater.rs`. No command can
//!   point any of them elsewhere.
//! * The config export and import write and read only the file the user picks
//!   in the system's own save and open dialogs; the interface never names a
//!   path.
//! * Discovery/connect timeouts are clamped so a malicious reply can't wedge us.

use crate::capture::{self, MonitorInfo};
use crate::devices;
use crate::error::{AppError, AppResult};
use crate::lights::{self, PowerResult, poisoned, power_one, unreachable, validate_ip};
use crate::runner::LiveSettings;
use crate::state::AppState;
use crate::tuya_cloud::{self, CloudError};
use lueur_core::color::Rgb;
use lueur_core::config::AppConfig;
use lueur_core::device::{self, DeviceKind, DiscoveredDevice, Endpoint, hue, nanoleaf};
use lueur_core::scene::LightState;
use std::io;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

/// Result of querying/toggling the effect, sent back to the UI.
#[derive(serde::Serialize)]
pub struct EffectStatus {
    pub running: bool,
    pub connected_lights: usize,
}

/// Return the current configuration.
#[tauri::command]
pub fn get_config(state: State<AppState>) -> AppResult<AppConfig> {
    state
        .config
        .lock()
        .map(|c| c.clone())
        .map_err(|_| poisoned())
}

/// Replace and persist the configuration. If the effect is running, live-tunable
/// settings are pushed to it immediately.
///
/// Pairings are the backend's: they change behind the interface's back (a
/// bridge paired, a Smart Life import), so the copy sent here is ignored, and
/// Tuya bindings take their key from them.
#[tauri::command]
pub fn save_config(app: AppHandle, state: State<AppState>, mut config: AppConfig) -> AppResult<()> {
    {
        let mut guard = state.config.lock().map_err(|_| poisoned())?;
        config.pairings = guard.pairings.clone();
        refresh_tuya_keys(&mut config);
        *guard = config;
    }
    state.persist()?;
    push_live(&state)?;
    // Scenes may have been saved, renamed or removed.
    crate::tray::refresh(&app);
    Ok(())
}

/// Push live settings to a running effect (topology changes need a restart,
/// which the UI performs explicitly).
fn push_live(state: &AppState) -> AppResult<()> {
    let cfg = state.config.lock().map_err(|_| poisoned())?;
    if let Ok(guard) = state.effect.lock()
        && let Some(handle) = guard.as_ref()
    {
        handle.update_live(LiveSettings {
            engine: cfg.engine,
            effect: cfg.transition.effect,
            duration_ms: cfg.transition.duration_ms,
        });
    }
    Ok(())
}

/// Name the export dialog suggests.
const EXPORT_FILE_NAME: &str = "lueur-config.json";
/// A config is a few kilobytes; anything far larger is not one.
const IMPORT_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// A file dialog for config files, over the main window.
fn config_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let dialog = app.dialog().file().add_filter("JSON", &["json"]);
    match app.get_webview_window("main") {
        Some(win) => dialog.set_parent(&win),
        None => dialog,
    }
}

/// Save the whole configuration, pairings included, to a file the user
/// picks. `false` if they cancelled.
#[tauri::command]
pub async fn export_config(app: AppHandle) -> AppResult<bool> {
    let json = {
        let cfg = app.state::<AppState>();
        let cfg = cfg.config.lock().map_err(|_| poisoned())?;
        cfg.to_json().map_err(AppError::internal)?
    };
    blocking(move || {
        let Some(path) = config_dialog(&app)
            .set_file_name(EXPORT_FILE_NAME)
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path.into_path().map_err(AppError::internal)?;
        std::fs::write(&path, json).map_err(|e| AppError::new("export_failed").detail(e))?;
        Ok(true)
    })
    .await
}

/// Replace the configuration with one read from a file the user picks, and
/// return it. `None` if they cancelled.
///
/// Whether Lueur opens at login stays this computer's: it mirrors the OS
/// login item, which an import does not touch.
#[tauri::command]
pub async fn import_config(app: AppHandle) -> AppResult<Option<AppConfig>> {
    let handle = app.clone();
    let imported = blocking(move || {
        let Some(path) = config_dialog(&handle).blocking_pick_file() else {
            return Ok(None);
        };
        let path = path.into_path().map_err(AppError::internal)?;
        let invalid = |e: &dyn std::fmt::Display| AppError::new("import_invalid").detail(e);
        let size = std::fs::metadata(&path).map_err(|e| invalid(&e))?.len();
        if size > IMPORT_MAX_BYTES {
            return Err(invalid(&"file too large"));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| invalid(&e))?;
        let config = AppConfig::from_json(&text).map_err(|e| invalid(&e))?;
        // The effect connects straight to these.
        for ip in config
            .bindings
            .iter()
            .map(|b| &b.ip)
            .chain(config.pairings.hue.iter().map(|h| &h.ip))
        {
            validate_ip(ip)?;
        }
        Ok(Some(config))
    })
    .await?;
    let Some(mut config) = imported else {
        return Ok(None);
    };

    let state = app.state::<AppState>();
    {
        let mut guard = state.config.lock().map_err(|_| poisoned())?;
        config.behavior.run_on_startup = guard.behavior.run_on_startup;
        refresh_tuya_keys(&mut config);
        *guard = config.clone();
    }
    state.persist()?;
    push_live(&state)?;
    crate::tray::refresh(&app);
    Ok(Some(config))
}

/// List connected monitors for the capture picker.
#[tauri::command]
pub fn list_monitors() -> AppResult<Vec<MonitorInfo>> {
    capture::list_monitors()
}

/// Discover bulbs of every family on the LAN, plus the Tuya bulbs imported
/// from Smart Life. `timeout_ms` is clamped to 500..=8000.
#[tauri::command]
pub async fn discover_devices(
    state: State<'_, AppState>,
    timeout_ms: Option<u64>,
) -> AppResult<Vec<DiscoveredDevice>> {
    let ms = timeout_ms.unwrap_or(2500).clamp(500, 8000);
    let (pairings, bindings) = {
        let cfg = state.config.lock().map_err(|_| poisoned())?;
        (cfg.pairings.clone(), cfg.bindings.clone())
    };
    // Discovery blocks on the network; run it off the async runtime's worker.
    tauri::async_runtime::spawn_blocking(move || {
        devices::discover(Duration::from_millis(ms), &pairings, &bindings)
            .map_err(|e| AppError::new("discovery_failed").detail(e))
    })
    .await
    .map_err(AppError::internal)?
}

/// A credential a bridge or controller granted.
enum Granted {
    Hue(device::HueBridge),
    Nanoleaf(device::NanoleafPairing),
}

/// Pair with a Hue bridge or Nanoleaf controller found by discovery.
///
/// Fails with `pair_waiting` until the user has pressed the bridge's link
/// button (or held the controller's power button); the interface retries.
#[tauri::command]
pub async fn pair_device(state: State<'_, AppState>, device: DiscoveredDevice) -> AppResult<()> {
    validate_ip(&device.ip)?;
    let family = device.kind.family();
    let granted = tauri::async_runtime::spawn_blocking(move || match &device.kind {
        DeviceKind::Hue { .. } => hue::pair(&device.ip).map(Granted::Hue),
        DeviceKind::Nanoleaf { .. } => {
            let id = device.id.strip_prefix("nanoleaf:").unwrap_or(&device.id);
            nanoleaf::pair(&device.ip, device.port, id).map(Granted::Nanoleaf)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "this device needs no pairing",
        )),
    })
    .await
    .map_err(AppError::internal)?
    .map_err(|e| {
        if e.kind() == io::ErrorKind::WouldBlock {
            AppError::new("pair_waiting").subject(family)
        } else {
            AppError::new("pair_failed").detail(e)
        }
    })?;

    {
        let mut cfg = state.config.lock().map_err(|_| poisoned())?;
        let p = &mut cfg.pairings;
        match granted {
            Granted::Hue(b) => {
                p.hue.retain(|x| x.id != b.id);
                p.hue.push(b);
            }
            Granted::Nanoleaf(n) => {
                p.nanoleaf.retain(|x| x.id != n.id);
                p.nanoleaf.push(n);
            }
        }
    }
    state.persist()
}

/// A Smart Life sign-in waiting for its QR code to be scanned.
#[derive(serde::Serialize)]
pub struct TuyaLogin {
    pub token: String,
    /// The QR code, as SVG markup generated here (never from the network).
    pub svg: String,
}

fn cloud_error(e: CloudError) -> AppError {
    match e {
        CloudError::Unreachable(_) => AppError::new("cloud_unreachable").detail(e),
        CloudError::Refused(_) => AppError::new("tuya_refused").detail(e),
    }
}

/// Start a Smart Life sign-in for the account with `user_code`.
#[tauri::command]
pub async fn tuya_login_start(user_code: String) -> AppResult<TuyaLogin> {
    if !tuya_cloud::valid_code(&user_code) {
        return Err(AppError::new("invalid_user_code"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let pending = tuya_cloud::start_login(&user_code).map_err(cloud_error)?;
        let svg = tuya_cloud::qr_svg(&pending.qr).ok_or_else(|| AppError::internal("QR"))?;
        Ok(TuyaLogin {
            token: pending.token,
            svg,
        })
    })
    .await
    .map_err(AppError::internal)?
}

/// Check whether the QR code was scanned. `None` until it is; then the
/// account's colour bulbs are imported and their number returned.
#[tauri::command]
pub async fn tuya_login_poll(
    state: State<'_, AppState>,
    token: String,
    user_code: String,
) -> AppResult<Option<usize>> {
    if !tuya_cloud::valid_code(&token) || !tuya_cloud::valid_code(&user_code) {
        return Err(AppError::new("invalid_user_code"));
    }
    let bulbs = tauri::async_runtime::spawn_blocking(move || {
        let Some(session) = tuya_cloud::poll_login(&token, &user_code).map_err(cloud_error)? else {
            return Ok(None);
        };
        let bulbs = session.bulbs();
        session.end();
        bulbs.map(Some).map_err(cloud_error)
    })
    .await
    .map_err(AppError::internal)??;
    let Some(bulbs) = bulbs else {
        return Ok(None);
    };

    let count = bulbs.len();
    {
        let mut cfg = state.config.lock().map_err(|_| poisoned())?;
        let known = &mut cfg.pairings.tuya;
        known.retain(|k| !bulbs.iter().any(|b| b.id == k.id));
        known.extend(bulbs);
        refresh_tuya_keys(&mut cfg);
    }
    state.persist()?;
    Ok(Some(count))
}

/// A bulb re-added in Smart Life gets a new key: carry the imported one to
/// the binding that drives it.
fn refresh_tuya_keys(cfg: &mut AppConfig) {
    let AppConfig {
        bindings, pairings, ..
    } = cfg;
    for binding in bindings {
        let Some(id) = binding.device_id.strip_prefix("tuya:") else {
            continue;
        };
        if let (Some(fresh), DeviceKind::Tuya { key, dps, .. }) =
            (pairings.tuya.iter().find(|b| b.id == id), &mut binding.kind)
        {
            key.clone_from(&fresh.key);
            *dps = fresh.dps;
        }
    }
}

/// Flash a bulb a colour so the user can identify it while placing lights.
#[tauri::command]
pub async fn test_device(endpoint: Endpoint, r: u8, g: u8, b: u8) -> AppResult<()> {
    validate_ip(&endpoint.ip)?;
    tauri::async_runtime::spawn_blocking(move || {
        devices::identify(&endpoint, Rgb::new(r, g, b)).map_err(|e| unreachable(&endpoint.ip, e))
    })
    .await
    .map_err(AppError::internal)?
}

/// Turn a single bulb on or off. Used by the per-light power button and by
/// anything that knows a bulb but not a room light.
#[tauri::command]
pub async fn set_device_power(endpoint: Endpoint, on: bool) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || power_one(&endpoint, on))
        .await
        .map_err(AppError::internal)?
}

/// Run a blocking light command off the async runtime's worker.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(AppError::internal)?
}

/// Turn the bulb bound to a room light on or off.
///
/// Resolving the binding here (rather than in the UI) keeps the address the
/// backend acts on the one it persisted itself.
#[tauri::command]
pub async fn set_light_power(app: AppHandle, light_id: String, on: bool) -> AppResult<()> {
    blocking(move || lights::set_light_power(&app.state::<AppState>(), &light_id, on)).await
}

/// Set a light by hand: on or off, brightness, white or colour. Called as a
/// slider moves; the interface keeps at most one call per light in flight.
/// The interface records the state in the config itself.
#[tauri::command]
pub async fn set_light_state(app: AppHandle, light_id: String, state: LightState) -> AppResult<()> {
    blocking(move || lights::set_light(&app.state::<AppState>(), &light_id, state)).await
}

/// Turn every bound bulb on or off at once. Off also stops the effect.
#[tauri::command]
pub async fn set_all_power(app: AppHandle, on: bool) -> AppResult<PowerResult> {
    let handle = app.clone();
    let result = blocking(move || lights::set_all_power(&handle.state::<AppState>(), on)).await;
    crate::tray::refresh(&app);
    result
}

/// Apply a built-in mood or a saved scene to every light. Stops the effect.
#[tauri::command]
pub async fn apply_scene(app: AppHandle, id: String) -> AppResult<PowerResult> {
    let handle = app.clone();
    let result = blocking(move || lights::apply_scene(&handle.state::<AppState>(), &id)).await;
    crate::tray::refresh(&app);
    result
}

/// Start the ambilight effect from the saved config.
#[tauri::command]
pub async fn start_effect(app: AppHandle) -> AppResult<EffectStatus> {
    // Opening the bulbs blocks (music-mode handshake); keep it off the UI thread.
    let handle = app.clone();
    let result = blocking(move || {
        let state = handle.state::<AppState>();
        crate::auto::user_took_over(&state);
        lights::start_effect(&state)
    })
    .await;
    crate::tray::refresh(&app);
    Ok(EffectStatus {
        running: true,
        connected_lights: result?,
    })
}

/// Stop the effect if running.
#[tauri::command]
pub async fn stop_effect(app: AppHandle) -> AppResult<()> {
    // stop() joins the loop thread; do it off the UI thread.
    let handle = app.clone();
    blocking(move || {
        let state = handle.state::<AppState>();
        crate::auto::user_took_over(&state);
        lights::stop_effect(&state, true);
        Ok(())
    })
    .await?;
    crate::tray::refresh(&app);
    Ok(())
}

/// Record the language the interface settled on, for the tray menu.
#[tauri::command]
pub fn set_ui_language(app: AppHandle, state: State<AppState>, lang: String) -> AppResult<()> {
    let lang = match lang.as_str() {
        "fr" => "fr",
        _ => "en",
    };
    *state.ui_language.lock().map_err(|_| poisoned())? = lang.to_string();
    crate::tray::refresh(&app);
    Ok(())
}

/// Report whether the effect is running.
#[tauri::command]
pub fn effect_status(state: State<AppState>) -> AppResult<EffectStatus> {
    let running = state.is_running();
    let connected = state
        .effect
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|h| h.connected_lights))
        .unwrap_or(0);
    Ok(EffectStatus {
        running,
        connected_lights: connected,
    })
}

/// Enable or disable launch-on-login via the autostart plugin, and persist the
/// preference in config.
#[tauri::command]
pub fn set_run_on_startup(app: AppHandle, state: State<AppState>, enabled: bool) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| AppError::new("login_item_failed").detail(e))?;
    } else {
        manager
            .disable()
            .map_err(|e| AppError::new("login_item_failed").detail(e))?;
    }
    {
        let mut cfg = state.config.lock().map_err(|_| poisoned())?;
        cfg.behavior.run_on_startup = enabled;
    }
    state.persist()
}

/// Show and focus the main window (used by the tray "Open" action / frontend).
#[tauri::command]
pub fn show_main_window(app: AppHandle) -> AppResult<()> {
    if let Some(win) = app.get_webview_window("main") {
        win.show().map_err(AppError::internal)?;
        win.set_focus().map_err(AppError::internal)?;
    }
    Ok(())
}
