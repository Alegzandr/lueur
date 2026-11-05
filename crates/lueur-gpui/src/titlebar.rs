//! The title bar on Windows, drawn by the app as the Tauri version drew it
//! (`src/lib/ui/TitleBar.svelte`): the sidebar runs to the top edge and the
//! caption buttons sit on the page, in its colours. The empty strip drags
//! the window and a double-click maximises it, through the system; the
//! buttons are the system's own glyphs and sizes, and the system handles
//! them (snap layouts included). Elsewhere the system's own bar stays.

use crate::theme::Theme;
use gpui::{
    Hsla, IntoElement, ParentElement, Rgba, Styled, Window, WindowControlArea, div, prelude::*, px,
};

/// Whether this system gets the app's own title bar.
pub const OWN: bool = cfg!(windows);

/// The bar's height, the system's.
pub const HEIGHT: f32 = 32.0;

/// The face of the caption glyphs: Windows 11's, or Windows 10's.
fn glyph_font() -> &'static str {
    #[cfg(windows)]
    {
        use windows::Wdk::System::SystemServices::RtlGetVersion;
        let mut version = windows::Win32::System::SystemInformation::OSVERSIONINFOW::default();
        // SAFETY: RtlGetVersion only fills the struct it is handed.
        let ok = unsafe { RtlGetVersion(&mut version) }.is_ok();
        if ok && version.dwBuildNumber >= 22000 {
            return "Segoe Fluent Icons";
        }
    }
    "Segoe MDL2 Assets"
}

#[derive(Clone, Copy)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn caption(kind: Caption, theme: &Theme) -> impl IntoElement {
    let (id, glyph, area) = match kind {
        Caption::Minimize => ("caption-min", "\u{e921}", WindowControlArea::Min),
        Caption::Maximize => ("caption-max", "\u{e922}", WindowControlArea::Max),
        Caption::Restore => ("caption-restore", "\u{e923}", WindowControlArea::Max),
        Caption::Close => ("caption-close", "\u{e8bb}", WindowControlArea::Close),
    };
    // The system's own red for closing.
    let red: Hsla = Rgba {
        r: 0xc4 as f32 / 255.0,
        g: 0x2b as f32 / 255.0,
        b: 0x1c as f32 / 255.0,
        a: 1.0,
    }
    .into();
    let (hover, pressed) = match kind {
        Caption::Close => (red, red.opacity(0.9)),
        _ => (theme.surface_hover, theme.line),
    };
    let close = matches!(kind, Caption::Close);
    div()
        .id(id)
        .occlude()
        .w(px(46.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .font_family(glyph_font())
        .text_size(px(10.0))
        .text_color(theme.text)
        .hover(move |s| {
            let s = s.bg(hover);
            if close {
                s.text_color(gpui::white())
            } else {
                s
            }
        })
        .active(move |s| s.bg(pressed))
        .window_control_area(area)
        .child(glyph)
}

/// The strip over the page: what `extra` holds (the update button), then
/// the caption buttons.
pub fn bar(extra: impl IntoElement, theme: &Theme, window: &Window) -> impl IntoElement {
    let max = if window.is_maximized() {
        Caption::Restore
    } else {
        Caption::Maximize
    };
    div()
        .id("titlebar")
        .h(px(HEIGHT))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_end()
        .window_control_area(WindowControlArea::Drag)
        .child(div().flex().items_center().pr(px(8.0)).child(extra))
        .child(
            div()
                .h_full()
                .flex()
                .child(caption(Caption::Minimize, theme))
                .child(caption(max, theme))
                .child(caption(Caption::Close, theme)),
        )
}
