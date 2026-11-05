//! Choosing one value among a few: the segmented control (a radio group)
//! and the select (a raised button that opens its list).

use crate::theme::{Theme, size};
use crate::ui::{icon, pressable};
use gpui::{
    AnyElement, App, ElementId, FontWeight, IntoElement, KeyDownEvent, Role, SharedString, Window,
    anchored, deferred, div, prelude::*, px,
};
use std::rc::Rc;

/// One choice: its value, its words, and a tip if the words need one.
#[derive(Clone)]
pub struct Choice<T> {
    pub value: T,
    pub label: SharedString,
    pub tip: Option<SharedString>,
}

pub fn choice<T>(value: T, label: SharedString) -> Choice<T> {
    Choice {
        value,
        label,
        tip: None,
    }
}

impl<T> Choice<T> {
    pub fn tip(mut self, tip: SharedString) -> Self {
        self.tip = Some(tip);
        self
    }
}

/// The index after `from` for an arrow, Home or End, wrapping.
fn moved(key: &str, from: usize, len: usize) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match key {
        "right" | "down" => {
            if from >= last {
                0
            } else {
                from + 1
            }
        }
        "left" | "up" => {
            if from == 0 {
                last
            } else {
                from - 1
            }
        }
        "home" => 0,
        "end" => last,
        _ => return None,
    })
}

/// Equal segments in a well, the chosen one raised. Tab lands on the group;
/// the arrows move the choice.
pub fn segmented<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    label: SharedString,
    value: Option<&T>,
    options: Vec<Choice<T>>,
    theme: &Theme,
    on_change: impl Fn(T, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: ElementId = id.into();
    let on_change = Rc::new(on_change);
    let index = options.iter().position(|o| Some(&o.value) == value);
    let values: Vec<T> = options.iter().map(|o| o.value.clone()).collect();
    let keys = {
        let on_change = on_change.clone();
        move |e: &KeyDownEvent, window: &mut Window, cx: &mut App| {
            if let Some(to) = moved(&e.keystroke.key, index.unwrap_or(0), values.len()) {
                cx.stop_propagation();
                on_change(values[to].clone(), window, cx);
            }
        }
    };
    let n = options.len();
    let segments = options.into_iter().enumerate().map(|(i, o)| {
        let chosen = Some(i) == index;
        let on_change = on_change.clone();
        let value = o.value.clone();
        div()
            .id(ElementId::NamedInteger(
                SharedString::from(format!("{id}-opt")),
                i as u64,
            ))
            .role(Role::RadioButton)
            .aria_label(o.label.clone())
            .aria_selected(chosen)
            .aria_position_in_set(i + 1)
            .aria_size_of_set(n)
            .when_some(o.tip.clone(), |d, tip| d.tooltip(crate::ui::tip(tip)))
            .flex_1()
            .h(px(24.0))
            .px(px(size::S2))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .cursor_pointer()
            .text_size(px(size::T_SM))
            .when(chosen, |d| {
                d.bg(theme.surface)
                    .text_color(theme.text)
                    .font_weight(FontWeight::MEDIUM)
                    .shadow_sm()
            })
            .when(!chosen, |d| {
                d.text_color(theme.text_2)
                    .hover(|s| s.text_color(theme.text))
            })
            .child(o.label)
            .on_click(move |_, window, cx| on_change(value.clone(), window, cx))
    });
    pressable(id.clone(), theme)
        .role(Role::RadioGroup)
        .aria_label(label)
        .on_key_down(keys)
        .flex()
        .p(px(2.0))
        .gap(px(2.0))
        .rounded(px(size::R_MD))
        .bg(theme.surface_hover)
        .cursor_default()
        .children(segments)
        .into_any_element()
}

/// Whether a select's list is open.
#[derive(Default)]
struct Open(bool);

