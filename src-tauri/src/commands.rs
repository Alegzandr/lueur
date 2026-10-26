//! Tauri IPC commands: the entire surface the frontend can call.
//!
//! Security notes:
//! * Every command validates its inputs (addresses, colour ranges, timeouts).
//! * No command takes a shell string, path to execute, or arbitrary URL; the
//!   network egress is LAN bulb control and discovery, the LAN broadcast
//!   that tells other computers running Lueur which bulbs this one drives
//!   (`peers.rs`: device ids and the computer's name, nothing secret), the
//!   Smart Life sign-in while the user is importing Tuya bulbs (Tuya's own
//!   hosts only, see `tuya_cloud.rs`), and the update check in
//!   `updater.rs`. No command can point any of them elsewhere.
//! * The config export and import write and read only the file the user picks
//!   in the system's own save and open dialogs; the interface never names a
//!   path.
//! * Credentials (Hue, Nanoleaf, Tuya keys) never reach the interface nor an
//!   exported file: see `secrets.rs`.
//! * Every bulb address is a private or link-local IPv4 (`validate_ip`),
//!   whether it comes from the interface, an imported file or discovery.
//! * Discovery/connect timeouts are clamped so a malicious reply can't wedge us.

use crate::audio::{self, AudioSources};
use crate::capture::{self, MonitorInfo};
use crate::devices;
use crate::error::{AppError, AppResult};
use crate::lights::{self, PowerResult, unreachable, validate_ip};
use crate::runner::{self, LiveSettings, Mode};
use crate::secrets;
use crate::sleep::Status as SleepStatus;
use crate::state::AppState;
use crate::tuya_cloud::{self, CloudError};
use lueur_core::color::Rgb;
use lueur_core::config::AppConfig;
use lueur_core::device::relink::Link;
use lueur_core::device::{self, DeviceKind, DiscoveredDevice, Endpoint, hue, nanoleaf};
use lueur_core::scene::LightState;
use std::collections::HashMap;
use std::io;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

/// One running effect: a mode and the lights playing it, in step.
#[derive(serde::Serialize)]
pub struct EffectGroup {
    /// `ambilight` or a show's id.
    pub mode: &'static str,
    pub light_ids: Vec<String>,
}

/// Result of querying/toggling the effects, sent back to the UI.
#[derive(serde::Serialize)]
pub struct EffectStatus {
    pub running: bool,
    pub connected_lights: usize,
    /// The mode playing, when exactly one does.
    pub mode: Option<&'static str>,
    /// Every room light an effect drives.
    pub light_ids: Vec<String>,
    /// Each mode playing and its lights.
    pub groups: Vec<EffectGroup>,
    /// The driven lights whose bulb dropped and is being reconnected.
    pub reconnecting: Vec<String>,
    /// The room lights whose bulb something else drives: driven here, they
    /// wait for it, and the user may take it.
    pub elsewhere: Vec<Elsewhere>,
    /// The music plays but its sound source is away, and it waits for it.
    pub music_waiting: bool,
}

/// A room light whose bulb something else drives.
#[derive(serde::Serialize)]
pub struct Elsewhere {
    pub light_id: String,
    /// The computer running Lueur that drives it; none for another app (a
    /// Yeelight only says it is streamed to, not by whom).
    pub by: Option<String>,
}

