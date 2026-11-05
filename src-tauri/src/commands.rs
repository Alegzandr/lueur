//! Tauri IPC commands: the entire surface the webview can call.
//!
//! Each is a thin wrapper over `lueur_shell::service`, where the work, the
//! input checks and the security notes live. What blocks (the network, a
//! bulb handshake, a file) runs off the async runtime's worker. The export
//! and import open the system's own save and open dialogs here: the page
//! never names a path.

use lueur_core::color::Rgb;
use lueur_core::config::AppConfig;
use lueur_core::device::{DiscoveredDevice, Endpoint};
use lueur_core::scene::LightState;
use lueur_shell::audio::AudioSources;
use lueur_shell::capture::MonitorInfo;
use lueur_shell::error::{AppError, AppResult};
use lueur_shell::identity::{self, Unverified};
use lueur_shell::lights::PowerResult;
use lueur_shell::runner::Glow;
use lueur_shell::service::{self, EffectStatus, TuyaLogin};
use lueur_shell::sleep::Status as SleepStatus;
use lueur_shell::{Shell, logging};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

type Sh<'a> = State<'a, Arc<Shell>>;

/// Run a blocking job off the async runtime's worker, with the shell.
async fn blocking<T: Send + 'static>(
    shell: &Arc<Shell>,
    f: impl FnOnce(&Arc<Shell>) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let shell = Arc::clone(shell);
    tauri::async_runtime::spawn_blocking(move || f(&shell))
        .await
        .map_err(AppError::internal)?
}

#[tauri::command]
pub fn get_config(shell: Sh) -> AppResult<AppConfig> {
    service::get_config(&shell)
}

#[tauri::command]
pub async fn save_config(shell: Sh<'_>, config: AppConfig) -> AppResult<()> {
    blocking(&shell, move |s| service::save_config(s, config)).await
}

/// A file dialog for config files, over the main window.
fn config_dialog(app: &AppHandle) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
    let dialog = app.dialog().file().add_filter("JSON", &["json"]);
    match app.get_webview_window(crate::MAIN_WINDOW) {
        Some(win) => dialog.set_parent(&win),
        None => dialog,
    }
}

/// `false` if the user cancelled.
#[tauri::command]
pub async fn export_config(app: AppHandle, shell: Sh<'_>) -> AppResult<bool> {
    blocking(&shell, move |s| {
        let Some(path) = config_dialog(&app)
            .set_file_name(service::EXPORT_FILE_NAME)
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path.into_path().map_err(AppError::internal)?;
        service::export_config(s, &path)?;
        Ok(true)
    })
    .await
}

/// `None` if the user cancelled.
#[tauri::command]
pub async fn import_config(app: AppHandle, shell: Sh<'_>) -> AppResult<Option<AppConfig>> {
    blocking(&shell, move |s| {
        let Some(path) = config_dialog(&app).blocking_pick_file() else {
            return Ok(None);
        };
        let path = path.into_path().map_err(AppError::internal)?;
        service::import_config(s, &path).map(Some)
    })
    .await
}

#[tauri::command]
pub fn list_monitors() -> AppResult<Vec<MonitorInfo>> {
    service::list_monitors()
}

#[tauri::command]
pub async fn list_audio_sources(shell: Sh<'_>) -> AppResult<AudioSources> {
    blocking(&shell, |_| Ok(service::list_audio_sources())).await
}

#[tauri::command]
pub async fn discover_devices(
    shell: Sh<'_>,
    timeout_ms: Option<u64>,
) -> AppResult<Vec<DiscoveredDevice>> {
    blocking(&shell, move |s| service::discover_devices(s, timeout_ms)).await
}

#[tauri::command]
pub async fn pair_device(shell: Sh<'_>, device: DiscoveredDevice) -> AppResult<()> {
    blocking(&shell, move |s| service::pair_device(s, device)).await
}

#[tauri::command]
pub async fn tuya_login_start(shell: Sh<'_>, user_code: String) -> AppResult<TuyaLogin> {
    blocking(&shell, move |_| service::tuya_login_start(&user_code)).await
}

