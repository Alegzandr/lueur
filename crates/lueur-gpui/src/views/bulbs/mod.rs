//! The Bulbs page, after `src/lib/views/Bulbs.svelte`: what the network
//! offers on one side, the room's lights on the other, and above them the
//! pairing of a bridge, the Smart Life sign-in and the check of devices
//! nothing vouches for.
//!
//! What only this page needs lives here, after `src/lib/state/
//! devices.svelte.ts` and `pairing.svelte.ts`: the bulbs last found, the
//! search running, the pairing and the sign-in under way. What it changes in
//! the room goes through [`actions`] to the app's [`Lueur`].

mod actions;
mod brands;
mod check;
mod controls;
mod lights;
mod network;
mod pairing;

use crate::i18n::{fill, t};
use crate::state::{Lueur, View};
use crate::theme::{size, theme};
use crate::ui::{self, TextInput};
use controls::{Look, btn, spinner};
use gpui::{
    AnyElement, AsyncApp, Context, Entity, FocusHandle, IntoElement, Render, Role, SharedString,
    Subscription, Task, WeakEntity, Window, div, prelude::*, px,
};
use lueur_core::device::DiscoveredDevice;
use lueur_shell::error::AppResult;
use lueur_shell::service;
use pairing::{Pairing, Tuya};
use std::collections::HashMap;
use std::time::Duration;

/// How long a search listens, unless said otherwise.
const SEARCH_MS: u64 = 3000;

