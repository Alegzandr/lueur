//! The Lighting page's own pieces: its buttons in their four kinds and two
//! sizes, the room's framed photographs, the lamp dot, a light's disc, the
//! zone badge, and a slider whose track is a gradient (the warmth and the
//! hue).

use super::model::{Room, kelvin_rgb, tint_rgb};
use crate::theme::{Theme, oklch, size};
use crate::ui::pressable;
use gpui::{
    AccessibleAction, AnyElement, App, Bounds, BoxShadow, Corners, Div, ElementId, FontWeight,
    Hsla, ImgResourceLoader, IntoElement, KeyDownEvent, MouseButton, Pixels, Resource, Rgba, Role,
    SharedString, Stateful, Window, canvas, div, linear_color_stop, linear_gradient, point,
    prelude::*, px,
};
use std::cell::Cell;
use std::rc::Rc;

/// A colour from its 0..=255 channels.
pub fn rgb8([r, g, b]: [u8; 3], a: f32) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a,
    }
    .into()
}

/// What a button looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Raised, near white: most buttons.
    Default,
    /// Ink: the one a part of the page leads with.
    Primary,
    /// Flat, in the second ink: the quiet ones.
    Plain,
    /// Raised, its words in the colour of what cannot be undone.
    Danger,
}

/// A button of `kind`, 28px high or 24px when `small`. Disabled, it stays
/// where the keyboard can find it but does nothing and reads fainter. The
/// caller adds its content, its name and what a click does.
pub fn btn(
    id: impl Into<ElementId>,
    kind: Kind,
    small: bool,
    disabled: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let (bg, hover, fg) = match kind {
        Kind::Primary => (theme.ink, theme.ink_hover, theme.on_ink),
        Kind::Plain => (gpui::transparent_black(), theme.surface_hover, theme.text_2),
        Kind::Danger => (theme.surface, theme.danger_soft, theme.danger),
        Kind::Default => (theme.surface, theme.surface_hover, theme.text),
    };
    let fg = if disabled && kind != Kind::Primary {
        theme.text_3
    } else {
        fg
    };
    pressable(id, theme)
        .role(Role::Button)
        .flex_none()
        .h(px(if small { 24.0 } else { 28.0 }))
        .px(px(if small { size::S2 } else { size::S3 }))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(if small { 6.0 } else { 7.0 }))
        .bg(bg)
        .when(kind == Kind::Default || kind == Kind::Danger, |d| {
            d.border_1().border_color(theme.line).shadow_xs()
        })
        .when(disabled, |d| {
            d.cursor_default()
                .when(kind == Kind::Primary, |d| d.opacity(0.5))
        })
        .when(!disabled, |d| d.hover(move |s| s.bg(hover)))
        .text_color(fg)
        .text_size(px(size::T_MD))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
}

/// A section's heading on the page, bold in the text's own ink, as the
/// Svelte page's `.group-title`.
pub fn group_title(text: SharedString, theme: &Theme) -> Div {
    div()
        .text_size(px(size::T_LG))
        .font_weight(FontWeight::BOLD)
        .text_color(theme.text)
        .child(text)
}

/// A lit dot: something giving light now, or waiting on you, in the one
/// colour the chrome allows, with a glow of its own size.
pub fn lamp(diameter: f32, theme: &Theme) -> Div {
    div()
        .flex_none()
        .size(px(diameter))
        .rounded_full()
        .bg(theme.active)
        .shadow(vec![
            BoxShadow::new(px(0.0), px(0.0), theme.glow.opacity(0.6)).blur_radius(px(diameter)),
        ])
}

/// The two ends of a mode's swatch: a picture's amber, violet and blue for
/// the screen, each show's own colours.
fn swatch(mode: &str) -> ([u8; 3], [u8; 3]) {
    match mode {
        "music" => ([0xff, 0x00, 0x50], [0x00, 0x78, 0xff]),
        "rainbow" => ([0xff, 0x96, 0x00], [0x1e, 0x32, 0xff]),
        "sunset" => ([0xff, 0x8c, 0x0a], [0xbe, 0x1e, 0xb4]),
        "ocean" => ([0x14, 0x23, 0xff], [0x00, 0xeb, 0xaf]),
        "candle" => ([0xff, 0xc4, 0x6a], [0xff, 0x4e, 0x04]),
        _ => ([0xd9, 0x7a, 0x2b], [0x3b, 0x7b, 0xe0]),
    }
}