/// Which driven lights are reconnecting, and which room lights something
/// else drives.
fn away(state: &AppState) -> (Vec<String>, Vec<Elsewhere>) {
    let mut reconnecting = Vec::new();
    let mut elsewhere = Vec::new();
    // The effects first: the config is not held while they are asked.
    let driven = state.effects.driven_ids();
    let links = state.effects.away();
    let Ok(cfg) = state.lock_config() else {
        return (reconnecting, elsewhere);
    };
    let holder = |light_id: &str| {
        cfg.binding_for(light_id)
            .and_then(|b| state.leases.holder(&b.device_id))
    };
    for (light_id, link) in links {
        let by = holder(&light_id);
        match link {
            Link::Up => {}
            Link::Reconnecting => reconnecting.push(light_id),
            // Held with nobody to name: still listening for who is there.
            Link::Held if by.is_none() => reconnecting.push(light_id),
            Link::Held | Link::Taken => elsewhere.push(Elsewhere { light_id, by }),
        }
    }
    for light in cfg.room.lights.iter().filter(|l| !driven.contains(&l.id)) {
        if let Some(by) = holder(&light.id) {
            elsewhere.push(Elsewhere {
                light_id: light.id.clone(),
                by: Some(by),
            });
        }
    }
    (reconnecting, elsewhere)
}

fn status(state: &AppState) -> EffectStatus {
    let groups = state.effects.groups();
    let (reconnecting, elsewhere) = away(state);
    EffectStatus {
        reconnecting,
        elsewhere,
        music_waiting: state.effects.waiting_for_sound(),
        running: !groups.is_empty(),
        connected_lights: groups.iter().map(|g| g.2).sum(),
        mode: match groups.as_slice() {
            [(m, _, _)] => Some(m.id()),
            _ => None,
        },
        light_ids: groups.iter().flat_map(|g| g.1.iter().cloned()).collect(),
        groups: groups
            .into_iter()
            .map(|(m, light_ids, _)| EffectGroup {
                mode: m.id(),
                light_ids,
            })
            .collect(),
    }
}

fn parse_mode(id: &str) -> AppResult<Mode> {
    Mode::from_id(id).ok_or_else(|| AppError::new("unknown_mode"))
}

/// Run a blocking job (the network, a file dialog, a bulb handshake) off the
/// async runtime's worker.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(AppError::internal)?
}

/// Return the current configuration, without its credentials.
#[tauri::command]
pub fn get_config(state: State<AppState>) -> AppResult<AppConfig> {
    let cfg = state.lock_config()?;
    Ok(secrets::redact_config(&cfg))
}

/// Every address the effect and the commands would connect to.
fn validate_addresses(config: &AppConfig) -> AppResult<()> {
    let pairings = &config.pairings;
    config
        .bindings
        .iter()
        .map(|b| &b.ip)
        .chain(pairings.hue.iter().map(|h| &h.ip))
        .chain(pairings.nanoleaf.iter().map(|n| &n.ip))
        .try_for_each(|ip| validate_ip(ip))
}

/// Replace and persist the configuration. If the effect is running, live-tunable
/// settings are pushed to it immediately.
///
/// Pairings are the backend's: they change behind the interface's back (a
/// bridge paired, a Smart Life import), so the copy sent here is ignored, and
/// bindings take their credentials from them (the interface never has any).
/// So are the lights that follow the natural light: moods and lights set by
/// hand switch them. So are the modes recorded for each light: starting and
/// stopping effects records them. So is what each light was set to: the
/// interface's copy may predate a mood, the tray or the natural light, and
/// would put the old states back.
///
/// A slider drag saves several times a second, so the disk write runs off
/// the main thread, and the tray menu is only rebuilt when what it shows
/// changed. New wall colours are shown at once on the lights set by hand.
#[tauri::command]
pub async fn save_config(app: AppHandle, mut config: AppConfig) -> AppResult<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let (had_lights, repainted) = {
            let mut guard = state.lock_config()?;
            let had_lights = !guard.bindings.is_empty();
            let repainted = guard.room.wall != config.room.wall;
            config.pairings = guard.pairings.clone();
            config.natural = guard.natural.clone();
            config.natural_scale = guard.natural_scale.clone();
            config.modes = guard.modes.clone();
            config.states = guard.states.clone();
            validate_addresses(&config)?;
            secrets::restore_bindings(&mut config, &guard.bindings);
            *guard = config;
            (had_lights, repainted)
        };
        let has_lights = !state.lock_config()?.bindings.is_empty();
        persist_and_go_live(&state)?;
        if repainted {
            lights::rewall(&state);
        }
        if has_lights != had_lights {
            crate::tray::refresh(&app);
        }
        Ok(())
    })
    .await
}

