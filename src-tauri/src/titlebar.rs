//! The native title bar, blended into the interface (Windows).
//!
//! The bar stays the system's own, so snapping, Snap Layouts on the maximise
//! button, double-click, dragging and the system menu all keep working. We
//! only paint it the sidebar's colour and drop its caption and icon (the
//! sidebar already shows both), so it reads as the top of the window rather
//! than a separate strip. The caption colour needs Windows 11; on Windows 10
//! the bar keeps its system colour and only loses the caption.

use tauri::{Theme, WebviewWindow};

/// The sidebar and the hairline colours of `src/styles/app.css`, in sRGB:
/// `--sidebar` and `--line` for light and dark. Keep them in step.
#[cfg(windows)]
const LIGHT: (u32, u32) = (0xee_ee_ee, 0xde_de_de);
#[cfg(windows)]
const DARK: (u32, u32) = (0x12_12_12, 0x30_30_30);

/// Paints the title bar for the theme the window now shows.
#[cfg(windows)]
pub fn paint(win: &WebviewWindow, theme: Theme) {
    use std::ffi::c_void;
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DwmSetWindowAttribute,
    };
    use windows::Win32::UI::Controls::{
        SetWindowThemeAttribute, WTA_NONCLIENT, WTA_OPTIONS, WTNCA_NODRAWCAPTION, WTNCA_NODRAWICON,
    };

    let Ok(hwnd) = win.hwnd() else { return };
    let hwnd = HWND(hwnd.0);
    let (caption, border) = if theme == Theme::Dark { DARK } else { LIGHT };
    // Greys, so the 0x00BBGGRR order of a COLORREF does not matter.
    let set = |attribute, color: u32| unsafe {
        let color = COLORREF(color);
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            &color as *const COLORREF as *const c_void,
            size_of::<COLORREF>() as u32,
        )
    };
    let _ = set(DWMWA_CAPTION_COLOR, caption);
    let _ = set(DWMWA_BORDER_COLOR, border);

    // Title and icon off in the bar only: the taskbar and Alt+Tab keep them.
    let flags = WTNCA_NODRAWCAPTION | WTNCA_NODRAWICON;
    let options = WTA_OPTIONS {
        dwFlags: flags,
        dwMask: flags,
    };
    let _ = unsafe {
        SetWindowThemeAttribute(
            hwnd,
            WTA_NONCLIENT,
            &options as *const WTA_OPTIONS as *const c_void,
            size_of::<WTA_OPTIONS>() as u32,
        )
    };
}

#[cfg(not(windows))]
pub fn paint(_win: &WebviewWindow, _theme: Theme) {}

/// Applies a forced theme (None follows the OS) and repaints the bar for it.
pub fn set_theme(win: &WebviewWindow, theme: Option<Theme>) {
    let _ = win.set_theme(theme);
    if let Ok(shown) = win.theme() {
        paint(win, shown);
    }
}