pub struct BulbsPage {
    lueur: Entity<Lueur>,
    /// The bulbs the last search found.
    discovered: Vec<DiscoveredDevice>,
    scanning: bool,
    has_scanned: bool,
    pairing: Option<Pairing>,
    /// Bumped by every pairing started or cancelled, so an older one still
    /// waiting on the device knows to stop.
    pair_run: u64,
    tuya: Option<Tuya>,
    /// Bumped by every sign-in started, reopened or closed, likewise.
    tuya_run: u64,
    code_input: Entity<TextInput>,
    _code_sub: Subscription,
    /// What is wrong with the user code, once Continue was pressed.
    code_problem: Option<&'static str>,
    renaming: Option<lights::Renaming>,
    /// The tips, opened or closed by hand; until then they open by
    /// themselves when a search found nothing.
    tips_open: Option<bool>,
    /// The technical detail of a failed pairing, shown on request.
    detail_open: bool,
    /// The bulb whose "Use for…" list is open.
    menu: Option<String>,
    /// True while a device check's answer is being sent.
    check_busy: bool,
    /// Unverified devices already seen, so only news is read out.
    check_seen: Vec<String>,
    check_news: SharedString,
    /// Places the keyboard is sent to, by name.
    handles: HashMap<String, FocusHandle>,
    /// Where to move the keyboard on the next frame, once it is drawn.
    focus_next: Option<String>,
    /// Redraws the pairing countdown while a device waits for its button.
    _ticker: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl BulbsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lueur = super::watch(cx);
        // First visit: look straight away rather than greeting with an
        // empty list.
        let first_visit = cx.observe(&lueur, |this: &mut Self, lueur, cx| {
            if lueur.read(cx).view == View::Bulbs && !this.has_scanned && !this.scanning {
                this.scan(SEARCH_MS, false, cx);
            }
            this.note_unverified(cx);
        });
        let (code_input, code_sub) = Self::new_code_input(cx);
        let mut page = BulbsPage {
            lueur: lueur.clone(),
            discovered: Vec::new(),
            scanning: false,
            has_scanned: false,
            pairing: None,
            pair_run: 0,
            tuya: None,
            tuya_run: 0,
            code_input,
            _code_sub: code_sub,
            code_problem: None,
            renaming: None,
            tips_open: None,
            detail_open: false,
            menu: None,
            check_busy: false,
            check_seen: Vec::new(),
            check_news: SharedString::default(),
            handles: HashMap::new(),
            focus_next: None,
            _ticker: None,
            _subs: vec![first_visit],
        };
        // Bound bulbs may have moved since the last launch: a quiet search
        // finds them at their new address before any light is set.
        let (bound, shown) = {
            let l = lueur.read(cx);
            (!l.config.bindings.is_empty(), l.view == View::Bulbs)
        };
        if bound || shown {
            page.scan(SEARCH_MS, !shown, cx);
        }
        page.load_unverified(cx);
        page
    }

    /// A focus handle kept under `key`, made the first time it is asked
    /// for. `tab_stop` is false for headings, reached only by the page.
    fn handle(&mut self, key: &str, tab_stop: bool, cx: &mut Context<Self>) -> FocusHandle {
        self.handles
            .entry(key.to_string())
            .or_insert_with(|| cx.focus_handle().tab_stop(tab_stop))
            .clone()
    }

    /// Run `job` with the shell off the window's thread, then `done` with
    /// its result on the page.
    fn job<T: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(&lueur_shell::Shell) -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
    ) {
        let shell = self.lueur.read(cx).shell.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { job(&shell) })
                .await;
            let _ = this.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Search the network, unless a search runs already.
    fn scan(&mut self, timeout_ms: u64, quiet: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            scan(&this, cx, timeout_ms, quiet, false).await;
        })
        .detach();
    }

    /// Take what a search found. Bound bulbs found at a new address are
    /// updated in place: the device id is stable, the address is not (nor,
    /// for a Tuya bulb, the protocol version it was found speaking). `quiet`
    /// keeps a failure of the search at launch from showing.
    fn found(
        &mut self,
        result: AppResult<Vec<DiscoveredDevice>>,
        quiet: bool,
        cx: &mut Context<Self>,
    ) {
        self.scanning = false;
        let found = match result {
            Ok(found) => found,
            Err(e) => {
                if !quiet {
                    self.lueur.update(cx, |l, cx| l.fail(&e, cx));
                }
                cx.notify();
                return;
            }
        };
        self.discovered = found;
        self.has_scanned = true;
        let names: Vec<String> = self
            .discovered
            .iter()
            .map(|d| self.device_name(d, cx))
            .collect();
        let found = self.discovered.clone();
        self.lueur.update(cx, |l, cx| {
            let mut moved = Vec::new();
            let mut changed = Vec::new();
            for (d, name) in found.iter().zip(&names) {
                let Some(b) = l.config.bindings.iter().find(|b| b.device_id == d.id) else {
                    continue;
                };
                if b.ip != d.ip || b.port != d.port {
                    let light = l.config.room.lights.iter().find(|x| x.id == b.light_id);
                    moved.push(light.map_or(name.clone(), |x| x.name.clone()));
                }
                if b.endpoint() != d.endpoint() {
                    changed.push(d.clone());
                }
            }
            if !changed.is_empty() {
                l.edit(cx, |c| {
                    for d in &changed {
                        if let Some(b) = c.bindings.iter_mut().find(|b| b.device_id == d.id) {
                            b.ip = d.ip.clone();
                            b.port = d.port;
                            b.kind = d.kind.clone();
                        }
                    }
                });
            }
            for name in moved {
                let text = fill(&l.t("readdressed"), &[("name", &name)]);
                l.inform(text, cx);
            }
        });
        cx.notify();
    }

    /// The bulb's own name if it has one; otherwise a numbered bulb, not to
    /// be taken for one of the room's lights.
    fn device_name(&self, d: &DiscoveredDevice, cx: &gpui::App) -> String {
        let own = d.name.trim();
        if !own.is_empty() {
            return own.to_string();
        }
        let i = self
            .discovered
            .iter()
            .position(|x| x.id == d.id)
            .unwrap_or(self.discovered.len());
        fill(&t(cx, "bulb_n"), &[("n", &(i + 1).to_string())])
    }

    /// Whether a light of the room has bulb `device_id`.
    fn is_bound(&self, device_id: &str, cx: &gpui::App) -> bool {
        self.lueur
            .read(cx)
            .config
            .bindings
            .iter()
            .any(|b| b.device_id == device_id)
    }

    /// The found bulbs no light has yet.
    fn unbound(&self, cx: &gpui::App) -> Vec<DiscoveredDevice> {
        self.discovered
            .iter()
            .filter(|d| !self.is_bound(&d.id, cx))
            .cloned()
            .collect()
    }

    /// What the page reads out as the pairing or the import moves on.
    fn said(&self, cx: &gpui::App) -> SharedString {
        if let Some(p) = &self.pairing {
            return p.said(self, cx);
        }
        self.tuya
            .as_ref()
            .map(|tu| tu.said(self, cx))
            .unwrap_or_default()
    }

    fn header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = *theme(cx);
        let page_head = self.handle("page-head", false, cx);
        let l = self.lueur.read(cx);
        let unbound_lights = l
            .config
            .room
            .lights
            .iter()
            .any(|x| l.config.binding_for(&x.id).is_none());
        let fresh: Vec<DiscoveredDevice> =
            self.unbound(cx).into_iter().filter(|d| d.paired).collect();
        let add_all = (fresh.len() > 1 && !unbound_lights).then(|| {
            let named: Vec<_> = fresh
                .iter()
                .map(|d| (d.clone(), self.device_name(d, cx)))
                .collect();
            let lueur = self.lueur.clone();
            btn(
                "add-all",
                t(cx, "add_all"),
                Look::Default,
                false,
                false,
                None,
                &theme,
                move |_, _, cx| {
                    let named = named.clone();
                    lueur.update(cx, |l, cx| actions::add_devices(l, named, true, cx));
                },
            )
        });
        let tuya_busy = self
            .tuya
            .as_ref()
            .is_some_and(|tu| !matches!(tu, Tuya::Done { .. }));
        let tuya = btn(
            "tuya-open",
            t(cx, "tuya_open"),
            Look::Default,
            false,
            tuya_busy,
            None,
            &theme,
            cx.listener(|this, _, window, cx| this.open_tuya(window, cx)),
        )
        .tooltip(ui::tip(t(cx, "tuya_open_tip")));
        let search_label = t(
            cx,
            if self.scanning {
                "searching"
            } else if self.has_scanned {
                "search_again"
            } else {
                "search"
            },
        );
        let search = btn(
            "search",
            search_label,
            if self.has_scanned {
                Look::Default
            } else {
                Look::Primary
            },
            false,
            self.scanning,
            self.scanning
                .then(|| spinner("search-spin", 12.0, theme.text_3)),
            &theme,
            cx.listener(|this, _, _, cx| this.scan(SEARCH_MS, false, cx)),
        );
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(size::S4))
            .min_h(px(34.0))
            .child(div().track_focus(&page_head).child(ui::page_title(
                "page-title",
                t(cx, "nav_bulbs"),
                &theme,
            )))
            .child(
                div()
                    .flex()
                    .gap(px(size::S2))
                    .children(add_all)
                    .child(tuya)
                    .child(search),
            )
            .into_any_element()
    }
}