/// The config was replaced: persist it, push the live-tunable settings to a
/// running effect (topology changes need a restart, which the UI performs
/// explicitly) and let the tray grey out what needs a bulb.
fn applied(app: &AppHandle, state: &AppState) -> AppResult<()> {
    persist_and_go_live(state)?;
    crate::tray::refresh(app);
    Ok(())
}

fn persist_and_go_live(state: &AppState) -> AppResult<()> {
    state.persist()?;
    let live = LiveSettings::of(&*state.lock_config()?);
    state.manual.protect(live.safety.flash_guard);
    state.effects.update_live(live);
    Ok(())
}

/// Name the export dialog suggests.
const EXPORT_FILE_NAME: &str = "lueur-config.json";
/// A config is a few kilobytes; anything far larger is not one.
const IMPORT_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// A file dialog for config files, over the main window.
fn config_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let dialog = app.dialog().file().add_filter("JSON", &["json"]);
    match app.get_webview_window(crate::MAIN_WINDOW) {
        Some(win) => dialog.set_parent(&win),
        None => dialog,
    }
}

/// Save the configuration to a file the user picks, without the
/// credentials: the file is safe to share, and on another computer the
/// bridges are paired again. `false` if they cancelled.
#[tauri::command]
pub async fn export_config(app: AppHandle) -> AppResult<bool> {
    let json = {
        let state = app.state::<AppState>();
        let cfg = state.lock_config()?;
        secrets::redact_config(&cfg)
            .to_json()
            .map_err(AppError::internal)?
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
/// return it (without credentials). `None` if they cancelled.
///
/// Whether Lueur opens at login stays this computer's: it mirrors the OS
/// login item, which an import does not touch. So do its pairings; those an
/// older export carries are added to them.
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
        validate_addresses(&config)?;
        Ok(Some(config))
    })
    .await?;
    let Some(mut config) = imported else {
        return Ok(None);
    };

    let state = app.state::<AppState>();
    {
        let mut guard = state.lock_config()?;
        config.behavior.run_on_startup = guard.behavior.run_on_startup;
        let mut pairings = guard.pairings.clone();
        secrets::merge_pairings(&mut pairings, std::mem::take(&mut config.pairings));
        config.pairings = pairings;
        // An older export's bindings carry their credentials: kept when no
        // pairing has one.
        let carried = config.bindings.clone();
        secrets::restore_bindings(&mut config, &carried);
        *guard = config;
    }
    applied(&app, &state)?;
    let cfg = state.lock_config()?;
    Ok(Some(secrets::redact_config(&cfg)))
}

/// List connected monitors for the capture picker.
#[tauri::command]
pub fn list_monitors() -> AppResult<Vec<MonitorInfo>> {
    capture::list_monitors()
}

/// What the music show can listen to, for its source picker. Off the UI
/// thread: asking every device for its name takes a moment.
#[tauri::command]
pub async fn list_audio_sources() -> AppResult<AudioSources> {
    blocking(|| Ok(audio::sources())).await
}

