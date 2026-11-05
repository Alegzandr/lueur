//! The Bulbs page's own controls: buttons in the sizes and looks DESIGN.md
//! gives them (default, small, plain, icon, danger, disabled), the
//! spinner, a light's disc, a disclosure, and a group's heading.

use crate::state::Lueur;
use crate::theme::{Theme, size};
use crate::ui;
use gpui::{
    Animation, AnimationExt, AnyElement, App, BoxShadow, ClickEvent, Div, ElementId, FocusHandle,
    FontWeight, Hsla, Rgba, Role, SharedString, Stateful, Transformation, Window, div,
    linear_color_stop, linear_gradient, percentage, point, prelude::*, px,
};
use lueur_core::device::common::kelvin_to_rgb;
use lueur_core::scene::{LightState, Tint};
use std::time::Duration;

/// How a button looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Default,
    /// The ink one a page leads with, one at most.
    Primary,
    /// Transparent until pointed at.
    Plain,
    /// A destructive action, plain with red words and a red wash when
    /// pointed at.
    Danger,
}

/// A button. Disabled, it leaves the Tab order and greys its words, as a
/// disabled `<button>` does; `on_click` is then never called.
#[allow(clippy::too_many_arguments)]
pub fn btn(
    id: impl Into<ElementId>,
    label: SharedString,
    look: Look,
    small: bool,
    disabled: bool,
    lead: Option<AnyElement>,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    shell(id, look, small, disabled, theme)
        .aria_label(label.clone())
        .px(px(if small { size::S2 } else { size::S3 }))
        .children(lead)
        .child(label)
        .when(!disabled, |d| d.on_click(on_click))
}

/// A square button holding a glyph, named for screen readers and in its tip.
pub fn icon_btn(
    id: impl Into<ElementId>,
    glyph: &str,
    label: SharedString,
    tip: SharedString,
    look: Look,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let color = if look == Look::Danger {
        theme.danger
    } else {
        theme.text_2
    };
    shell(id, look, true, false, theme)
        .aria_label(label)
        .tooltip(ui::tip(tip))
        .w(px(24.0))
        .child(ui::icon(glyph, 14.0, false, color))
        .on_click(on_click)
}

/// What every button shares: the size, the corner, the colours of its look.
fn shell(
    id: impl Into<ElementId>,
    look: Look,
    small: bool,
    disabled: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let (bg, hover, fg) = match look {
        Look::Primary => (theme.ink, theme.ink_hover, theme.on_ink),
        Look::Plain => (gpui::transparent_black(), theme.surface_hover, theme.text_2),
        Look::Default => (theme.surface, theme.surface_hover, theme.text),
        Look::Danger => (gpui::transparent_black(), theme.danger_soft, theme.danger),
    };
    let base = if disabled {
        div().id(id)
    } else {
        ui::pressable(id, theme)
    };
    base.role(Role::Button)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .h(px(if small { 24.0 } else { 28.0 }))
        .rounded(px(if small { 6.0 } else { 7.0 }))
        .text_size(px(if small { size::T_SM } else { size::T_MD }))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .bg(bg)
        .when(look == Look::Default, |d| {
            d.border_1().border_color(theme.line)
        })
        .text_color(if disabled { theme.text_3 } else { fg })
        .when(disabled && look == Look::Primary, |d| d.opacity(0.55))
        .when(!disabled, |d| {
            d.hover(move |s| {
                let s = s.bg(hover);
                if look == Look::Plain {
                    s.text_color(theme.text)
                } else {
                    s
                }
            })
        })
}

/// "Working": a glyph turning. It turns for as long as the work goes on, so
/// it never stops to look finished while it is not.
pub fn spinner(
    id: impl Into<ElementId>,
    px_size: f32,
    color: Hsla,
    reduce_motion: bool,
) -> AnyElement {
    let glyph = ui::icon("refresh", px_size, false, color);
    // Under reduced motion the glyph stands still: the words beside it say
    // that something is under way.
    if reduce_motion {
        return glyph.into_any_element();
    }
    glyph
        .with_animation(
            id,
            Animation::new(Duration::from_millis(900)).repeat(),
            |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
}

/// A section's heading, a level-2 heading for screen readers, with what
/// stands on its right. `focus` lets the page move the keyboard to it.
pub fn group_head(
    id: impl Into<ElementId>,
    title: SharedString,
    focus: Option<&FocusHandle>,
    right: Option<AnyElement>,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(size::S3))
        .min_h(px(24.0))
        .px(px(size::S1))
        .pb(px(size::S2))
        .child(
            div()
                .id(id)
                .role(Role::Heading)
                .aria_level(2)
                .aria_label(title.clone())
                .when_some(focus, |d, f| d.track_focus(f))
                .text_size(px(size::T_LG))
                .font_weight(FontWeight::BOLD)
                .text_color(theme.text)
                .child(title),
        )
        .children(right)
}