/// Search the network from a task: after the search already running when
/// `wait`, else not at all while one runs. False once the page is gone.
async fn scan(
    this: &WeakEntity<BulbsPage>,
    cx: &mut AsyncApp,
    timeout_ms: u64,
    quiet: bool,
    wait: bool,
) -> bool {
    loop {
        let Ok(scanning) = this.read_with(cx, |p, _| p.scanning) else {
            return false;
        };
        if !scanning {
            break;
        }
        if !wait {
            return true;
        }
        cx.background_executor()
            .timer(Duration::from_millis(200))
            .await;
    }
    let Ok(shell) = this.update(cx, |p, cx| {
        p.scanning = true;
        cx.notify();
        p.lueur.read(cx).shell.clone()
    }) else {
        return false;
    };
    let result = cx
        .background_executor()
        .spawn(async move { service::discover_devices(&shell, Some(timeout_ms)) })
        .await;
    this.update(cx, |p, cx| p.found(result, quiet, cx)).is_ok()
}

impl Render for BulbsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_lights = !self.lueur.read(cx).config.room.lights.is_empty();
        // Side by side once the room has lights and the window is wide
        // enough for two columns; before that, the network alone.
        let two_cols = has_lights && window.viewport_size().width >= px(1200.0);
        let said = self.said(cx);
        let header = self.header(cx);
        let check = self.device_check(cx);
        let pairing = self.pairing_panel(cx);
        let tuya = self.tuya_panel(cx);
        let network = self.network_column(cx);
        let lights = has_lights.then(|| self.lights_column(cx));
        let columns = if two_cols {
            div()
                .flex()
                .items_start()
                .gap(px(36.0))
                .child(div().flex_1().min_w_0().child(network))
                .children(lights.map(|c| div().flex_1().min_w_0().child(c)))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(size::S6))
                .child(network)
                .children(lights)
        };
        // The keyboard goes where the page sent it, now that the place it
        // goes to has its handle.
        if let Some(key) = self.focus_next.take() {
            let handle = self.handle(&key, false, cx);
            handle.focus(window, cx);
        }
        // A row centring one column that fills it up to its widest. The
        // column asks for no width of its own (`w_0`): sized by its words,
        // it would push the window as wide as its longest line.
        let page = div()
            .flex_1()
            .w_0()
            .min_w_0()
            .max_w(px(if has_lights { 1240.0 } else { 820.0 }))
            .flex()
            .flex_col()
            .gap(px(size::S5))
            .child(header)
            // Always present, so each change to the pairing or the import,
            // and each device to check, is read out.
            .child(
                div()
                    .id("bulbs-status")
                    .role(Role::Status)
                    .aria_label(SharedString::from(format!("{said} {}", self.check_news)))
                    .size_0()
                    .overflow_hidden(),
            )
            .children(check)
            .children(pairing)
            .children(tuya)
            .child(columns);
        div().flex().justify_center().child(page)
    }
}