/// Discover bulbs of every family on the LAN, plus the Tuya bulbs imported
/// from Smart Life. `timeout_ms` is clamped to 500..=8000.
#[tauri::command]
pub async fn discover_devices(
    app: AppHandle,
    state: State<'_, AppState>,
    timeout_ms: Option<u64>,
) -> AppResult<Vec<DiscoveredDevice>> {
    let ms = timeout_ms.unwrap_or(2500).clamp(500, 8000);
    let (pairings, bindings) = {
        let cfg = state.lock_config()?;
        (cfg.pairings.clone(), cfg.bindings.clone())
    };
    let mut found = blocking(move || {
        devices::discover(Duration::from_millis(ms), &pairings, &bindings)
            .map_err(|e| AppError::new("discovery_failed").detail(e))
    })
    .await?;
    // Discovery only hands a credential to a bridge found away from home once
    // its pinned certificate answered there: it moved, and its pairing
    // follows. What nothing vouches for waits for the user (`identity.rs`).
    if secrets::follow_moves(&mut *state.lock_config()?, &found) {
        state.persist()?;
    }
    crate::identity::note(&app, &found)?;
    secrets::redact_devices(&mut found);
    Ok(found)
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
    let bridges = state.lock_config()?.pairings.hue.clone();
    let granted = blocking(move || {
        match &device.kind {
            DeviceKind::Hue { .. } => hue::pair(&device.ip, &bridges).map(Granted::Hue),
            DeviceKind::Nanoleaf { .. } => {
                let id = device
                    .id
                    .strip_prefix(nanoleaf::ID_PREFIX)
                    .unwrap_or(&device.id);
                nanoleaf::pair(&device.ip, device.port, id).map(Granted::Nanoleaf)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "this device needs no pairing",
            )),
        }
        .map_err(|e| {
            if e.kind() == io::ErrorKind::WouldBlock {
                AppError::new("pair_waiting").subject(family)
            } else if let Some(refusal) = hue::pair_refusal(&e) {
                AppError::new(refusal.code())
            } else {
                AppError::new("pair_failed").detail(e)
            }
        })
    })
    .await?;

    {
        let mut cfg = state.lock_config()?;
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
        // Bindings at the device's address get its credential; those left at
        // another one lose theirs.
        secrets::follow_pairings(&mut cfg);
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
    let user_code = tuya_cloud::normalize_code(&user_code);
    if !tuya_cloud::valid_code(&user_code) {
        return Err(AppError::new("invalid_user_code"));
    }
    blocking(move || {
        let pending = tuya_cloud::start_login(&user_code).map_err(cloud_error)?;
        let svg = tuya_cloud::qr_svg(&pending.qr).ok_or_else(|| AppError::internal("QR"))?;
        Ok(TuyaLogin {
            token: pending.token,
            svg,
        })
    })
    .await
}

/// Check whether the QR code was scanned. `None` until it is; then the
/// account's colour bulbs are imported and their number returned.
#[tauri::command]
pub async fn tuya_login_poll(
    state: State<'_, AppState>,
    token: String,
    user_code: String,
) -> AppResult<Option<usize>> {
    let user_code = tuya_cloud::normalize_code(&user_code);
    if !tuya_cloud::valid_code(&token) || !tuya_cloud::valid_code(&user_code) {
        return Err(AppError::new("invalid_user_code"));
    }
    let bulbs = blocking(move || {
        let Some(session) = tuya_cloud::poll_login(&token, &user_code).map_err(cloud_error)? else {
            return Ok(None);
        };
        let bulbs = session.bulbs();
        session.end();
        bulbs.map(Some).map_err(cloud_error)
    })
    .await?;
    let Some(bulbs) = bulbs else {
        return Ok(None);
    };

    let count = bulbs.len();
    {
        let mut cfg = state.lock_config()?;
        let known = &mut cfg.pairings.tuya;
        known.retain(|k| !bulbs.iter().any(|b| b.id == k.id));
        known.extend(bulbs);
        let before = cfg.bindings.clone();
        secrets::restore_bindings(&mut cfg, &before);
    }
    state.persist()?;
    Ok(Some(count))
}

