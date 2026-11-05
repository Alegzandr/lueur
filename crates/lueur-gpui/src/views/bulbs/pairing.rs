//! Pairing Hue bridges and Nanoleaf controllers, and the Smart Life sign-in
//! that imports Tuya bulbs, and the panels the page shows for them.

use super::controls::{Look, btn, disclosure, group_head, note, spinner};
use super::{BulbsPage, SEARCH_MS, actions};
use crate::i18n::{Lang, error_text, fill, t, tn};
use crate::state::View;
use crate::theme::{Theme, size, theme};
use crate::ui::{self, InputEvent, TextInput};
use gpui::{
    AnyElement, App, AsyncApp, Context, Entity, Focusable, FontWeight, Image, ImageFormat, Role,
    SharedString, Subscription, WeakEntity, Window, div, img, prelude::*, px, relative,
};
use lueur_core::device::DiscoveredDevice;
use lueur_shell::error::AppError;
use lueur_shell::service::{self, TuyaLogin};
use lueur_shell::tuya_cloud::normalize_code;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a bridge or controller waits for its button to be pressed.
const PAIR_WINDOW: Duration = Duration::from_secs(30);
/// How long a Smart Life QR code waits to be scanned.
const QR_WINDOW: Duration = Duration::from_secs(180);

/// A bridge or controller being paired, and how it went.
pub struct Pairing {
    pub device: DiscoveredDevice,
    pub step: PairStep,
    pub until: Instant,
}

pub enum PairStep {
    /// The page asks for the button and counts down.
    Waiting,
    Paired,
    Failed(AppError),
}

/// Where the Smart Life sign-in stands.
#[derive(Clone)]
pub enum Tuya {
    /// Asking for the user code; `expired` after a QR code went unscanned.
    Code { expired: bool },
    /// The code was sent; the QR code is on its way.
    Starting,
    /// The QR code, drawn by the shell from a token it checked, never
    /// fetched.
    Qr { image: Arc<Image> },
    /// Imported; looking for the bulbs on the network.
    Adding,
    Done {
        imported: usize,
        added: usize,
        /// Bulbs found but left for the user to place, since lights wait
        /// for a bulb and new ones would duplicate them.
        ready: Vec<String>,
        /// Fewer Smart Life bulbs answered than were imported.
        missing: bool,
    },
}

/// Why a (normalized) user code cannot be right, or `None` if it may be.
pub fn user_code_problem(code: &str) -> Option<&'static str> {
    if code.is_empty() {
        Some("tuya_code_empty")
    } else if !code.bytes().all(|b| b.is_ascii_alphanumeric()) || code.len() > 64 {
        Some("tuya_code_chars")
    } else if code.len() < 4 {
        Some("tuya_code_short")
    } else {
        None
    }
}

/// What came of a Smart Life import, sentence by sentence.
fn tuya_summary(
    imported: usize,
    added: usize,
    missing: bool,
    ready: usize,
    cx: &App,
) -> Vec<String> {
    if imported == 0 {
        return vec![t(cx, "tuya_none").to_string()];
    }
    let mut out = Vec::new();
    if added > 0 {
        out.push(tn(cx, "tuya_added", added));
    }
    if ready > 0 {
        out.push(tn(cx, "tuya_ready", ready));
    }
    if missing {
        out.push(t(cx, "tuya_missing").to_string());
    }
    if out.is_empty() {
        out.push(t(cx, "tuya_already").to_string());
    }
    out
}

/// A failed pairing in words: the button not pressed in time, or why it
/// did not work.
fn failure_text(e: &AppError, cx: &App) -> String {
    if e.code == "pair_waiting" {
        t(cx, "pair_expired").to_string()
    } else {
        let lang = *cx.global::<Lang>();
        format!("{} {}", t(cx, "pair_failed"), error_text(lang, e))
    }
}

impl Pairing {
    fn hue(&self) -> bool {
        self.device.kind.family() == "hue"
    }

