//! A slider that knows its factory default, after `src/lib/ui/Slider.svelte`:
//! a double-click, or the reset mark that appears once the value drifts,
//! snaps it back. Dragging, a click on the track, the arrow keys (Shift for
//! bigger steps), Home and End all move it; screen readers can step it.

use crate::theme::{Theme, size};
use crate::ui::{icon, pressable};
use gpui::{
    AccessibleAction, App, Bounds, ElementId, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    Pixels, Role, SharedString, Window, canvas, div, prelude::*, px,
};
use std::rc::Rc;

type OnChange = Rc<dyn Fn(f32, &mut Window, &mut App)>;

/// What is being dragged: the slider's id, so a drag only moves its own.
#[derive(Clone)]
struct Dragged(ElementId);

/// The track's place on screen, from the last frame, for a click on it.
#[derive(Default)]
struct Track(Option<Bounds<Pixels>>);

/// `v` on the track, on a step.
fn snap_to(v: f32, min: f32, max: f32, step: f32) -> f32 {
    let v = v.clamp(min, max);
    if step > 0.0 {
        let n = ((v - min) / step).round();
        (min + n * step).clamp(min, max)
    } else {
        v
    }
}

pub struct Slider {
    id: ElementId,
    label: SharedString,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    default: Option<f32>,
    format: Rc<dyn Fn(f32) -> String>,
    ends: Option<(SharedString, SharedString)>,
    reset_label: SharedString,
    on_change: OnChange,
}

/// A slider for `value` between `min` and `max`, in `step`s.
pub fn slider(
    id: impl Into<ElementId>,
    label: SharedString,
    value: f32,
    (min, max, step): (f32, f32, f32),
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Slider {
    Slider {
        id: id.into(),
        label,
        value,
        min,
        max,
        step,
        default: None,
        format: Rc::new(|v| format!("{v}")),
        ends: None,
        reset_label: SharedString::default(),
        on_change: Rc::new(on_change),
    }
}

impl Slider {
    /// The factory default a double-click goes back to, and the words of
    /// the reset mark.
    pub fn default_value(mut self, default: f32, reset_label: SharedString) -> Self {
        self.default = Some(default);
        self.reset_label = reset_label;
        self
    }

    /// How the value reads beside the track.
    pub fn format(mut self, format: impl Fn(f32) -> String + 'static) -> Self {
        self.format = Rc::new(format);
        self
    }

    /// Words under the two ends of the track.
    pub fn ends(mut self, low: SharedString, high: SharedString) -> Self {
        self.ends = Some((low, high));
        self
    }

    pub fn render(self, theme: &Theme, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Slider {
            id,
            label,
            value,
            min,
            max,
            step,
            default,
            format,
            ends,
            reset_label,
            on_change,
        } = self;
        let snap = Rc::new(move |v: f32| snap_to(v, min, max, step));
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
        let is_default = default.is_none_or(|d| (value - d).abs() < step / 2.0);

        // A position on the track to a value.
        let at = {
            let snap = snap.clone();
            move |x: Pixels, bounds: Bounds<Pixels>| {
                let w = f32::from(bounds.size.width).max(1.0);
                let f = (f32::from(x - bounds.origin.x) / w).clamp(0.0, 1.0);
                snap(min + f * (max - min))
            }
        };

        let change = {
            let on_change = on_change.clone();
            move |v: f32, window: &mut Window, cx: &mut App| {
                if (v - value).abs() > f32::EPSILON {
                    on_change(v, window, cx);
                }
            }
        };

        let thumb = 18.0;
        let bar = {
            let track_store = track.clone();
            let at_click = at.clone();
            let change_click = change.clone();
            let at_drag = at.clone();
            let change_drag = change.clone();
            let drag_id = id.clone();
            let reset = on_change.clone();
            div()
                .id(ElementId::Name(SharedString::from(format!("{id}-bar"))))
                .relative()
                // Full width of its column, never squeezed below its height:
                // `flex_1` would give it a zero basis there.
                .w_full()
                .flex_none()
                .h(px(thumb))
                .cursor_pointer()
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            track_store.update(cx, |t, _| t.0 = Some(bounds));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                // The track, filled up to the value.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px((thumb - 4.0) / 2.0))
                        .h(px(4.0))
                        .rounded_full()
                        .bg(theme.line)
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(fill))
                                .rounded_full()
                                .bg(theme.text_2),
                        ),
                )
                // The thumb, centred on the value.
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
                    if e.click_count >= 2 {
                        if let Some(d) = default {
                            reset(d, window, cx);
                        }
                        return;
                    }
                    if let Some(bounds) = track.read(cx).0 {
                        change_click(at_click(e.position.x, bounds), window, cx);
                    }
                })
                .on_drag(Dragged(id.clone()), |_, _, _, cx| cx.new(|_| gpui::Empty))
                .on_drag_move::<Dragged>(move |e, window, cx| {
                    if e.drag(cx).0 == drag_id {
                        change_drag(at_drag(e.event.position.x, e.bounds), window, cx);
                    }
                })
        };

        let keys = {
            let change = change.clone();
            let snap = snap.clone();
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
        let increment = {
            let change = change.clone();
            let snap = snap.clone();
            move |_: Option<&gpui::accesskit::ActionData>, window: &mut Window, cx: &mut App| {
                change(snap(value + step), window, cx)
            }
        };
        let decrement = {
            let change = change.clone();
            move |_: Option<&gpui::accesskit::ActionData>, window: &mut Window, cx: &mut App| {
                change(snap(value - step), window, cx)
            }
        };

        let shown = SharedString::from(format(value));
        let control = pressable(id.clone(), theme)
            .role(Role::Slider)
            .aria_label(label.clone())
            .aria_numeric_value(value as f64)
            .aria_min_numeric_value(min as f64)
            .aria_max_numeric_value(max as f64)
            .aria_numeric_value_step(step as f64)
            .aria_value(shown.clone())
            .on_a11y_action(AccessibleAction::Increment, increment)
            .on_a11y_action(AccessibleAction::Decrement, decrement)
            .on_key_down(keys)
            .flex_1()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(2.0))
            .rounded(px(size::R_SM))
            .cursor_default()
            .child(bar)
            .when_some(ends, |d, (low, high)| {
                d.child(
                    div()
                        .flex()
                        .justify_between()
                        .text_size(px(size::T_XS))
                        .text_color(theme.text_3)
                        .child(low)
                        .child(high),
                )
            });

        div()
            .flex()
            .items_center()
            .gap(px(size::S3))
            .min_w(px(160.0))
            .child(control)
            .child(
                div()
                    .min_w(px(44.0))
                    .text_size(px(size::T_SM))
                    .text_color(theme.text_2)
                    .font_weight(FontWeight::MEDIUM)
                    .child(shown),
            )
            .when(!is_default, |d| {
                let reset = on_change.clone();
                let target = default.unwrap_or(value);
                d.child(
                    pressable(
                        ElementId::Name(SharedString::from(format!("{id}-reset"))),
                        theme,
                    )
                    .role(Role::Button)
                    .aria_label(reset_label.clone())
                    .tooltip(crate::ui::tip(reset_label))
                    .p(px(3.0))
                    .rounded(px(size::R_SM))
                    .hover(|s| s.bg(theme.surface_hover))
                    .child(icon("reset", 12.0, false, theme.text_3))
                    .on_click(move |_, window, cx| reset(target, window, cx)),
                )
            })
    }
}