/// Flash a bulb a colour so the user can identify it while placing lights.
/// The interface names the bulb; its credential comes from the pairings.
#[tauri::command]
pub async fn test_device(
    state: State<'_, AppState>,
    mut endpoint: Endpoint,
    r: u8,
    g: u8,
    b: u8,
) -> AppResult<()> {
    validate_ip(&endpoint.ip)?;
    // The error names the light the bulb drives, when it drives one.
    let light = {
        let cfg = state.lock_config()?;
        lights::device_light_name(&cfg, &endpoint.id).map(str::to_string)
    };
    if !secrets::restore(
        &endpoint.id,
        &endpoint.ip,
        &mut endpoint.kind,
        &state.lock_config()?.pairings,
    ) {
        return Err(lights::named(
            unreachable(&endpoint.ip, "not paired"),
            light.as_deref(),
        ));
    }
    blocking(move || {
        devices::identify(&endpoint, Rgb::new(r, g, b))
            .map_err(|e| lights::named(unreachable(&endpoint.ip, e), light.as_deref()))
    })
    .await
}

/// Turn the bulb bound to a room light on or off.
///
/// Resolving the binding here (rather than in the UI) keeps the address the
/// backend acts on the one it persisted itself.
#[tauri::command]
pub async fn set_light_power(app: AppHandle, light_id: String, on: bool) -> AppResult<()> {
    let handle = app.clone();
    let result =
        blocking(move || lights::set_light_power(&handle.state::<AppState>(), &light_id, on)).await;
    crate::tray::sync(&app);
    result
}

/// Set a light by hand: on or off, brightness, white or colour. Called as a
/// slider moves; the interface keeps at most one call per light in flight.
/// The state is recorded once the bulb has it. A light set by hand no longer
/// follows the natural light, unless only its brightness changed (see
/// [`lights::set_by_hand`]); the others still do.
#[tauri::command]
pub async fn set_light_state(app: AppHandle, light_id: String, state: LightState) -> AppResult<()> {
    let handle = app.clone();
    let result = blocking(move || {
        let s = handle.state::<AppState>();
        lights::set_by_hand(&s, &light_id, state)?;
        lights::set_light(&s, &light_id, state)
    })
    .await;
    // The tray's mood tick and status line follow; it rebuilds only when they change.
    crate::tray::sync(&app);
    result
}

/// Turn every bound bulb on or off at once. Off also stops the effect.
#[tauri::command]
pub async fn set_all_power(app: AppHandle, on: bool) -> AppResult<PowerResult> {
    let handle = app.clone();
    let result = blocking(move || lights::set_all_power(&handle.state::<AppState>(), on)).await;
    crate::tray::refresh(&app);
    result
}

/// Apply a built-in mood to the lights named, or to every light with null.
/// Stops the effects playing on them.
#[tauri::command]
pub async fn apply_scene(
    app: AppHandle,
    id: String,
    light_ids: Option<Vec<String>>,
) -> AppResult<PowerResult> {
    let handle = app.clone();
    let result = blocking(move || {
        lights::apply_scene(&handle.state::<AppState>(), &id, light_ids.as_deref())
    })
    .await;
    crate::tray::refresh(&app);
    result
}

/// Start the ambilight, or the show named by `mode` (see `Mode::id`), on the
/// whole room, replacing whatever runs.
#[tauri::command]
pub async fn start_effect(app: AppHandle, mode: Option<String>) -> AppResult<EffectStatus> {
    let mode = parse_mode(mode.as_deref().unwrap_or("ambilight"))?;
    // Opening the bulbs blocks (music-mode handshake); keep it off the UI thread.
    let handle = app.clone();
    let result = blocking(move || {
        let state = handle.state::<AppState>();
        crate::auto::user_took_over(&state);
        lights::start(&state, mode)
    })
    .await;
    crate::tray::refresh(&app);
    result?;
    Ok(status(&app.state::<AppState>()))
}