#[tauri::command]
pub async fn tuya_login_poll(
    shell: Sh<'_>,
    token: String,
    user_code: String,
) -> AppResult<Option<usize>> {
    blocking(&shell, move |s| {
        service::tuya_login_poll(s, &token, &user_code)
    })
    .await
}

#[tauri::command]
pub async fn test_device(shell: Sh<'_>, endpoint: Endpoint, r: u8, g: u8, b: u8) -> AppResult<()> {
    blocking(&shell, move |s| {
        service::test_device(s, endpoint, Rgb::new(r, g, b))
    })
    .await
}

#[tauri::command]
pub async fn set_light_power(shell: Sh<'_>, light_id: String, on: bool) -> AppResult<()> {
    blocking(&shell, move |s| service::set_light_power(s, &light_id, on)).await
}

#[tauri::command]
pub async fn set_light_state(shell: Sh<'_>, light_id: String, state: LightState) -> AppResult<()> {
    blocking(&shell, move |s| {
        service::set_light_state(s, &light_id, state)
    })
    .await
}

#[tauri::command]
pub async fn set_all_power(shell: Sh<'_>, on: bool) -> AppResult<PowerResult> {
    blocking(&shell, move |s| service::set_all_power(s, on)).await
}

#[tauri::command]
pub async fn apply_scene(
    shell: Sh<'_>,
    id: String,
    light_ids: Option<Vec<String>>,
) -> AppResult<PowerResult> {
    blocking(&shell, move |s| {
        service::apply_scene(s, &id, light_ids.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn start_effect(shell: Sh<'_>, mode: Option<String>) -> AppResult<EffectStatus> {
    blocking(&shell, move |s| service::start_effect(s, mode.as_deref())).await
}

#[tauri::command]
pub async fn stop_effect(shell: Sh<'_>, mode: Option<String>) -> AppResult<()> {
    blocking(&shell, move |s| service::stop_effect(s, mode.as_deref())).await
}

#[tauri::command]
pub async fn set_lights_mode(
    shell: Sh<'_>,
    light_ids: Vec<String>,
    mode: Option<String>,
) -> AppResult<EffectStatus> {
    blocking(&shell, move |s| {
        service::set_lights_mode(s, &light_ids, mode.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn restart_effect(shell: Sh<'_>) -> AppResult<EffectStatus> {
    blocking(&shell, |s| service::restart_effect(s)).await
}

#[tauri::command]
pub fn set_sleep_timer(shell: Sh, minutes: Option<u32>) -> AppResult<Option<SleepStatus>> {
    service::set_sleep_timer(&shell, minutes)
}

#[tauri::command]
pub fn sleep_timer(shell: Sh) -> Option<SleepStatus> {
    service::sleep_timer(&shell)
}

#[tauri::command]
pub fn set_ui_language(shell: Sh, lang: String) -> AppResult<()> {
    service::set_ui_language(&shell, &lang)
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

#[tauri::command]
pub fn effect_status(shell: Sh) -> EffectStatus {
    service::effect_status(&shell)
}

#[tauri::command]
pub fn light_glows() -> HashMap<String, Vec<Glow>> {
    service::light_glows()
}

#[tauri::command]
pub fn take_light(shell: Sh, light_id: String) -> AppResult<EffectStatus> {
    service::take_light(&shell, &light_id)
}

#[tauri::command]
pub fn set_run_on_startup(shell: Sh, enabled: bool) -> AppResult<()> {
    service::set_run_on_startup(&shell, enabled)
}

#[tauri::command]
pub fn credentials_sealed(shell: Sh) -> bool {
    service::credentials_sealed(&shell)
}

#[tauri::command]
pub fn unverified_devices(shell: Sh) -> AppResult<Vec<Unverified>> {
    identity::unverified_devices(&shell)
}

#[tauri::command]
pub fn confirm_device(shell: Sh, id: String) -> AppResult<()> {
    identity::confirm_device(&shell, &id)
}

#[tauri::command]
pub fn forget_device(shell: Sh, id: String) -> AppResult<()> {
    identity::forget_device(&shell, &id)
}

/// Show the logs folder in the file manager.
#[tauri::command]
pub fn open_logs_folder(app: AppHandle) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = logging::logs_folder()?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| {
            log::warn!("cannot open the logs folder: {e}");
            AppError::new("logs_unavailable").detail(e)
        })
}
