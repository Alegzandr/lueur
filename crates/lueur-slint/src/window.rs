//! The window around the pages: the sidebar, the update button and the
//! toasts, and what their buttons do.

use crate::i18n::{self, t};
use crate::models::keep;
use crate::rt;
use crate::state::{Lueur, View};
use crate::{AppWindow, Nav, NavItem, Toast};
use slint::{ComponentHandle, SharedString};

/// Wire the window's own buttons.
pub fn bind(ui: &AppWindow) {
    let nav = ui.global::<Nav>();
    nav.on_go(|i| {
        if let Some(view) = View::from_index(i) {
            rt::with(|l| l.show(view));
        }
    });
    nav.on_toggle_effect(|| rt::with(Lueur::toggle_effect));
    titlebar(ui);
    nav.on_restart_into_update(crate::app::restart_into_update);
    nav.on_dismiss(|id| rt::with(|l| l.dismiss(id as u64)));
    nav.on_hold_toast(|id, held| rt::with(|l| l.hold_toast(id as u64, held)));
    nav.on_toast_action(|id| {
        let action = rt::with(|l| {
            let action = l
                .toasts
                .iter()
                .find(|t| t.id == id as u64)
                .and_then(|t| t.action.as_ref().map(|(_, run)| run.clone()));
            l.dismiss(id as u64);
            action
        });
        // Run outside the state: the action reaches it itself.
        if let Some(run) = action {
            run();
        }
    });
}

/// The window's own title bar, on Windows (`TitleBar` in `shell.slint`):
/// its strip moves the window through the system, so snapping to the
/// screen's edges works as with the system's bar.
fn titlebar(ui: &AppWindow) {
    let nav = ui.global::<Nav>();
    nav.set_own_titlebar(cfg!(windows));
    nav.set_caption_font(caption_font().into());
    nav.on_drag_window(|| {
        use slint::winit_030::WinitWindowAccessor;
        let Some(ui) = rt::ui() else { return };
        ui.window().with_winit_window(|w| {
            if let Err(e) = w.drag_window() {
                log::warn!("cannot move the window: {e}");
            }
        });
    });
    nav.on_minimize(|| {
        if let Some(ui) = rt::ui() {
            ui.window().set_minimized(true);
        }
    });
    nav.on_toggle_maximize(|| {
        if let Some(ui) = rt::ui() {
            let w = ui.window();
            w.set_maximized(!w.is_maximized());
        }
    });
    nav.on_close_window(crate::app::close_main);
}

/// The face of the caption glyphs: Windows 11's, or Windows 10's.
fn caption_font() -> &'static str {
    #[cfg(windows)]
    {
        use windows::Wdk::System::SystemServices::RtlGetVersion;
        let mut version = windows::Win32::System::SystemInformation::OSVERSIONINFOW::default();
        // SAFETY: RtlGetVersion only fills the struct it is handed.
        let ok = unsafe { RtlGetVersion(&mut version) }.is_ok();
        if ok && version.dwBuildNumber < 22000 {
            return "Segoe MDL2 Assets";
        }
    }
    "Segoe Fluent Icons"
}

/// Bring the sidebar, the update button and the toasts up to date.
pub fn project(l: &Lueur, ui: &AppWindow) {
    let nav = ui.global::<Nav>();
    if nav.get_view() != l.view.index() {
        nav.set_view(l.view.index());
    }
    // Lights drawn in the room with no bulb yet, counted on Bulbs.
    let unbound = l
        .config
        .room
        .lights
        .iter()
        .filter(|light| l.config.binding_for(&light.id).is_none())
        .count();
    let items = View::ALL.iter().map(|&view| NavItem {
        label: t(view.label_key()).into(),
        icon: view.icon().into(),
        badge: if view == View::Bulbs {
            unbound as i32
        } else {
            0
        },
    });
    keep(nav.get_items(), |m| nav.set_items(m), items);

    let version = l.update_ready.clone().unwrap_or_default();
    nav.set_update_ready(version.as_str().into());
    nav.set_update_tip(i18n::fill(&t("update_ready"), &[("version", version.as_str())]).into());

    let toasts = l.toasts.iter().map(|toast| Toast {
        id: toast.id as i32,
        error: toast.error,
        text: if toast.count > 1 {
            SharedString::from(format!("{} (×{})", toast.text, toast.count))
        } else {
            toast.text.as_str().into()
        },
        detail: toast.detail.clone().unwrap_or_default().into(),
        action: toast
            .action
            .as_ref()
            .map(|(label, _)| label.as_str().into())
            .unwrap_or_default(),
    });
    keep(nav.get_toasts(), |m| nav.set_toasts(m), toasts);
}