/// Stop the effect playing `mode`, or every effect without one.
#[tauri::command]
pub async fn stop_effect(app: AppHandle, mode: Option<String>) -> AppResult<()> {
    let mode = mode.as_deref().map(parse_mode).transpose()?;
    // stop() joins the loop thread; do it off the UI thread.
    let handle = app.clone();
    blocking(move || {
        let state = handle.state::<AppState>();
        crate::auto::user_took_over(&state);
        match mode {
            Some(m) => lights::stop_mode(&state, m, true),
            None => lights::stop_effect(&state, true),
        }
        Ok(())
    })
    .await?;
    crate::tray::refresh(&app);
    Ok(())
}

/// Play the mode named by `mode` on the lights named, in step with the
/// others playing it, or give them back to the hand with null.
#[tauri::command]
pub async fn set_lights_mode(
    app: AppHandle,
    light_ids: Vec<String>,
    mode: Option<String>,
) -> AppResult<EffectStatus> {
    let mode = mode.as_deref().map(parse_mode).transpose()?;
    let handle = app.clone();
    let result = blocking(move || {
        let state = handle.state::<AppState>();
        crate::auto::user_took_over(&state);
        lights::set_lights_mode(&state, &light_ids, mode)
    })
    .await;
    crate::tray::refresh(&app);
    result?;
    Ok(status(&app.state::<AppState>()))
}

/// Restart the effects so they pick up changes to the room, the bulbs or
/// the capture. Each keeps its lights.
#[tauri::command]
pub async fn restart_effect(app: AppHandle) -> AppResult<EffectStatus> {
    let handle = app.clone();
    let result = blocking(move || lights::restart(&handle.state::<AppState>())).await;
    crate::tray::refresh(&app);
    result?;
    Ok(status(&app.state::<AppState>()))
}

/// Turn the lights off in `minutes` (1 to 240), or cancel the timer with
/// null.
#[tauri::command]
pub fn set_sleep_timer(app: AppHandle, minutes: Option<u32>) -> AppResult<Option<SleepStatus>> {
    crate::sleep::set(&app, minutes)
}

/// The sleep timer, if one is set.
#[tauri::command]
pub fn sleep_timer(state: State<AppState>) -> Option<SleepStatus> {
    crate::sleep::status(&state)
}

/// Record the language the interface settled on, for the tray menu.
#[tauri::command]
pub fn set_ui_language(app: AppHandle, state: State<AppState>, lang: String) -> AppResult<()> {
    let lang = match lang.as_str() {
        "fr" => "fr",
        _ => "en",
    };
    *state.ui_language.lock().map_err(|_| AppError::poisoned())? = lang;
    crate::tray::refresh(&app);
    Ok(())
}

/// Force the window's theme (None follows the OS), its frame included.
#[tauri::command]
pub fn set_window_theme(window: tauri::WebviewWindow, theme: Option<String>) {
    let theme = match theme.as_deref() {
        Some("light") => Some(tauri::Theme::Light),
        Some("dark") => Some(tauri::Theme::Dark),
        _ => None,
    };
    crate::titlebar::set_theme(&window, theme);
}

/// Report whether the effect is running.
#[tauri::command]
pub fn effect_status(state: State<AppState>) -> AppResult<EffectStatus> {
    Ok(status(&state))
}

/// What every light an effect drives was last sent, zone by zone, so the
/// window can show the room as it plays.
#[tauri::command]
pub fn light_glows() -> HashMap<String, Vec<runner::Glow>> {
    runner::glows()
}

/// Take a light's bulb from the other computer or app that drives it, so
/// the effect playing on it here drives it again.
#[tauri::command]
pub fn take_light(state: State<AppState>, light_id: String) -> AppResult<EffectStatus> {
    lights::take(&state, &light_id)?;
    Ok(status(&state))
}

/// Enable or disable launch-on-login via the autostart plugin, and persist the
/// preference in config.
#[tauri::command]
pub fn set_run_on_startup(app: AppHandle, state: State<AppState>, enabled: bool) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let changed = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    changed.map_err(|e| AppError::new("login_item_failed").detail(e))?;
    state.lock_config()?.behavior.run_on_startup = enabled;
    state.persist()
}