#[allow(clippy::too_many_arguments)]
/// A raised button naming the choice, which opens the list of the others.
/// Closed, the arrows step through them as a native select does.
pub fn select<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    label: SharedString,
    value: Option<&T>,
    options: Vec<Choice<T>>,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
    on_change: impl Fn(T, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: ElementId = id.into();
    let open = window.use_keyed_state(
        ElementId::Name(SharedString::from(format!("{id}-open"))),
        cx,
        |_, _| Open(false),
    );
    let is_open = open.read(cx).0;
    let on_change = Rc::new(on_change);
    let index = options.iter().position(|o| Some(&o.value) == value);
    let current = index.map(|i| options[i].label.clone()).unwrap_or_default();
    let values: Vec<T> = options.iter().map(|o| o.value.clone()).collect();

    let keys = {
        let on_change = on_change.clone();
        let open = open.clone();
        move |e: &KeyDownEvent, window: &mut Window, cx: &mut App| {
            if e.keystroke.key == "escape" {
                open.update(cx, |o, cx| {
                    o.0 = false;
                    cx.notify();
                });
                return;
            }
            if let Some(to) = moved(&e.keystroke.key, index.unwrap_or(0), values.len()) {
                cx.stop_propagation();
                on_change(values[to].clone(), window, cx);
            }
        }
    };

    let toggle = {
        let open = open.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            open.update(cx, |o, cx| {
                o.0 = !o.0;
                cx.notify();
            })
        }
    };

    let button = pressable(id.clone(), theme)
        .role(Role::ComboBox)
        .aria_label(label)
        .aria_value(current.clone())
        .aria_expanded(is_open)
        .on_key_down(keys)
        .h(px(28.0))
        .pl(px(size::S3))
        .pr(px(size::S2))
        .flex()
        .items_center()
        .gap(px(size::S2))
        .rounded(px(7.0))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.line)
        .hover(|s| s.bg(theme.surface_hover))
        .text_size(px(size::T_MD))
        .text_color(theme.text)
        .child(div().flex_1().truncate().child(current))
        .child(icon("chevron", 12.0, false, theme.text_3))
        .on_click(toggle);

    let list = is_open.then(|| {
        let items = options.into_iter().enumerate().map(|(i, o)| {
            let chosen = Some(i) == index;
            let on_change = on_change.clone();
            let open = open.clone();
            let value = o.value.clone();
            pressable(
                ElementId::NamedInteger(SharedString::from(format!("{id}-item")), i as u64),
                theme,
            )
            .role(Role::ListBoxOption)
            .aria_label(o.label.clone())
            .aria_selected(chosen)
            .h(px(28.0))
            .px(px(size::S3))
            .flex()
            .items_center()
            .gap(px(size::S2))
            .rounded(px(size::R_SM))
            .hover(|s| s.bg(theme.surface_hover))
            .text_size(px(size::T_MD))
            .text_color(theme.text)
            .child(div().w(px(14.0)).when(chosen, |d| {
                d.child(icon("check", 12.0, false, theme.select))
            }))
            .child(o.label)
            .on_click(move |_, window, cx| {
                open.update(cx, |o, cx| {
                    o.0 = false;
                    cx.notify();
                });
                on_change(value.clone(), window, cx);
            })
        });
        let close = open.clone();
        deferred(
            anchored().snap_to_window().child(
                div()
                    .id(ElementId::Name(SharedString::from(format!("{id}-list"))))
                    .role(Role::ListBox)
                    .mt(px(4.0))
                    .min_w(px(180.0))
                    .max_h(px(320.0))
                    .overflow_y_scroll()
                    .p(px(4.0))
                    .rounded(px(size::R_MD))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.line)
                    .shadow_lg()
                    .on_mouse_down_out(move |_, _, cx| {
                        close.update(cx, |o, cx| {
                            o.0 = false;
                            cx.notify();
                        })
                    })
                    .children(items),
            ),
        )
        .with_priority(1)
    });

    div()
        .flex()
        .flex_col()
        .child(button)
        .children(list)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::moved;

    #[test]
    fn the_arrows_wrap_and_home_and_end_jump() {
        assert_eq!(moved("right", 2, 3), Some(0));
        assert_eq!(moved("left", 0, 3), Some(2));
        assert_eq!(moved("end", 0, 3), Some(2));
        assert_eq!(moved("a", 0, 3), None);
        assert_eq!(moved("right", 0, 0), None);
    }
}
