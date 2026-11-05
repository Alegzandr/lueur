//! The commands: the entire surface a window can call.
//!
//! Each blocks for as long as the work takes (the network, a bulb handshake,
//! a file), so a front end calls them off its UI thread.
//!
//! Security notes:
//! * Every command validates its inputs (addresses, colour ranges, timeouts).
//! * No command takes a shell string, path to execute, or arbitrary URL; the
//!   network egress is LAN bulb control and discovery, the LAN broadcast
//!   that tells other computers running Lueur which bulbs this one drives
//!   (`peers.rs`: device ids and the computer's name, nothing secret), the
//!   Smart Life sign-in while the user is importing Tuya bulbs (Tuya's own
//!   hosts only, see `tuya_cloud.rs`), and the update check in `update.rs`,
//!   whose address is a constant. No command can point any of them elsewhere.
//! * The config export and import write and read only the file the user picks
//!   in the system's own save and open dialogs, which the front end opens; a
//!   page or a script never names the path.
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
use crate::{Shell, autostart};
use lueur_core::color::Rgb;
use lueur_core::config::AppConfig;
use lueur_core::device::relink::Link;
use lueur_core::device::{self, DeviceKind, DiscoveredDevice, Endpoint, hue, nanoleaf};
use lueur_core::scene::LightState;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// One running effect: a mode and the lights playing it, in step.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectGroup {
    /// `ambilight` or a show's id.
    pub mode: &'static str,
    pub light_ids: Vec<String>,
}

/// Result of querying/toggling the effects, sent back to the UI.
#[derive(Debug, Clone, serde::Serialize)]
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
    /// The ambilight reads an HDR screen, its colours brought back to SDR.
    pub hdr: bool,
    /// The ambilight has not been able to read the screen for a while; its
    /// lights hold their colour meanwhile.
    pub screen_lost: bool,
}

/// A room light whose bulb something else drives.
#[derive(Debug, Clone, serde::Serialize)]
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