/// A line of small grey text.
pub fn note(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(px(size::T_SM))
        .text_color(theme.text_3)
        .child(text.into())
}

/// A button that shows or hides what follows it, as `<details>` does.
pub fn disclosure(
    id: impl Into<ElementId>,
    label: SharedString,
    open: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    // The caret points right while closed and down once open.
    let chevron = ui::icon("chevron", 14.0, false, theme.text_3).with_transformation(
        Transformation::rotate(percentage(if open { 0.25 } else { 0.0 })),
    );
    ui::pressable(id, theme)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_expanded(open)
        .flex()
        .items_center()
        .gap(px(size::S2))
        .rounded(px(size::R_SM))
        .child(chevron)
        .child(label)
        .on_click(on_click)
}

fn rgb(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
    .into()
}

fn hex(v: u32) -> Hsla {
    rgb((v >> 16) as u8, (v >> 8) as u8, v as u8, 1.0)
}

/// The two ends of the swatch a mode is drawn with, as the Lighting page
/// draws them.
fn mode_swatch(mode: &str) -> (Hsla, Hsla) {
    match mode {
        // Amber to blue: a picture's colours, without the red of an error.
        "ambilight" => (
            crate::theme::oklch(0.66, 0.16, 55.0, 1.0),
            crate::theme::oklch(0.6, 0.16, 250.0, 1.0),
        ),
        "music" => (hex(0xff0050), hex(0x0078ff)),
        "rainbow" => (hex(0xff9600), hex(0x00ff6e)),
        "sunset" => (hex(0xff8c0a), hex(0xbe1eb4)),
        "ocean" => (hex(0x1423ff), hex(0x00ebaf)),
        "candle" => (hex(0xffc46a), hex(0xff4e04)),
        _ => (hex(0xffb070), hex(0xffb070)),
    }
}

/// A light as it glows now, drawn round: its white
/// or colour, fainter as it dims, the gradient of the effect it plays, a
/// hollow ring while off, a dashed outline with no bulb. What a bulb gives
/// is content, so its real colour belongs here.
pub fn light_dot(l: &Lueur, light_id: &str, px_size: f32, theme: &Theme) -> Div {
    let dot = div().size(px(px_size)).flex_none().rounded_full();
    if l.config.binding_for(light_id).is_none() {
        return dot
            .border_1()
            .border_dashed()
            .border_color(theme.line_strong);
    }
    let driven = l.status.running && l.status.light_ids.iter().any(|x| x == light_id);
    let state: LightState = l.config.states.get(light_id).copied().unwrap_or_default();
    if !driven && !state.on {
        return dot.border_1().border_color(theme.line_strong);
    }
    let halo = |c: Hsla| BoxShadow {
        color: c.opacity(0.45),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(px_size * 0.45),
        spread_radius: px(0.0),
        inset: false,
    };
    if driven {
        let mode = l
            .status
            .groups
            .iter()
            .find(|g| g.light_ids.iter().any(|x| x == light_id))
            .map(|g| g.mode)
            .unwrap_or("ambilight");
        let (from, to) = mode_swatch(mode);
        return dot
            .bg(linear_gradient(
                135.0,
                linear_color_stop(from, 0.0),
                linear_color_stop(to, 1.0),
            ))
            .shadow(vec![halo(theme.active)]);
    }
    let c = match state.tint {
        Tint::White { kelvin } => kelvin_to_rgb(kelvin),
        Tint::Color { r, g, b } => lueur_core::color::Rgb::new(r, g, b),
    };
    let alpha = (40.0 + 0.6 * state.bright as f32) / 100.0;
    // A hairline keeps a near-white bulb visible on a white card.
    dot.bg(rgb(c.r, c.g, c.b, alpha))
        .border_1()
        .border_color(rgb(0, 0, 0, 0.12))
        .shadow(vec![halo(rgb(c.r, c.g, c.b, 1.0))])
}
