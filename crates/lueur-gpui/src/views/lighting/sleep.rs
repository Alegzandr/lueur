//! The sleep timer. It is for bedtime:
//! it puts the whole room out, whatever is picked (the tray sets it too,
//! with no pick to follow). An occasional tool, so it waits behind a clock
//! in the page's head and opens a small panel.

use super::LightingPage;
use super::controls::{Kind, btn};
use crate::i18n::{fill, t};
use crate::theme::{Theme, size};
use crate::ui::{self, InputEvent, TextInput};
use gpui::{
    AnyElement, AppContext, Context, Focusable, FontWeight, IntoElement, KeyDownEvent, Role,
    SharedString, Window, anchored, deferred, div, prelude::*, px,
};
use std::time::{Duration, Instant};

/// The durations offered at a click, in minutes. Any other is typed.
const PRESETS: [u32; 3] = [15, 30, 60];
/// The longest timer the shell takes (`sleep::MAX_MINUTES`).
const MAX_MINUTES: u32 = 240;

/// The minutes typed, when they make a timer.
fn minutes(text: &str) -> Option<u32> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|m| (1..=MAX_MINUTES).contains(m))
}

impl LightingPage {
    /// Seconds until the lights go off, while a timer runs.
    fn sleep_left(&self, cx: &Context<Self>) -> Option<f32> {
        let (status, at) = self.lueur.read(cx).sleep.as_ref()?;
        Some((status.left as f32 - at.elapsed().as_secs_f32()).max(0.0))
    }