/// A light as it glows now, drawn round: its white or colour, fainter as it
/// dims, the gradient of the effect it plays, a hollow ring while off, a
/// dashed one with no bulb. What a bulb gives is content, so its real colour
/// belongs here. `edge` rings it with the page's ground, for a stack.
pub fn light_dot(room: &Room, id: &str, diameter: f32, edge: bool, theme: &Theme) -> Div {
    let bound = room.bound(id);
    let on = bound && room.on(id);
    let hairline = oklch(0.0, 0.0, 0.0, 0.12);
    let dot = div().flex_none().size(px(diameter)).rounded_full();
    let dot = if !bound {
        dot.border_1()
            .border_dashed()
            .border_color(theme.line_strong)
    } else if !on {
        dot.border_1().border_color(theme.line_strong)
    } else if room.driven(id) {
        let (a, b) = swatch(room.mode_of(id).unwrap_or("ambilight"));
        dot.bg(linear_gradient(
            135.0,
            linear_color_stop(rgb8(a, 1.0), 0.0),
            linear_color_stop(rgb8(b, 1.0), 1.0),
        ))
        .border_1()
        .border_color(hairline)
        .shadow(vec![
            BoxShadow::new(px(0.0), px(0.0), theme.active.opacity(0.45))
                .blur_radius(px(diameter * 0.45)),
        ])
    } else {
        let s = room.state(id);
        let c = tint_rgb(s.tint);
        let strength = (40.0 + 0.6 * f32::from(s.bright)) / 100.0;
        dot.bg(rgb8(c, strength))
            .border_1()
            .border_color(hairline)
            .shadow(vec![
                BoxShadow::new(px(0.0), px(0.0), rgb8(c, 0.45)).blur_radius(px(diameter * 0.45)),
            ])
    };
    if edge {
        div()
            .flex_none()
            .p(px(2.0))
            .rounded_full()
            .bg(theme.bg)
            .child(dot)
    } else {
        dot
    }
}

/// The mark of a screen zone: its colour with its letter on it, so the zone
/// reads without telling colours apart.
pub fn zone_badge(
    edge: lueur_core::geometry::ScreenEdge,
    letter: SharedString,
    theme: &Theme,
) -> Div {
    use lueur_core::geometry::ScreenEdge;
    let c = match (edge, theme.dark) {
        (ScreenEdge::Left, false) => oklch(0.42, 0.14, 262.0, 1.0),
        (ScreenEdge::Center, false) => oklch(0.51, 0.1, 175.0, 1.0),
        (ScreenEdge::Right, false) => oklch(0.625, 0.145, 58.0, 1.0),
        (ScreenEdge::Left, true) => oklch(0.64, 0.13, 252.0, 1.0),
        (ScreenEdge::Center, true) => oklch(0.74, 0.12, 175.0, 1.0),
        (ScreenEdge::Right, true) => oklch(0.87, 0.13, 80.0, 1.0),
    };
    let ink = if theme.dark {
        oklch(0.2, 0.0, 0.0, 1.0)
    } else {
        oklch(1.0, 0.0, 0.0, 1.0)
    };
    div()
        .flex_none()
        .size(px(15.0))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(c)
        .text_color(ink)
        .text_size(px(9.5))
        .font_weight(FontWeight::BOLD)
        .child(letter)
}