    pub fn said(&self, page: &BulbsPage, cx: &App) -> SharedString {
        match &self.step {
            PairStep::Waiting => t(
                cx,
                if self.hue() {
                    "pair_hue"
                } else {
                    "pair_nanoleaf"
                },
            ),
            PairStep::Paired => fill(
                &t(cx, "paired"),
                &[("name", &page.device_name(&self.device, cx))],
            )
            .into(),
            PairStep::Failed(e) => failure_text(e, cx).into(),
        }
    }
}

impl Tuya {
    pub fn said(&self, page: &BulbsPage, cx: &App) -> SharedString {
        match self {
            Tuya::Adding => t(cx, "tuya_adding"),
            Tuya::Done {
                imported,
                added,
                missing,
                ..
            } => tuya_summary(*imported, *added, *missing, page.tuya_ready(cx).len(), cx)
                .join(" ")
                .into(),
            Tuya::Code { expired: true } => t(cx, "tuya_expired"),
            _ => SharedString::default(),
        }
    }
}

impl BulbsPage {
    /// Pair with a Hue bridge or Nanoleaf controller: the page asks the
    /// user to press its button and counts down, while Lueur keeps asking
    /// the device for up to 30 seconds. The outcome stays on the page until
    /// it is closed.
    pub(super) fn pair(&mut self, device: DiscoveredDevice, cx: &mut Context<Self>) {
        if self
            .pairing
            .as_ref()
            .is_some_and(|p| matches!(p.step, PairStep::Waiting))
        {
            return;
        }
        self.pair_run += 1;
        let run = self.pair_run;
        let until = Instant::now() + PAIR_WINDOW;
        self.pairing = Some(Pairing {
            device: device.clone(),
            step: PairStep::Waiting,
            until,
        });
        self.detail_open = false;
        // Keyboard and screen reader users land on the panel as it opens.
        self.focus_next = Some("pair-head".into());
        self.count_down(cx);
        cx.notify();
        let shell = self.lueur.read(cx).shell.clone();
        cx.spawn(async move |this, cx| {
            let result = loop {
                let (s, d) = (shell.clone(), device.clone());
                let r = cx
                    .background_executor()
                    .spawn(async move { service::pair_device(&s, d) })
                    .await;
                match r {
                    Ok(()) => break Ok(()),
                    Err(e) => {
                        let current = this
                            .read_with(cx, |p, _| p.pair_run == run)
                            .unwrap_or(false);
                        if e.code != "pair_waiting" || Instant::now() > until || !current {
                            break Err(e);
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(1500))
                            .await;
                    }
                }
            };
            let paired = result.is_ok();
            let current = this.update(cx, |p, cx| {
                // Even when cancelled meanwhile: the shell keeps the
                // pairing, so the config here must catch up with it.
                if paired {
                    p.lueur.update(cx, |l, cx| l.reload_config(cx));
                }
                let current = p.pair_run == run;
                if current && let Some(pairing) = &mut p.pairing {
                    pairing.step = match result {
                        Ok(()) => PairStep::Paired,
                        Err(e) => PairStep::Failed(e),
                    };
                }
                cx.notify();
                current
            });
            // Its lamps now show up: search for them.
            if paired && current.unwrap_or(false) {
                super::scan(&this, cx, SEARCH_MS, false, true).await;
            }
        })
        .detach();
    }

