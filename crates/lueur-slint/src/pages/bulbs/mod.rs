//! The Bulbs page: what the network offers on one side, the room's lights
//! on the other, and above them the pairing of a bridge, the Smart Life
//! sign-in and the check of devices nothing vouches for.
//!
//! What only this page needs lives in its [`Local`]: the bulbs last found,
//! the search running, the pairing and the sign-in under way. What it
//! changes in the room goes through [`actions`] to the app's [`Lueur`].
//! `bulbs.slint` draws what [`view`] words.

mod actions;
pub(crate) mod brands;
mod check;
mod pairing;
mod view;

use crate::i18n::{fill, t};
use crate::rt;
use crate::state::{Lueur, View};
use crate::{AppWindow, BulbsState};
use lueur_core::device::DiscoveredDevice;
use lueur_shell::error::AppResult;
use lueur_shell::service;
use pairing::{Pairing, Tuya};
use slint::ComponentHandle;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

pub use view::project;

/// How long a search listens, unless said otherwise.
const SEARCH_MS: u64 = 3000;

/// What to do once a search is over.
type Then = Box<dyn FnOnce(&mut Lueur)>;

/// What the page keeps for itself.
#[derive(Default)]
pub struct Local {
    /// The bulbs the last search found.
    discovered: Vec<DiscoveredDevice>,
    scanning: bool,
    has_scanned: bool,
    /// The page was shown once: the first visit searched.
    visited: bool,
    /// Searches asked for while one ran, to run after it, each with what
    /// to do once over.
    queued: Vec<(u64, bool, Then)>,
    pairing: Option<Pairing>,
    tuya: Option<Tuya>,
    /// Bumped by every sign-in started, reopened or closed, so an older one
    /// still waiting on the cloud knows to stop. Shared with its thread.
    tuya_run: Arc<AtomicU64>,
    /// What is wrong with the user code, once Continue was pressed.
    code_problem: Option<&'static str>,
    /// The code as last typed, to point out characters it can never hold.
    typed_code: String,
    /// A light whose name is being typed, and its name before.
    renaming: Option<(String, String)>,
    /// The tips, opened or closed by hand; until then they open by
    /// themselves when a search found nothing.
    tips_open: Option<bool>,
    /// The technical detail of a failed pairing, shown on request.
    detail_open: bool,
    /// True while a device check's answer is being sent.
    check_busy: bool,
    /// Unverified devices already seen, so only news is read out.
    check_seen: Vec<String>,
    check_news: String,
}

pub fn bind(ui: &AppWindow) {
    let g = ui.global::<BulbsState>();
    g.on_search(|| rt::with(|l| scan(l, SEARCH_MS, false, None)));
    g.on_add_all(|| rt::with(add_all));
    g.on_open_tuya(|| rt::with(pairing::open_tuya));
    g.on_check_not_mine(|id| rt::with(|l| check::not_mine(l, &id)));
    g.on_check_mine(|id| rt::with(|l| check::confirm(l, &id)));
    g.on_check_pair_again(|id| rt::with(|l| check::pair_again(l, &id)));
    g.on_pair(|id| {
        rt::with(|l| {
            if let Some(d) = l
                .pages
                .bulbs
                .discovered
                .iter()
                .find(|d| d.id == id.as_str())
            {
                let d = d.clone();
                pairing::pair(l, d);
            }
        })
    });
    g.on_use_for(|id, i| rt::with(|l| use_for(l, &id, i.max(0) as usize)));
    g.on_add(|id| rt::with(|l| give(l, &id, None)));
    g.on_identify_device(|id| {
        rt::with(|l| {
            if let Some(d) = l
                .pages
                .bulbs
                .discovered
                .iter()
                .find(|d| d.id == id.as_str())
            {
                let endpoint = d.endpoint();
                actions::identify(l, endpoint);
            }
        })
    });
    g.on_pair_retry(|| rt::with(pairing::retry));
    g.on_pair_close(|| rt::with(pairing::end));
    g.on_toggle_detail(|| rt::with(|l| l.pages.bulbs.detail_open = !l.pages.bulbs.detail_open));
    g.on_code_edited(|code| {
        rt::with(|l| {
            l.pages.bulbs.code_problem = None;
            l.pages.bulbs.typed_code = code.to_string();
        })
    });
    g.on_tuya_continue(|code| rt::with(|l| pairing::submit_code(l, &code)));
    g.on_tuya_cancel(|| rt::with(pairing::close_tuya));
    g.on_tuya_add_ready(|| rt::with(pairing::add_ready));
    g.on_tuya_plan(|| {
        rt::with(|l| {
            pairing::close_tuya(l);
            l.show(View::Room);
        })
    });
    g.on_tuya_search_again(|| rt::with(pairing::search_again));
    g.on_toggle_tips(|| {
        rt::with(|l| {
            let open = view::tips_open(&l.pages.bulbs);
            l.pages.bulbs.tips_open = Some(!open);
        })
    });
    g.on_open_plan(|| rt::with(|l| l.show(View::Room)));
    g.on_light_show_on_plan(|id| rt::with(|l| actions::show_on_plan(l, &id)));
    g.on_light_identify(|id| {
        rt::with(|l| {
            if let Some(b) = l.config.binding_for(&id) {
                let endpoint = b.endpoint();
                actions::identify(l, endpoint);
            }
        })
    });
    g.on_light_unassign(|id| rt::with(|l| actions::unassign(l, &id)));
    g.on_light_remove(|id| rt::with(|l| actions::remove_light(l, &id)));
    g.on_light_rename(|id, name| {
        rt::with(|l| {
            let b = &mut l.pages.bulbs;
            if b.renaming.as_ref().is_none_or(|(r, _)| r != id.as_str()) {
                let before = l
                    .config
                    .room
                    .lights
                    .iter()
                    .find(|x| x.id == id.as_str())
                    .map(|x| x.name.clone())
                    .unwrap_or_default();
                b.renaming = Some((id.to_string(), before));
            }
            actions::rename_light(l, &id, &name);
        })
    });
    g.on_light_rename_done(|id| {
        rt::with(|l| {
            let before = match l.pages.bulbs.renaming.take() {
                Some((r, before)) if r == id.as_str() => before,
                other => {
                    l.pages.bulbs.renaming = other;
                    String::new()
                }
            };
            actions::settle_name(l, &id, &before);
        })
    });

    // At launch, once the state is in place: bound bulbs may have moved
    // since the last time, and a quiet search finds them at their new
    // address before any light is set.
    slint::Timer::single_shot(Duration::ZERO, || {
        if !rt::ready() {
            return;
        }
        rt::with(|l| {
            let (bound, shown) = (!l.config.bindings.is_empty(), l.view == View::Bulbs);
            if bound || shown {
                scan(l, SEARCH_MS, !shown, None);
            }
            check::load(l);
        });
    });
}

