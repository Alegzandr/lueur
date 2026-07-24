//! Tauri IPC commands — the entire surface the frontend can call.
//!
//! Security notes:
//! * Every command validates its inputs (addresses, colour ranges, timeouts).
//! * No command takes a shell string, path to execute, or arbitrary URL; the
//!   only network egress is LAN Yeelight control and SSDP discovery.
//! * Discovery/connect timeouts are clamped so a malicious reply can't wedge us.

use crate::capture::{self, MonitorInfo};
use crate::runner::{self, LiveSettings};
use crate::state::AppState;
use lueur_core::color::Rgb;
use lueur_core::config::AppConfig;
use lueur_core::yeelight::{self, DiscoveredDevice, YeelightControl};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

/// Result of querying/toggling the effect, sent back to the UI.
#[derive(serde::Serialize)]
pub struct EffectStatus {
    pub running: bool,
    pub connected_lights: usize,
}

/// Return the current configuration.
#[tauri::command]
pub fn get_config(state: State<AppState>) -> Result<AppConfig, String> {
    state
        .config
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "config lock poisoned".to_string())
}

/// Replace and persist the configuration. If the effect is running, live-tunable
/// settings are pushed to it immediately.
#[tauri::command]
pub fn save_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    {
        let mut guard = state.config.lock().map_err(|_| "config lock poisoned")?;
        *guard = config;
    }
    state.persist()?;

    // Push live settings to a running effect (topology changes need a restart,
    // which the UI performs explicitly).
    let cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
    if let Ok(guard) = state.effect.lock() {
        if let Some(handle) = guard.as_ref() {
            handle.update_live(LiveSettings {
                engine: cfg.engine,
                effect: cfg.transition.effect,
                duration_ms: cfg.transition.duration_ms,
            });
        }
    }
    Ok(())
}

/// List connected monitors for the capture picker.
#[tauri::command]
pub fn list_monitors() -> Result<Vec<MonitorInfo>, String> {
    capture::list_monitors()
}

/// Discover Yeelight bulbs on the LAN. `timeout_ms` is clamped to 500..=8000.
#[tauri::command]
pub async fn discover_devices(timeout_ms: Option<u64>) -> Result<Vec<DiscoveredDevice>, String> {
    let ms = timeout_ms.unwrap_or(2500).clamp(500, 8000);
    // Discovery blocks on UDP; run it off the async runtime's worker.
    tauri::async_runtime::spawn_blocking(move || {
        yeelight::discover(Duration::from_millis(ms)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("discovery task failed: {e}"))?
}

/// Flash a bulb a colour so the user can identify it while placing lights.
/// Uses a plain (rate-limited) control connection — this is a one-shot.
#[tauri::command]
pub async fn test_device(
    ip: String,
    port: Option<u16>,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    validate_ip(&ip)?;
    let port = port.unwrap_or(yeelight::DEFAULT_CONTROL_PORT);
    tauri::async_runtime::spawn_blocking(move || {
        let mut ctrl = YeelightControl::connect(&ip, port, Duration::from_secs(3))
            .map_err(|e| format!("connect failed: {e}"))?;
        ctrl.set_power(true).map_err(|e| e.to_string())?;
        ctrl.set_rgb(Rgb::new(r, g, b)).map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("test task failed: {e}"))?
}

/// Start the ambilight effect from the saved config.
#[tauri::command]
pub async fn start_effect(state: State<'_, AppState>) -> Result<EffectStatus, String> {
    // Stop any existing effect first (idempotent restart).
    stop_effect(state.clone()).await?;

    let config = state
        .config
        .lock()
        .map(|c| c.clone())
        .map_err(|_| "config lock poisoned".to_string())?;

    // `runner::start` blocks (music-mode handshake); keep it off the UI thread.
    let handle = tauri::async_runtime::spawn_blocking(move || runner::start(&config))
        .await
        .map_err(|e| format!("start task failed: {e}"))??;

    let connected = handle.connected_lights;
    {
        let mut guard = state.effect.lock().map_err(|_| "effect lock poisoned")?;
        *guard = Some(handle);
    }
    Ok(EffectStatus {
        running: true,
        connected_lights: connected,
    })
}

/// Stop the effect if running.
#[tauri::command]
pub async fn stop_effect(state: State<'_, AppState>) -> Result<(), String> {
    let handle = {
        let mut guard = state.effect.lock().map_err(|_| "effect lock poisoned")?;
        guard.take()
    };
    if let Some(mut handle) = handle {
        // stop() joins the loop thread; do it off the UI thread.
        tauri::async_runtime::spawn_blocking(move || handle.stop())
            .await
            .map_err(|e| format!("stop task failed: {e}"))?;
    }
    Ok(())
}

/// Report whether the effect is running.
#[tauri::command]
pub fn effect_status(state: State<AppState>) -> Result<EffectStatus, String> {
    let running = state.is_running();
    let connected = state
        .effect
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|h| h.connected_lights))
        .unwrap_or(0);
    Ok(EffectStatus { running, connected_lights: connected })
}

/// Enable or disable launch-on-login via the autostart plugin, and persist the
/// preference in config.
#[tauri::command]
pub fn set_run_on_startup(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())?;
    } else {
        manager.disable().map_err(|e| e.to_string())?;
    }
    {
        let mut cfg = state.config.lock().map_err(|_| "config lock poisoned")?;
        cfg.behavior.run_on_startup = enabled;
    }
    state.persist()
}

/// Show and focus the main window (used by the tray "Open" action / frontend).
#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.show().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Reject anything that isn't a bare IPv4/IPv6 address, so a command can never
/// be tricked into connecting somewhere unexpected via a hostname or URL.
fn validate_ip(ip: &str) -> Result<(), String> {
    ip.parse::<std::net::IpAddr>()
        .map(|_| ())
        .map_err(|_| format!("invalid IP address: {ip}"))
}
