//! The window frame, blended into the interface (Windows).
//!
//! A native caption is one colour across the whole width, so it could only
//! ever match the sidebar or the content, never both. On Windows the page
//! draws the bar instead (src/lib/ui/TitleBar.svelte): the sidebar runs to the
//! top edge and the caption buttons sit on the content. The window keeps its
//! system shadow, rounded corners, resize edges and Aero Snap (dragging goes
//! through the system); we only paint its one-pixel border the hairline
//! colour. Elsewhere the system's own bar stays.

use tauri::{Theme, WebviewWindow};

/// The hairline colour of `src/styles/app.css` (`--line`), in sRGB, for light
/// and dark. Keep it in step.
#[cfg(windows)]
const LIGHT: u32 = 0xde_de_de;
#[cfg(windows)]
const DARK: u32 = 0x30_30_30;

/// Drops the system caption. Called once, before the window first shows.
pub fn own(win: &WebviewWindow) {
    #[cfg(windows)]
    let _ = win.set_decorations(false);
    #[cfg(not(windows))]
    let _ = win;
}

/// Paints the window border for the theme the window now shows.
#[cfg(windows)]
pub fn paint(win: &WebviewWindow, theme: Theme) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::Graphics::Dwm::{DWMWA_BORDER_COLOR, DwmSetWindowAttribute};

    let Ok(hwnd) = win.hwnd() else { return };
    // A grey, so the 0x00BBGGRR order of a COLORREF does not matter.
    let color = COLORREF(if theme == Theme::Dark { DARK } else { LIGHT });
    let _ = unsafe {
        DwmSetWindowAttribute(
            HWND(hwnd.0),
            DWMWA_BORDER_COLOR,
            &color as *const COLORREF as *const c_void,
            size_of::<COLORREF>() as u32,
        )
    };
}

#[cfg(not(windows))]
pub fn paint(_win: &WebviewWindow, _theme: Theme) {}

/// Applies a forced theme (None follows the OS) and repaints the border for it.
pub fn set_theme(win: &WebviewWindow, theme: Option<Theme>) {
    let _ = win.set_theme(theme);
    if let Ok(shown) = win.theme() {
        paint(win, shown);
    }
}
