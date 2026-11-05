//! Shared controls, after `src/lib/ui/`. Each is reachable with Tab, works
//! with Enter or Space, draws an amber focus ring only for the keyboard, and
//! tells assistive technology what it is and what state it is in.

use crate::theme::{Theme, size};
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, Hsla, IntoElement, Role, SharedString,
    Stateful, Svg, Window, div, prelude::*, px, svg,
};

/// A glyph from `assets/icons`, in the colour of the text around it unless
/// `color` says otherwise. Filled is for the one thing chosen among its
/// siblings (the current page in the sidebar); small glyphs are a step
/// heavier so their strokes keep their weight.
pub fn icon(name: &str, px_size: f32, filled: bool, color: Hsla) -> Svg {
    let weight = if filled {
        "fill"
    } else if px_size <= 12.0 {
        "bold"
    } else {
        "regular"
    };
    svg()
        .path(SharedString::from(format!("icons/{name}-{weight}.svg")))
        .size(px(px_size))
        .flex_none()
        .text_color(color)
}

/// The base of every pressable control: focusable, reachable with Tab, an
/// amber ring when the keyboard reaches it.
pub fn pressable(id: impl Into<ElementId>, theme: &Theme) -> Stateful<Div> {
    let focus = theme.focus;
    div()
        .id(id)
        .focusable()
        .tab_stop(true)
        .cursor_pointer()
        .focus_visible(move |s| s.border_2().border_color(focus))
}

/// A button: `primary` is the ink one a page leads with.
pub fn button(
    id: impl Into<ElementId>,
    label: SharedString,
    primary: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, hover, fg) = if primary {
        (theme.ink, theme.ink_hover, theme.on_ink)
    } else {
        (theme.surface, theme.surface_hover, theme.text)
    };
    pressable(id, theme)
        .role(Role::Button)
        .aria_label(label.clone())
        .h(px(32.0))
        .px(px(size::S4))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(size::S2))
        .rounded(px(size::R_MD))
        .bg(bg)
        .when(!primary, |d| d.border_1().border_color(theme.line))
        .hover(move |s| s.bg(hover))
        .text_color(fg)
        .text_size(px(size::T_MD))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
        .on_click(on_click)
}

/// An on/off switch with its name for screen readers. On is amber: a light
/// that is on is light. The state is also told by the knob's side.
pub fn switch(
    id: impl Into<ElementId>,
    label: SharedString,
    on: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let track = if on { theme.active } else { theme.surface_2 };
    pressable(id, theme)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .w(px(34.0))
        .h(px(20.0))
        .p(px(2.0))
        .flex()
        .when(on, |d| d.justify_end())
        .rounded_full()
        .bg(track)
        .when(!on, |d| d.border_1().border_color(theme.control_line))
        .child(
            div()
                .size(px(if on { 16.0 } else { 14.0 }))
                .rounded_full()
                .bg(gpui::white()),
        )
        .on_click(on_click)
}

/// A frosted card holding a group of rows.
pub fn card(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(size::R_LG))
        .bg(theme.material)
        .border_1()
        .border_color(theme.separator)
        .overflow_hidden()
}

/// A page's title, a heading for screen readers.
pub fn page_title(id: impl Into<ElementId>, text: SharedString, theme: &Theme) -> AnyElement {
    div()
        .id(id)
        .role(Role::Heading)
        .aria_level(1)
        .aria_label(text.clone())
        .text_size(px(size::T_XL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(text)
        .into_any_element()
}