    /// Redraw the countdown four times a second while a device waits for
    /// its button.
    fn count_down(&mut self, cx: &mut Context<Self>) {
        self._ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let waiting = this.update(cx, |p, cx| {
                    cx.notify();
                    p.pairing
                        .as_ref()
                        .is_some_and(|x| matches!(x.step, PairStep::Waiting))
                });
                if !waiting.unwrap_or(false) {
                    break;
                }
            }
        }));
    }

    /// Close the pairing panel. Focus goes back to the device's Pair button
    /// when there still is one, else to the list it was in, rather than
    /// being lost with the panel.
    fn end_pairing(&mut self, cx: &mut Context<Self>) {
        let id = self.pairing.take().map(|p| p.device.id);
        self.pair_run += 1;
        let button = id.filter(|id| {
            self.discovered
                .iter()
                .any(|d| &d.id == id && !d.paired && !self.is_bound(id, cx))
        });
        let network_shown =
            !self.unbound(cx).is_empty() || self.lueur.read(cx).config.room.lights.is_empty();
        self.focus_next = Some(match button {
            Some(id) => format!("pair-{id}"),
            None if network_shown => "network-head".into(),
            None => "page-head".into(),
        });
        cx.notify();
    }

    /// Retry with the device as last found, in case it moved meanwhile.
    fn retry_pairing(&mut self, device: DiscoveredDevice, cx: &mut Context<Self>) {
        let d = self
            .discovered
            .iter()
            .find(|x| x.id == device.id)
            .cloned()
            .unwrap_or(device);
        self.pair(d, cx);
    }

    /// The field for the Smart Life user code, named in the current
    /// language, and what the page does as it is typed in.
    pub(super) fn new_code_input(cx: &mut Context<Self>) -> (Entity<TextInput>, Subscription) {
        let label = t(cx, "tuya_code");
        let input = cx.new(|cx| TextInput::new(cx, "", label, "", 80));
        let sub = cx.subscribe(&input, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Changed => {
                this.code_problem = None;
                cx.notify();
            }
            InputEvent::Submit => this.submit_code(cx),
        });
        (input, sub)
    }

    /// Open the Smart Life sign-in, at the user-code step.
    pub(super) fn open_tuya(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.tuya_run += 1;
        self.tuya = Some(Tuya::Code { expired: false });
        self.code_problem = None;
        let (input, sub) = Self::new_code_input(cx);
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.code_input = input;
        self._code_sub = sub;
        cx.notify();
    }

    pub(super) fn close_tuya(&mut self, cx: &mut Context<Self>) {
        self.tuya_run += 1;
        self.tuya = None;
        cx.notify();
    }

    /// Continue: the code as typed, without the spaces and dashes people
    /// put in to read it, checked, then sent.
    fn submit_code(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.tuya, Some(Tuya::Code { .. })) {
            return;
        }
        let code = normalize_code(self.code_input.read(cx).text());
        self.code_input
            .update(cx, |input, cx| input.set_text(code.clone(), cx));
        self.code_problem = user_code_problem(&code);
        if self.code_problem.is_none() {
            self.sign_in(code, cx);
        }
        cx.notify();
    }

    /// Sign in to Smart Life with the user code: show the QR code, wait
    /// (three minutes at most) for it to be scanned, import the bulbs, then
    /// find and add them (see [`finish_import`]).
    fn sign_in(&mut self, code: String, cx: &mut Context<Self>) {
        self.tuya_run += 1;
        let run = self.tuya_run;
        self.tuya = Some(Tuya::Starting);
        let shell = self.lueur.read(cx).shell.clone();
        cx.spawn(async move |this, cx| {
            // Cancelled or replaced meanwhile, a failure is no longer news.
            let failed = |e: AppError, this: &WeakEntity<BulbsPage>, cx: &mut AsyncApp| {
                let _ = this.update(cx, |p, cx| {
                    if p.tuya_run == run {
                        p.tuya = Some(Tuya::Code { expired: false });
                        p.lueur.update(cx, |l, cx| l.fail(&e, cx));
                        cx.notify();
                    }
                });
            };
            let c = code.clone();
            let start = cx
                .background_executor()
                .spawn(async move { service::tuya_login_start(&c) })
                .await;
            let TuyaLogin { token, svg } = match start {
                Ok(login) => login,
                Err(e) => return failed(e, &this, cx),
            };
            let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()));
            let shown = this.update(cx, |p, cx| {
                let current = p.tuya_run == run;
                if current {
                    p.tuya = Some(Tuya::Qr { image });
                    cx.notify();
                }
                current
            });
            if !shown.unwrap_or(false) {
                return;
            }
            let deadline = Instant::now() + QR_WINDOW;
            loop {
                let (s, token, c) = (shell.clone(), token.clone(), code.clone());
                let polled = cx
                    .background_executor()
                    .spawn(async move { service::tuya_login_poll(&s, &token, &c) })
                    .await;
                match polled {
                    Err(e) => return failed(e, &this, cx),
                    Ok(Some(imported)) => {
                        // Imported even if the page moved on meanwhile: the
                        // shell keeps the keys, so the config here must
                        // catch up with it.
                        let current = this.update(cx, |p, cx| {
                            p.lueur.update(cx, |l, cx| l.reload_config(cx));
                            p.tuya_run == run
                        });
                        if current.unwrap_or(false) {
                            finish_import(&this, cx, imported, run).await;
                        }
                        return;
                    }
                    Ok(None) => {
                        let current = this.update(cx, |p, cx| {
                            let current = p.tuya_run == run;
                            if current && Instant::now() > deadline {
                                p.tuya = Some(Tuya::Code { expired: true });
                                cx.notify();
                                return false;
                            }
                            current
                        });
                        if !current.unwrap_or(false) {
                            return;
                        }
                        cx.background_executor().timer(Duration::from_secs(2)).await;
                    }
                }
            }
        })
        .detach();
    }

    /// Look again for the Smart Life bulbs that did not answer.
    fn search_tuya_again(&mut self, imported: usize, cx: &mut Context<Self>) {
        self.tuya_run += 1;
        let run = self.tuya_run;
        cx.spawn(async move |this, cx| finish_import(&this, cx, imported, run).await)
            .detach();
    }

    /// The Smart Life bulbs found after an import, still waiting for a light.
    pub(super) fn tuya_ready(&self, cx: &App) -> Vec<DiscoveredDevice> {
        let Some(Tuya::Done { ready, .. }) = &self.tuya else {
            return Vec::new();
        };
        self.discovered
            .iter()
            .filter(|d| ready.contains(&d.id) && !self.is_bound(&d.id, cx))
            .cloned()
            .collect()
    }

    /// Add the Smart Life bulbs left for the user to place, as new lights.
    fn add_ready_tuya(&mut self, cx: &mut Context<Self>) {
        // Some may have been given to a light by hand meanwhile.
        let named: Vec<_> = self
            .tuya_ready(cx)
            .into_iter()
            .map(|d| {
                let name = self.device_name(&d, cx);
                (d, name)
            })
            .collect();
        let n = named.len();
        self.lueur
            .update(cx, |l, cx| actions::add_devices(l, named, false, cx));
        if let Some(Tuya::Done { added, ready, .. }) = &mut self.tuya {
            *added += n;
            ready.clear();
        }
        cx.notify();
    }

    /// The panel of a pairing under way or just ended.
    pub(super) fn pairing_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *theme(cx);
        let head = self.handle("pair-head", false, cx);
        let p = self.pairing.as_ref()?;
        let device = p.device.clone();
        let name = self.device_name(&device, cx);
        let hue = p.hue();
        let title = fill(&t(cx, "pair_title"), &[("name", &name)]);
        let say = |text: SharedString| {
            div()
                .text_size(px(size::T_LG))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(text)
        };
        let (mark_bg, mark): (gpui::Hsla, AnyElement) = match &p.step {
            PairStep::Waiting => (
                theme.glow_soft,
                spinner("pair-spin", 18.0, theme.glow_text, cx.reduce_motion()),
            ),
            PairStep::Paired => (
                theme.ok.opacity(0.16),
                ui::icon("check", 18.0, false, theme.ok).into_any_element(),
            ),
            PairStep::Failed(_) => (
                theme.danger_soft,
                ui::icon("alert", 18.0, false, theme.danger).into_any_element(),
            ),
        };
        let mut text = div().flex_1().min_w_0().flex().flex_col().gap(px(size::S2));
        let paired = matches!(p.step, PairStep::Paired);
        match &p.step {
            PairStep::Waiting => {
                let left = p.until.saturating_duration_since(Instant::now());
                let frac = left.as_secs_f32() / PAIR_WINDOW.as_secs_f32();
                let timer = if left > Duration::ZERO {
                    let n = left.as_millis().div_ceil(1000).to_string();
                    format!(
                        "{} · {}",
                        fill(&t(cx, "pair_left"), &[("n", &n)]),
                        t(cx, "pair_waiting")
                    )
                } else {
                    // Time is up, but the last answer from the device is
                    // still on its way.
                    t(cx, "pair_checking").to_string()
                };
                text = text
                    .child(say(t(cx, if hue { "pair_hue" } else { "pair_nanoleaf" })))
                    .child(note(
                        t(cx, if hue { "pair_why" } else { "pair_why_nanoleaf" }),
                        &theme,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(size::S3))
                            .child(
                                div()
                                    .flex_1()
                                    .max_w(px(260.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(theme.line)
                                    .overflow_hidden()
                                    .child(div().h_full().w(relative(frac)).bg(theme.glow)),
                            )
                            .child(
                                note(timer.clone(), &theme)
                                    .id("pair-timer")
                                    .role(Role::Timer)
                                    .aria_label(timer),
                            ),
                    );
            }
            PairStep::Paired => {
                text = text.child(say(fill(&t(cx, "paired"), &[("name", &name)]).into()));
            }
            PairStep::Failed(e) => {
                text = text.child(say(failure_text(e, cx).into()));
                if let Some(detail) = e.detail.clone() {
                    let open = self.detail_open;
                    text = text.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(size::S1))
                            .text_size(px(size::T_SM))
                            .text_color(theme.text_3)
                            .child(disclosure(
                                "pair-detail",
                                t(cx, "details"),
                                open,
                                &theme,
                                cx.listener(|this, _, _, cx| {
                                    this.detail_open = !this.detail_open;
                                    cx.notify();
                                }),
                            ))
                            .when(open, |d| d.child(detail)),
                    );
                }
            }
        }
        let failed = matches!(p.step, PairStep::Failed(_));
        let retry_device = device.clone();
        let actions = div()
            .flex()
            .gap(px(size::S2))
            .mt(px(size::S1))
            .when(failed, |d| {
                d.child(btn(
                    "pair-retry",
                    t(cx, "retry"),
                    Look::Primary,
                    true,
                    false,
                    None,
                    &theme,
                    cx.listener(move |this, _, _, cx| this.retry_pairing(retry_device.clone(), cx)),
                ))
            })
            .child(btn(
                "pair-close",
                t(cx, if paired { "close" } else { "tuya_cancel" }),
                if paired { Look::Default } else { Look::Plain },
                true,
                false,
                None,
                &theme,
                cx.listener(|this, _, _, cx| this.end_pairing(cx)),
            ));
        Some(
            div()
                .child(group_head(
                    "pair-title",
                    title.into(),
                    Some(&head),
                    None,
                    &theme,
                ))
                .child(
                    ui::card(&theme)
                        .flex_row()
                        .items_start()
                        .gap(px(size::S4))
                        .px(px(size::S5))
                        .py(px(size::S4))
                        .child(
                            div()
                                .size(px(36.0))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(mark_bg)
                                .child(mark),
                        )
                        .child(text.child(actions)),
                )
                .into_any_element(),
        )
    }

    /// The panel of the Smart Life sign-in.
    pub(super) fn tuya_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = *theme(cx);
        let tu = self.tuya.clone()?;
        let body: AnyElement = match tu {
            Tuya::Qr { image } => div()
                .flex()
                .items_center()
                .gap(px(size::S5))
                .child(
                    div()
                        .id("tuya-qr")
                        .role(Role::Image)
                        .aria_label(t(cx, "tuya_qr"))
                        .size(px(168.0))
                        .flex_none()
                        .rounded(px(size::R_MD))
                        .overflow_hidden()
                        // A QR code reads dark on white whatever the theme.
                        .bg(gpui::white())
                        .child(img(image).size_full()),
                )
                .child(
                    tuya_text()
                        .child(div().text_color(theme.text).child(t(cx, "tuya_scan")))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(size::S2))
                                .child(spinner("tuya-spin", 12.0, theme.text_3, cx.reduce_motion()))
                                .child(note(t(cx, "tuya_waiting"), &theme)),
                        )
                        .child(div().flex().child(btn(
                            "tuya-cancel",
                            t(cx, "tuya_cancel"),
                            Look::Plain,
                            true,
                            false,
                            None,
                            &theme,
                            cx.listener(|this, _, _, cx| this.close_tuya(cx)),
                        ))),
                )
                .into_any_element(),
            Tuya::Adding => tuya_text()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(size::S2))
                        .child(spinner("tuya-spin", 12.0, theme.text_3, cx.reduce_motion()))
                        .child(t(cx, "tuya_adding")),
                )
                .into_any_element(),
            Tuya::Done {
                imported,
                added,
                missing,
                ..
            } => self.tuya_done(imported, added, missing, &theme, cx),
            Tuya::Code { expired } => self.tuya_form(expired, false, &theme, cx),
            Tuya::Starting => self.tuya_form(false, true, &theme, cx),
        };
        Some(
            div()
                .child(group_head(
                    "tuya-title",
                    t(cx, "tuya_title"),
                    None,
                    None,
                    &theme,
                ))
                .child(
                    ui::card(&theme)
                        .px(px(size::S5))
                        .py(px(size::S4))
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn tuya_done(
        &mut self,
        imported: usize,
        added: usize,
        missing: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ready = self.tuya_ready(cx).len();
        let lines = tuya_summary(imported, added, missing, ready, cx);
        let lueur = self.lueur.clone();
        tuya_text()
            .children(lines.into_iter().enumerate().map(|(i, line)| {
                div()
                    .text_color(theme.text)
                    .when(i == 0, |d| {
                        d.text_size(px(size::T_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                    })
                    .child(line)
            }))
            .child(
                div()
                    .flex()
                    .gap(px(size::S2))
                    .when(ready > 0, |d| {
                        d.child(btn(
                            "tuya-add",
                            tn(cx, "tuya_add_n", ready).into(),
                            Look::Primary,
                            false,
                            false,
                            None,
                            theme,
                            cx.listener(|this, _, _, cx| this.add_ready_tuya(cx)),
                        ))
                    })
                    .when(ready == 0 && added > 0, |d| {
                        d.child(btn(
                            "tuya-plan",
                            t(cx, "step2_btn"),
                            Look::Primary,
                            false,
                            false,
                            None,
                            theme,
                            cx.listener(move |this, _, _, cx| {
                                this.close_tuya(cx);
                                lueur.update(cx, |l, cx| l.show(View::Room, cx));
                            }),
                        ))
                    })
                    .when(missing, |d| {
                        d.child(btn(
                            "tuya-search",
                            t(cx, "search_again"),
                            Look::Default,
                            false,
                            self.scanning,
                            None,
                            theme,
                            cx.listener(move |this, _, _, cx| this.search_tuya_again(imported, cx)),
                        ))
                    })
                    .child(btn(
                        "tuya-close",
                        t(cx, "close"),
                        Look::Plain,
                        false,
                        false,
                        None,
                        theme,
                        cx.listener(|this, _, _, cx| this.close_tuya(cx)),
                    )),
            )
            .into_any_element()
    }

    /// The user-code form: the field, what is wrong with the code, and how
    /// to find it.
    fn tuya_form(
        &mut self,
        expired: bool,
        starting: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Characters a code can never hold are pointed out while typing; a
        // code too short only on Continue, since it is short until finished.
        let typed = normalize_code(self.code_input.read(cx).text());
        let shown =
            self.code_problem
                .or((user_code_problem(&typed) == Some("tuya_code_chars"))
                    .then_some("tuya_code_chars"));
        tuya_text()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(size::S3))
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(size::T_MD))
                    .child(t(cx, "tuya_code"))
                    .child(
                        div()
                            .w(px(220.0))
                            .rounded(px(8.0))
                            .border_1()
                            .border_color(if shown.is_some() {
                                theme.danger
                            } else {
                                gpui::transparent_black()
                            })
                            .child(self.code_input.clone()),
                    ),
            )
            // Read out once as it appears; the field is not also tied to it,
            // which would read it a second time.
            .when_some(shown, |d, problem| {
                d.child(
                    div()
                        .id("tuya-problem")
                        .role(Role::Alert)
                        .aria_label(t(cx, problem))
                        .flex()
                        .items_center()
                        .gap(px(size::S2))
                        .text_color(theme.danger)
                        .child(ui::icon("alert", 13.0, false, theme.danger))
                        .child(t(cx, problem)),
                )
            })
            .when(expired, |d| {
                d.child(
                    div()
                        .text_color(theme.glow_text)
                        .child(t(cx, "tuya_expired")),
                )
            })
            .child(note(t(cx, "tuya_code_help"), theme))
            .child(
                div()
                    .flex()
                    .gap(px(size::S2))
                    .child(btn(
                        "tuya-cancel",
                        t(cx, "tuya_cancel"),
                        Look::Plain,
                        false,
                        false,
                        None,
                        theme,
                        cx.listener(|this, _, _, cx| this.close_tuya(cx)),
                    ))
                    .child(btn(
                        "tuya-continue",
                        t(cx, "tuya_continue"),
                        Look::Primary,
                        false,
                        starting,
                        starting.then(|| {
                            spinner("tuya-start-spin", 12.0, theme.text_3, cx.reduce_motion())
                        }),
                        theme,
                        cx.listener(|this, _, _, cx| this.submit_code(cx)),
                    )),
            )
            .into_any_element()
    }
}