/// One of the room's photographs, framed: it covers its box (the caller
/// sizes it), held a little above its middle, with 6px corners and a
/// hairline edge. `zoom` brings it closer from its middle (shared, so an
/// animation can move it); `tint` lays the colour of a light set by hand
/// over the neutral room.
pub fn picture(path: &'static str, zoom: Rc<Cell<f32>>, tint: Option<[u8; 3]>, radius: f32) -> Div {
    div()
        .relative()
        .overflow_hidden()
        .rounded(px(radius))
        .child(
            photo(path, zoom, 0.42, (0.0, 0.0), radius)
                .absolute()
                .size_full(),
        )
        // A white or colour set by hand: the neutral room takes its hue.
        // GPUI has no colour blend mode, so the colour is laid over it,
        // half seen.
        .when_some(tint, |d, c| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(radius))
                    .bg(rgb8(c, 0.42)),
            )
        })
        // A hairline keeps a bright picture's edge on a white card.
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(radius))
                .border_1()
                .border_color(oklch(0.0, 0.0, 0.0, 0.14)),
        )
}

/// A photograph drawn to cover its box: brought `zoom` times closer, held
/// at `focus_y` of its height (0 its top, 1 its bottom), moved by `offset`
/// pixels, its corners rounded by `radius`.
pub fn photo(
    path: &'static str,
    zoom: Rc<Cell<f32>>,
    focus_y: f32,
    offset: (f32, f32),
    radius: f32,
) -> gpui::Canvas<Option<std::sync::Arc<gpui::RenderImage>>> {
    let source = Resource::Embedded(SharedString::new_static(path));
    canvas(
        move |_, window, cx| match window.use_asset::<ImgResourceLoader>(&source, cx) {
            Some(Ok(image)) => Some(image),
            _ => None,
        },
        move |bounds, image, window, _| {
            let Some(image) = image else { return };
            let frame = image.size(0);
            let (iw, ih) = (
                u32::from(frame.width) as f32,
                u32::from(frame.height) as f32,
            );
            if iw <= 0.0 || ih <= 0.0 {
                return;
            }
            let (bw, bh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let scale = (bw / iw).max(bh / ih) * zoom.get().max(1.0);
            let (w, h) = (iw * scale, ih * scale);
            let at = Bounds {
                origin: point(
                    bounds.origin.x + px((bw - w) * 0.5 + offset.0),
                    bounds.origin.y + px((bh - h) * focus_y + offset.1),
                ),
                size: gpui::size(px(w), px(h)),
            };
            let _ = window.paint_image(bounds, at, Corners::all(px(radius)), image, 0, false);
        },
    )
}

/// The thumb's place on the track, from the last frame, for a click on it.
#[derive(Default)]
struct Track(Option<Bounds<Pixels>>);

/// What is being dragged: the slider's id, so a drag only moves its own.
#[derive(Clone)]
struct Dragged(ElementId);

type OnChange = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// A slider whose track shows what it sets: the warmth from warm to cool
/// white, the hue round the wheel. Dragging, a click on the track, the
/// arrows (Shift for bigger steps), Home and End all move it.
#[allow(clippy::too_many_arguments)]
pub fn gradient_slider(
    id: impl Into<ElementId>,
    label: SharedString,
    value: f32,
    (min, max, step): (f32, f32, f32),
    stops: Vec<Hsla>,
    shown: SharedString,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: ElementId = id.into();
    let on_change: OnChange = Rc::new(on_change);
    let snap = move |v: f32| {
        let v = v.clamp(min, max);
        (min + ((v - min) / step).round() * step).clamp(min, max)
    };
    let change = {
        let on_change = on_change.clone();
        move |v: f32, window: &mut Window, cx: &mut App| {
            if (v - value).abs() > f32::EPSILON {
                on_change(v, window, cx);
            }
        }
    };
    let at = move |x: Pixels, bounds: Bounds<Pixels>| {
        let w = f32::from(bounds.size.width).max(1.0);
        let f = (f32::from(x - bounds.origin.x) / w).clamp(0.0, 1.0);
        snap(min + f * (max - min))
    };
    let track = window.use_keyed_state(
        ElementId::Name(SharedString::from(format!("{id}-track"))),
        cx,
        |_, _| Track::default(),
    );
    let fill = if max > min {
        (value - min) / (max - min)
    } else {
        0.0
    };
    let thumb = 18.0;
    let n = stops.len().saturating_sub(1).max(1);
    let segments = stops.windows(2).map(|pair| {
        div()
            .h_full()
            .w(gpui::relative(1.0 / n as f32))
            .bg(linear_gradient(
                90.0,
                linear_color_stop(pair[0], 0.0),
                linear_color_stop(pair[1], 1.0),
            ))
    });
    let store = track.clone();
    let drag_id = id.clone();
    let change_click = change.clone();
    let change_drag = change.clone();
    let bar = div()
        .id(ElementId::Name(SharedString::from(format!("{id}-bar"))))
        .relative()
        .w_full()
        .h(px(thumb))
        .cursor_pointer()
        .child(
            canvas(
                move |bounds, _, cx| store.update(cx, |t, _| t.0 = Some(bounds)),
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px((thumb - 8.0) / 2.0))
                .h(px(8.0))
                .rounded_full()
                .overflow_hidden()
                .flex()
                .children(segments),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px((thumb - 8.0) / 2.0))
                .h(px(8.0))
                .rounded_full()
                .border_1()
                .border_color(oklch(0.0, 0.0, 0.0, 0.12)),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .left(gpui::relative(fill))
                .ml(px(-thumb / 2.0))
                .size(px(thumb))
                .rounded_full()
                .bg(gpui::white())
                .border_1()
                .border_color(theme.line_strong)
                .shadow_sm(),
        )
        .on_mouse_down(MouseButton::Left, move |e, window, cx| {
            if let Some(bounds) = track.read(cx).0 {
                change_click(at(e.position.x, bounds), window, cx);
            }
        })
        .on_drag(Dragged(id.clone()), |_, _, _, cx| cx.new(|_| gpui::Empty))
        .on_drag_move::<Dragged>(move |e, window, cx| {
            if e.drag(cx).0 == drag_id {
                change_drag(at(e.event.position.x, e.bounds), window, cx);
            }
        });

    let keys = {
        let change = change.clone();
        move |e: &KeyDownEvent, window: &mut Window, cx: &mut App| {
            let big = (max - min) / 10.0;
            let by = if e.keystroke.modifiers.shift {
                big.max(step)
            } else {
                step
            };
            let to = match e.keystroke.key.as_str() {
                "right" | "up" => value + by,
                "left" | "down" => value - by,
                "pageup" => value + big,
                "pagedown" => value - big,
                "home" => min,
                "end" => max,
                _ => return,
            };
            cx.stop_propagation();
            change(snap(to), window, cx);
        }
    };
    let up = {
        let change = change.clone();
        move |_: Option<&gpui::accesskit::ActionData>, window: &mut Window, cx: &mut App| {
            change(snap(value + step), window, cx)
        }
    };
    let down = move |_: Option<&gpui::accesskit::ActionData>, window: &mut Window, cx: &mut App| {
        change(snap(value - step), window, cx)
    };
    pressable(id, theme)
        .role(Role::Slider)
        .aria_label(label)
        .aria_numeric_value(f64::from(value))
        .aria_min_numeric_value(f64::from(min))
        .aria_max_numeric_value(f64::from(max))
        .aria_numeric_value_step(f64::from(step))
        .aria_value(shown)
        .on_a11y_action(AccessibleAction::Increment, up)
        .on_a11y_action(AccessibleAction::Decrement, down)
        .on_key_down(keys)
        .w_full()
        .p(px(2.0))
        .rounded(px(size::R_SM))
        .cursor_default()
        .child(bar)
        .into_any_element()
}

/// The warm to cool whites of the Warmth slider, at its two ends and its
/// middle.
pub fn warmth_stops() -> Vec<Hsla> {
    [super::model::MIN_KELVIN, 4000, super::model::MAX_KELVIN]
        .into_iter()
        .map(|k| rgb8(kelvin_rgb(k), 1.0))
        .collect()
}

/// The wheel of hues, red round to red.
pub fn hue_stops() -> Vec<Hsla> {
    [0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0]
        .into_iter()
        .map(|h| rgb8(super::model::hue_rgb(h), 1.0))
        .collect()
}