/// Search the network. With `then`, after the search already running if
/// one runs, and `then` once it is over; without, not at all while one runs.
fn scan(l: &mut Lueur, timeout_ms: u64, quiet: bool, then: Option<Then>) {
    if l.pages.bulbs.scanning {
        if let Some(then) = then {
            l.pages.bulbs.queued.push((timeout_ms, quiet, then));
        }
        return;
    }
    l.pages.bulbs.scanning = true;
    l.run_then(
        move |s| service::discover_devices(s, Some(timeout_ms)),
        move |l, result| {
            found(l, result, quiet);
            if let Some(then) = then {
                then(l);
            }
            if !l.pages.bulbs.queued.is_empty() {
                let (timeout, quiet, then) = l.pages.bulbs.queued.remove(0);
                scan(l, timeout, quiet, Some(then));
            }
        },
    );
}

/// The first visit looks straight away rather than greeting with an empty
/// list; asked for by the page as it is drawn.
fn first_visit(l: &mut Lueur) {
    if l.view != View::Bulbs || l.pages.bulbs.visited {
        return;
    }
    l.pages.bulbs.visited = true;
    if !l.pages.bulbs.has_scanned && !l.pages.bulbs.scanning {
        scan(l, SEARCH_MS, false, None);
    }
}

/// Take what a search found. Bound bulbs found at a new address are
/// updated in place: the device id is stable, the address is not (nor, for
/// a Tuya bulb, the protocol version it was found speaking). `quiet` keeps a
/// failure of the search at launch from showing.
fn found(l: &mut Lueur, result: AppResult<Vec<DiscoveredDevice>>, quiet: bool) {
    l.pages.bulbs.scanning = false;
    let found = match result {
        Ok(found) => found,
        Err(e) => {
            if !quiet {
                l.fail(&e);
            }
            return;
        }
    };
    l.pages.bulbs.discovered = found;
    l.pages.bulbs.has_scanned = true;
    let found = l.pages.bulbs.discovered.clone();
    let names: Vec<String> = found
        .iter()
        .map(|d| device_name(&l.pages.bulbs, d))
        .collect();
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
        l.edit(|c| {
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
        l.inform(text);
    }
}

/// The bulb's own name if it has one; otherwise a numbered bulb, not to be
/// taken for one of the room's lights.
fn device_name(local: &Local, d: &DiscoveredDevice) -> String {
    let own = d.name.trim();
    if !own.is_empty() {
        return own.to_string();
    }
    let i = local
        .discovered
        .iter()
        .position(|x| x.id == d.id)
        .unwrap_or(local.discovered.len());
    fill(&t("bulb_n"), &[("n", &(i + 1).to_string())])
}

/// Whether a light of the room has bulb `device_id`.
fn is_bound(l: &Lueur, device_id: &str) -> bool {
    l.config.bindings.iter().any(|b| b.device_id == device_id)
}

/// The found bulbs no light has yet.
fn unbound(l: &Lueur) -> Vec<DiscoveredDevice> {
    l.pages
        .bulbs
        .discovered
        .iter()
        .filter(|d| !is_bound(l, &d.id))
        .cloned()
        .collect()
}

/// The lights of the room still without a bulb, as "Use for…" lists them.
fn waiting_lights(l: &Lueur) -> Vec<(String, String)> {
    let c = &l.config;
    c.room
        .lights
        .iter()
        .filter(|x| c.binding_for(&x.id).is_none())
        .map(|x| (x.id.clone(), x.name.clone()))
        .collect()
}

/// Every paired bulb found and not in the room, as new lights.
fn add_all(l: &mut Lueur) {
    let named: Vec<_> = unbound(l)
        .into_iter()
        .filter(|d| d.paired)
        .map(|d| {
            let name = device_name(&l.pages.bulbs, &d);
            (d, name)
        })
        .collect();
    actions::add_devices(l, named, true);
}

/// "Use for…" picked choice `i`: one of the lights waiting for a bulb, or,
/// past them, a new light.
fn use_for(l: &mut Lueur, device_id: &str, i: usize) {
    let target = waiting_lights(l).get(i).map(|(id, _)| id.clone());
    give(l, device_id, target);
}

/// Give bulb `device_id` to light `target`, or to a new one.
fn give(l: &mut Lueur, device_id: &str, target: Option<String>) {
    let Some(d) = l
        .pages
        .bulbs
        .discovered
        .iter()
        .find(|d| d.id == device_id)
        .cloned()
    else {
        return;
    };
    let name = device_name(&l.pages.bulbs, &d);
    match target {
        None => actions::add_devices(l, vec![(d, name)], true),
        Some(id) => actions::assign_device(l, &id, &d, &name),
    }
}