    fn toggle_sleep(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The same click that closed it from outside does not open it again.
        if self
            .sleep_closed_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(250))
        {
            return;
        }
        self.sleep_open = !self.sleep_open;
        self.sleep_field = None;
        if self.sleep_open {
            // The panel's first control takes the keyboard as it opens.
            window.focus(&self.sleep_first, cx);
        }
        cx.notify();
    }

    /// Close the panel and give the keyboard back to the clock.
    fn close_sleep(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sleep_open = false;
        self.sleep_field = None;
        window.focus(&self.sleep_focus, cx);
        cx.notify();
    }

    fn start_sleep(&mut self, minutes: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.sleep_field = None;
        self.lueur
            .update(cx, |l, cx| l.set_sleep(Some(minutes), cx));
        // The keyboard goes to Cancel, which takes the presets' place.
        window.focus(&self.sleep_first, cx);
        cx.notify();
    }

    /// "Other…": a field for any duration, in place of the presets.
    fn type_minutes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input =
            cx.new(|cx| TextInput::new(cx, "45", crate::i18n::t(cx, "sleep_minutes"), "", 3));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, input, e: &InputEvent, window, cx| match e {
                InputEvent::Submit => {
                    if let Some(m) = minutes(input.read(cx).text()) {
                        this.start_sleep(m, window, cx);
                    }
                }
                InputEvent::Changed => cx.notify(),
            },
        );
        self._subscriptions.push(sub);
        window.focus(&input.focus_handle(cx), cx);
        self.sleep_field = Some(input);
        cx.notify();
    }

    pub(super) fn sleep_button(
        &mut self,
        theme: &Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let left = self.sleep_left(cx);
        let title = t(cx, "sleep_timer");
        let state: SharedString = match left {
            None => t(cx, "sleep_in"),
            Some(s) if s <= 60.0 => t(cx, "sleep_fading"),
            Some(s) => fill(
                &t(cx, "sleep_left"),
                &[("n", &((s / 60.0).ceil() as u32).to_string())],
            )
            .into(),
        };
        let page = cx.entity().downgrade();
        let trigger = btn("sleep", Kind::Plain, true, false, theme)
            .track_focus(&self.sleep_focus)
            .aria_label(match left {
                None => title.clone(),
                Some(_) => format!("{title}: {state}").into(),
            })
            .aria_expanded(self.sleep_open)
            .tooltip(ui::tip(title.clone()))
            .gap(px(5.0))
            .child(ui::icon("clock", 15.0, false, theme.text_2))
            .children(left.map(|s| {
                let n = if s <= 60.0 {
                    "<1".to_string()
                } else {
                    ((s / 60.0).ceil() as u32).to_string()
                };
                div()
                    .text_size(px(size::T_SM))
                    .text_color(theme.text)
                    .child(format!("{n} min"))
            }))
            .on_click({
                let page = page.clone();
                move |_, window, cx| {
                    let _ = page.update(cx, |p, cx| p.toggle_sleep(window, cx));
                }
            });

        let panel = self
            .sleep_open
            .then(|| self.sleep_panel(state, title, theme, cx));
        // Where the clock is, so the panel opens under it.
        let at = self.sleep_at.clone();
        div()
            .relative()
            .child(trigger)
            .child(
                gpui::canvas(move |bounds, _, _| at.set(Some(bounds)), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .children(panel)
            .into_any_element()
    }

    fn sleep_panel(
        &self,
        state: SharedString,
        title: SharedString,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let l = self.lueur.read(cx);
        let running = l.sleep.is_some();
        // Lueur keeps the computer awake while a timer runs, and stays open
        // even when closing the window would quit it.
        let stays_open = !l.config.behavior.minimize_to_tray;
        let page = cx.entity().downgrade();
        let body: AnyElement = if running {
            let cancel = t(cx, "sleep_cancel");
            let lueur = self.lueur.clone();
            let page = page.clone();
            div()
                .flex()
                .child(
                    btn("sleep-cancel", Kind::Default, true, false, theme)
                        .track_focus(&self.sleep_first)
                        .aria_label(cancel.clone())
                        .child(cancel)
                        .on_click(move |_, window, cx| {
                            lueur.update(cx, |l, cx| l.set_sleep(None, cx));
                            let _ = page.update(cx, |p, cx| {
                                window.focus(&p.sleep_first, cx);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element()
        } else if let Some(field) = self.sleep_field.clone() {
            let typed = minutes(field.read(cx).text());
            let start = t(cx, "sleep_start");
            let back = t(cx, "sleep_back");
            let (p1, p2, p3) = (page.clone(), page.clone(), page.clone());
            div()
                .flex()
                .items_center()
                .gap(px(size::S2))
                .on_key_down(move |e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        let _ = p1.update(cx, |p, cx| {
                            p.sleep_field = None;
                            window.focus(&p.sleep_first, cx);
                            cx.notify();
                        });
                    }
                })
                .child(div().w(px(64.0)).child(field))
                .child(
                    div()
                        .text_size(px(size::T_SM))
                        .text_color(theme.text_2)
                        .child("min"),
                )
                .child(
                    btn("sleep-start", Kind::Default, true, typed.is_none(), theme)
                        .aria_label(start.clone())
                        .child(start)
                        .when_some(typed, |b, m| {
                            b.on_click(move |_, window, cx| {
                                let _ = p2.update(cx, |p, cx| p.start_sleep(m, window, cx));
                            })
                        }),
                )
                .child(
                    btn("sleep-back", Kind::Plain, true, false, theme)
                        .w(px(24.0))
                        .px_0()
                        .aria_label(back.clone())
                        .tooltip(ui::tip(back))
                        .child(ui::icon("close", 14.0, false, theme.text_2))
                        .on_click(move |_, window, cx| {
                            let _ = p3.update(cx, |p, cx| {
                                p.sleep_field = None;
                                window.focus(&p.sleep_first, cx);
                                cx.notify();
                            });
                        }),
                )
                .into_any_element()
        } else {
            let presets = PRESETS.iter().enumerate().map(|(i, &m)| {
                let page = page.clone();
                let label: SharedString = format!("{m} min").into();
                btn(("sleep-preset", i), Kind::Default, true, false, theme)
                    .when(i == 0, |b| b.track_focus(&self.sleep_first))
                    .aria_label(label.clone())
                    .child(label)
                    .on_click(move |_, window, cx| {
                        let _ = page.update(cx, |p, cx| p.start_sleep(m, window, cx));
                    })
            });
            let other = t(cx, "sleep_other");
            let page = page.clone();
            div()
                .id("sleep-presets")
                .role(Role::Group)
                .aria_label(title.clone())
                .flex()
                .gap(px(size::S1))
                .children(presets)
                .child(
                    btn("sleep-other", Kind::Default, true, false, theme)
                        .aria_label(other.clone())
                        .child(other)
                        .on_click(move |_, window, cx| {
                            let _ = page.update(cx, |p, cx| p.type_minutes(window, cx));
                        }),
                )
                .into_any_element()
        };
        let note: SharedString = if running {
            if stays_open {
                format!("{} {}", t(cx, "sleep_keep_on"), t(cx, "sleep_stays_open")).into()
            } else {
                t(cx, "sleep_keep_on")
            }
        } else {
            t(cx, "sleep_timer_tip")
        };
        let (close_out, close_key) = (page.clone(), page);
        deferred(
            anchored()
                .when_some(self.sleep_at.get(), |a, target| {
                    a.attach(target, gpui::Anchor::BottomRight)
                })
                .anchor(gpui::Anchor::TopRight)
                .snap_to_window()
                .child(
                    div()
                        .id("sleep-panel")
                        .role(Role::Dialog)
                        .aria_label(title)
                        .occlude()
                        .mt(px(6.0))
                        .w(px(300.0))
                        .flex()
                        .flex_col()
                        .gap(px(size::S3))
                        .pt(px(size::S3))
                        .px(px(size::S4))
                        .pb(px(size::S4))
                        .rounded(px(size::R_LG))
                        .bg(theme.surface)
                        .border_1()
                        .border_color(theme.line)
                        .shadow_lg()
                        .on_key_down(move |e: &KeyDownEvent, window, cx| {
                            if e.keystroke.key == "escape" {
                                cx.stop_propagation();
                                let _ = close_key.update(cx, |p, cx| p.close_sleep(window, cx));
                            }
                        })
                        .on_mouse_down_out(move |_, _, cx| {
                            let _ = close_out.update(cx, |p, cx| {
                                p.sleep_open = false;
                                p.sleep_field = None;
                                p.sleep_closed_at = Some(Instant::now());
                                cx.notify();
                            });
                        })
                        .child(
                            div()
                                .text_size(px(size::T_MD))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(state),
                        )
                        .child(body)
                        .child(
                            div()
                                .id("sleep-note")
                                .when(running, |d| d.role(Role::Status))
                                .text_size(px(size::T_SM))
                                .text_color(theme.text_3)
                                .child(note),
                        ),
                ),
        )
        .with_priority(1)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_whole_minutes_in_range_make_a_timer() {
        assert_eq!(super::minutes(" 45 "), Some(45));
        assert_eq!(super::minutes("0"), None);
        assert_eq!(super::minutes("241"), None);
        assert_eq!(super::minutes("1.5"), None);
    }
}