pub fn status(state: &AppState) -> EffectStatus {
    let groups = state.effects.groups();
    let (reconnecting, elsewhere) = away(state);
    EffectStatus {
        reconnecting,
        elsewhere,
        music_waiting: state.effects.waiting_for_sound(),
        hdr: state.effects.hdr(),
        screen_lost: state.effects.screen_lost(),
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

/// Return the current configuration, without its credentials.
pub fn get_config(state: &AppState) -> AppResult<AppConfig> {
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
/// A slider drag saves several times a second, so the front end calls this
/// off its UI thread, and the tray menu is only rebuilt when what it shows
/// changed. New wall colours are shown at once on the lights set by hand.
pub fn save_config(shell: &Shell, mut config: AppConfig) -> AppResult<()> {
    let state: &AppState = shell;
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
    persist_and_go_live(state)?;
    if repainted {
        lights::rewall(state);
    }
    if has_lights != had_lights {
        shell.tray_refresh();
    }
    Ok(())
}

/// The config was replaced: persist it, push the live-tunable settings to a
/// running effect (topology changes need a restart, which the UI performs
/// explicitly) and let the tray grey out what needs a bulb.
fn applied(shell: &Shell) -> AppResult<()> {
    persist_and_go_live(shell)?;
    shell.tray_refresh();
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
pub const EXPORT_FILE_NAME: &str = "lueur-config.json";
/// A config is a few kilobytes; anything far larger is not one.
const IMPORT_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Save the configuration to the file the user picked, without the
/// credentials: the file is safe to share, and on another computer the
/// bridges are paired again.
pub fn export_config(state: &AppState, path: &Path) -> AppResult<()> {
    let json = {
        let cfg = state.lock_config()?;
        secrets::redact_config(&cfg)
            .to_json()
            .map_err(AppError::internal)?
    };
    std::fs::write(path, json).map_err(|e| AppError::new("export_failed").detail(e))
}

/// Replace the configuration with the one in the file the user picked, and
/// return it (without credentials).
///
/// Whether Lueur opens at login stays this computer's: it mirrors the OS
/// login item, which an import does not touch. So do its pairings; those an
/// older export carries are added to them. The photosensitivity protection
/// stays on if it was.
pub fn import_config(shell: &Shell, path: &Path) -> AppResult<AppConfig> {
    let invalid = |e: &dyn std::fmt::Display| AppError::new("import_invalid").detail(e);
    let size = std::fs::metadata(path).map_err(|e| invalid(&e))?.len();
    if size > IMPORT_MAX_BYTES {
        return Err(invalid(&"file too large"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| invalid(&e))?;
    let mut config = AppConfig::from_json(&text).map_err(|e| invalid(&e))?;
    // The effect connects straight to these.
    validate_addresses(&config)?;

    {
        let mut guard = shell.lock_config()?;
        config.behavior.run_on_startup = guard.behavior.run_on_startup;
        // A file may turn the photosensitivity protection on, never off:
        // someone else's setup must not take it away without a word.
        config.safety.flash_guard |= guard.safety.flash_guard;
        let mut pairings = guard.pairings.clone();
        secrets::merge_pairings(&mut pairings, std::mem::take(&mut config.pairings));
        config.pairings = pairings;
        // An older export's bindings carry their credentials: kept when no
        // pairing has one.
        let carried = config.bindings.clone();
        secrets::restore_bindings(&mut config, &carried);
        *guard = config;
    }
    applied(shell)?;
    let cfg = shell.lock_config()?;
    Ok(secrets::redact_config(&cfg))
}

/// List connected monitors for the capture picker.
pub fn list_monitors() -> AppResult<Vec<MonitorInfo>> {
    capture::list_monitors()
}

/// What the music show can listen to, for its source picker. Asking every
/// device for its name takes a moment.
pub fn list_audio_sources() -> AudioSources {
    audio::sources()
}

/// Discover bulbs of every family on the LAN, plus the Tuya bulbs imported
/// from Smart Life. `timeout_ms` is clamped to 500..=8000.
pub fn discover_devices(
    shell: &Shell,
    timeout_ms: Option<u64>,
) -> AppResult<Vec<DiscoveredDevice>> {
    let ms = timeout_ms.unwrap_or(2500).clamp(500, 8000);
    let (pairings, bindings) = {
        let cfg = shell.lock_config()?;
        (cfg.pairings.clone(), cfg.bindings.clone())
    };
    let mut found = devices::discover(Duration::from_millis(ms), &pairings, &bindings)
        .map_err(|e| AppError::new("discovery_failed").detail(e))?;
    // Discovery only hands a credential to a bridge found away from home once
    // its pinned certificate answered there: it moved, and its pairing
    // follows. What nothing vouches for waits for the user (`identity.rs`).
    if secrets::follow_moves(&mut *shell.lock_config()?, &found) {
        shell.persist()?;
    }
    crate::identity::note(shell, &found)?;
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
pub fn pair_device(state: &AppState, device: DiscoveredDevice) -> AppResult<()> {
    validate_ip(&device.ip)?;
    let family = device.kind.family();
    let bridges = state.lock_config()?.pairings.hue.clone();
    let granted = match &device.kind {
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
    })?;

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
#[derive(Debug, Clone, serde::Serialize)]
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
pub fn tuya_login_start(user_code: &str) -> AppResult<TuyaLogin> {
    let user_code = tuya_cloud::normalize_code(user_code);
    if !tuya_cloud::valid_code(&user_code) {
        return Err(AppError::new("invalid_user_code"));
    }
    let pending = tuya_cloud::start_login(&user_code).map_err(cloud_error)?;
    let svg = tuya_cloud::qr_svg(&pending.qr).ok_or_else(|| AppError::internal("QR"))?;
    Ok(TuyaLogin {
        token: pending.token,
        svg,
    })
}

/// Check whether the QR code was scanned. `None` until it is; then the
/// account's colour bulbs are imported and their number returned.
pub fn tuya_login_poll(state: &AppState, token: &str, user_code: &str) -> AppResult<Option<usize>> {
    let user_code = tuya_cloud::normalize_code(user_code);
    if !tuya_cloud::valid_code(token) || !tuya_cloud::valid_code(&user_code) {
        return Err(AppError::new("invalid_user_code"));
    }
    let Some(session) = tuya_cloud::poll_login(token, &user_code).map_err(cloud_error)? else {
        return Ok(None);
    };
    let bulbs = session.bulbs();
    session.end();
    let bulbs = bulbs.map_err(cloud_error)?;

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
pub fn test_device(state: &AppState, mut endpoint: Endpoint, color: Rgb) -> AppResult<()> {
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
    devices::identify(&endpoint, color)
        .map_err(|e| lights::named(unreachable(&endpoint.ip, e), light.as_deref()))
}

/// Turn the bulb bound to a room light on or off.
///
/// Resolving the binding here (rather than in the UI) keeps the address the
/// backend acts on the one it persisted itself.
pub fn set_light_power(shell: &Shell, light_id: &str, on: bool) -> AppResult<()> {
    let result = lights::set_light_power(shell, light_id, on);
    shell.tray_sync();
    result
}

/// Set a light by hand: on or off, brightness, white or colour. Called as a
/// slider moves; the interface keeps at most one call per light in flight.
/// The state is recorded once the bulb has it. A light set by hand no longer
/// follows the natural light, unless only its brightness changed (see
/// [`lights::set_by_hand`]); the others still do.
pub fn set_light_state(shell: &Shell, light_id: &str, state: LightState) -> AppResult<()> {
    let result = lights::set_by_hand(shell, light_id, state)
        .and_then(|()| lights::set_light(shell, light_id, state));
    // The tray's mood tick and status line follow; it rebuilds only when they
    // change.
    shell.tray_sync();
    result
}

/// Turn every bound bulb on or off at once. Off also stops the effect.
pub fn set_all_power(shell: &Shell, on: bool) -> AppResult<PowerResult> {
    let result = lights::set_all_power(shell, on);
    shell.tray_refresh();
    result
}

/// Apply a built-in mood to the lights named, or to every light with None.
/// Stops the effects playing on them.
pub fn apply_scene(
    shell: &Shell,
    id: &str,
    light_ids: Option<&[String]>,
) -> AppResult<PowerResult> {
    let result = lights::apply_scene(shell, id, light_ids);
    shell.tray_refresh();
    result
}

/// Start the ambilight, or the show named by `mode` (see `Mode::id`), on the
/// whole room, replacing whatever runs. Opening the bulbs blocks (the
/// music-mode handshake).
pub fn start_effect(shell: &Shell, mode: Option<&str>) -> AppResult<EffectStatus> {
    let mode = parse_mode(mode.unwrap_or("ambilight"))?;
    crate::auto::user_took_over(shell);
    let result = lights::start(shell, mode);
    shell.tray_refresh();
    result?;
    Ok(status(shell))
}

/// Stop the effect playing `mode`, or every effect without one. Joins the
/// loop thread.
pub fn stop_effect(shell: &Shell, mode: Option<&str>) -> AppResult<()> {
    let mode = mode.map(parse_mode).transpose()?;
    crate::auto::user_took_over(shell);
    match mode {
        Some(m) => lights::stop_mode(shell, m, true),
        None => lights::stop_effect(shell, true),
    }
    shell.tray_refresh();
    Ok(())
}

/// Play the mode named by `mode` on the lights named, in step with the
/// others playing it, or give them back to the hand with None.
pub fn set_lights_mode(
    shell: &Shell,
    light_ids: &[String],
    mode: Option<&str>,
) -> AppResult<EffectStatus> {
    let mode = mode.map(parse_mode).transpose()?;
    crate::auto::user_took_over(shell);
    let result = lights::set_lights_mode(shell, light_ids, mode);
    shell.tray_refresh();
    result?;
    Ok(status(shell))
}

/// Restart the effects so they pick up changes to the room, the bulbs or
/// the capture. Each keeps its lights.
pub fn restart_effect(shell: &Shell) -> AppResult<EffectStatus> {
    let result = lights::restart(shell);
    shell.tray_refresh();
    result?;
    Ok(status(shell))
}

/// Turn the lights off in `minutes` (1 to 240), or cancel the timer with
/// None.
pub fn set_sleep_timer(shell: &Arc<Shell>, minutes: Option<u32>) -> AppResult<Option<SleepStatus>> {
    crate::sleep::set(shell, minutes)
}

/// The sleep timer, if one is set.
pub fn sleep_timer(state: &AppState) -> Option<SleepStatus> {
    crate::sleep::status(state)
}

/// Record the language the interface settled on, for the tray menu.
pub fn set_ui_language(shell: &Shell, lang: &str) -> AppResult<()> {
    let lang = match lang {
        "fr" => "fr",
        _ => "en",
    };
    *shell.ui_language.lock().map_err(|_| AppError::poisoned())? = lang;
    shell.tray_refresh();
    Ok(())
}

/// Report whether the effect is running.
pub fn effect_status(state: &AppState) -> EffectStatus {
    status(state)
}

/// What every light an effect drives was last sent, zone by zone, so the
/// window can show the room as it plays.
pub fn light_glows() -> HashMap<String, Vec<runner::Glow>> {
    runner::glows()
}

/// Take a light's bulb from the other computer or app that drives it, so
/// the effect playing on it here drives it again.
pub fn take_light(state: &AppState, light_id: &str) -> AppResult<EffectStatus> {
    lights::take(state, light_id)?;
    Ok(status(state))
}

/// Open Lueur at login or stop doing so, and persist the preference.
pub fn set_run_on_startup(state: &AppState, enabled: bool) -> AppResult<()> {
    autostart::set(enabled).map_err(|e| AppError::new("login_item_failed").detail(e))?;
    state.lock_config()?.behavior.run_on_startup = enabled;
    state.persist()
}

/// Whether the credentials are encrypted on disk this session. Settings
/// says so when they are not, for want of a keyring.
pub fn credentials_sealed(state: &AppState) -> bool {
    state.credentials_sealed()
}
