//! Launch at login.
//!
//! Built exactly as the Tauri autostart plugin builds it (`auto-launch`,
//! named after the product, with `--minimized`, a LaunchAgent on macOS), so
//! the login item one version of Lueur wrote is the one the next reads and
//! rewrites, whichever shell draws its window.

use crate::MINIMIZED_ARG;
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// The name the login item carries: the product's, as the plugin used.
const APP_NAME: &str = "Lueur";

/// The path the login item starts: the AppImage rather than the copy it
/// mounted, the executable elsewhere.
fn app_path() -> std::io::Result<String> {
    let exe = std::env::current_exe()?;
    #[cfg(target_os = "linux")]
    {
        // Trusted only when this process really runs from the mounted image,
        // so a stray variable cannot point the login item elsewhere.
        if let (Some(image), Some(dir)) = (std::env::var_os("APPIMAGE"), std::env::var_os("APPDIR"))
            && exe.starts_with(&dir)
        {
            return Ok(std::path::PathBuf::from(image).display().to_string());
        }
    }
    #[cfg(target_os = "macos")]
    let exe = exe.canonicalize()?;
    Ok(exe.display().to_string())
}

fn launcher() -> Result<AutoLaunch, String> {
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name(APP_NAME);
    builder.set_args(&[MINIMIZED_ARG]);
    builder.set_app_path(&app_path().map_err(|e| e.to_string())?);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    builder.build().map_err(|e| e.to_string())
}

/// Whether Lueur opens at login.
pub fn is_enabled() -> bool {
    launcher().and_then(|l| l.is_enabled().map_err(|e| e.to_string())) == Ok(true)
}

/// Open Lueur at login, or stop doing so.
pub fn set(enabled: bool) -> Result<(), String> {
    let l = launcher()?;
    if enabled { l.enable() } else { l.disable() }.map_err(|e| e.to_string())
}

/// An install or an update may have moved the executable: point an enabled
/// login item at where it is now.
pub fn refresh() {
    if is_enabled()
        && let Err(e) = set(true)
    {
        log::warn!("cannot update the login item: {e}");
    }
}