/// The column of words beside the QR code, or the form.
fn tuya_text() -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(size::S3))
        .text_size(px(size::T_MD))
}

/// After a Smart Life import: look for the bulbs on the network and add the
/// ones found to the room, so importing really ends with bulbs in the room.
/// When lights drawn without a bulb are waiting, adding new lights would
/// duplicate them: the user is asked to pick instead, with a button to add
/// them all anyway.
async fn finish_import(this: &WeakEntity<BulbsPage>, cx: &mut AsyncApp, imported: usize, run: u64) {
    let current = this.update(cx, |p, cx| {
        let current = p.tuya_run == run;
        if current {
            p.tuya = Some(Tuya::Adding);
            cx.notify();
        }
        current
    });
    if !current.unwrap_or(false) {
        return;
    }
    if imported > 0 {
        super::scan(this, cx, 6000, false, true).await;
    }
    let _ = this.update(cx, |p, cx| {
        if p.tuya_run != run {
            return;
        }
        let found: Vec<DiscoveredDevice> = p
            .discovered
            .iter()
            .filter(|d| d.kind.family() == "tuya")
            .cloned()
            .collect();
        let ready: Vec<DiscoveredDevice> = found
            .iter()
            .filter(|d| d.paired && !p.is_bound(&d.id, cx))
            .cloned()
            .collect();
        let lights_waiting = {
            let c = &p.lueur.read(cx).config;
            c.room.lights.iter().any(|x| c.binding_for(&x.id).is_none())
        };
        let added = if lights_waiting { 0 } else { ready.len() };
        if added > 0 {
            let named: Vec<_> = ready
                .iter()
                .map(|d| (d.clone(), p.device_name(d, cx)))
                .collect();
            p.lueur
                .update(cx, |l, cx| actions::add_devices(l, named, false, cx));
        }
        p.tuya = Some(Tuya::Done {
            imported,
            added,
            ready: if added > 0 {
                Vec::new()
            } else {
                ready.iter().map(|d| d.id.clone()).collect()
            },
            // Every Smart Life bulb heard, bound or not, against every one
            // imported: fewer heard means some did not answer, though not
            // how many, since bound ones may come from another account.
            missing: found.len() < imported,
        });
        cx.notify();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_code_is_checked_before_it_is_sent() {
        assert_eq!(user_code_problem(""), Some("tuya_code_empty"));
        assert_eq!(user_code_problem("ab"), Some("tuya_code_short"));
        assert_eq!(user_code_problem("abc!"), Some("tuya_code_chars"));
        assert_eq!(user_code_problem("é123"), Some("tuya_code_chars"));
        assert_eq!(user_code_problem(&"a".repeat(65)), Some("tuya_code_chars"));
        assert_eq!(user_code_problem("eu1234AbC"), None);
        // Spaces and dashes people type to read it go before the check.
        assert_eq!(user_code_problem(&normalize_code("eu12 34-AbC")), None);
    }
}
