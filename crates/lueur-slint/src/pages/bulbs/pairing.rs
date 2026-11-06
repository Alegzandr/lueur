//! Pairing Hue bridges and Nanoleaf controllers, and the Smart Life sign-in
//! that imports Tuya bulbs.
//!
//! Each waits on a device or on the cloud for a while, on a thread of its
//! own. A pairing or a sign-in cancelled or replaced meanwhile is told by a
//! flag or a counter it shares with its thread, and stops.

use super::{SEARCH_MS, actions, device_name, is_bound, scan};
use crate::BulbsState;
use crate::i18n::{error_text, fill, t, tn};
use crate::rt;
use crate::state::Lueur;
use lueur_core::device::DiscoveredDevice;
use lueur_shell::error::AppError;
use lueur_shell::service::{self, TuyaLogin};
use lueur_shell::tuya_cloud::normalize_code;
use slint::ComponentHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a bridge or controller waits for its button to be pressed.
pub const PAIR_WINDOW: Duration = Duration::from_secs(30);
/// How long a Smart Life QR code waits to be scanned.
const QR_WINDOW: Duration = Duration::from_secs(180);

/// A bridge or controller being paired, and how it went.
pub struct Pairing {
    pub device: DiscoveredDevice,
    pub step: PairStep,
    pub until: Instant,
    /// Set once the pairing is closed or replaced.
    cancelled: Arc<AtomicBool>,
}

pub enum PairStep {
    /// The page asks for the button and counts down.
    Waiting,
    Paired,
    Failed(AppError),
}

impl Pairing {
    pub fn hue(&self) -> bool {
        self.device.kind.family() == "hue"
    }
}

/// Where the Smart Life sign-in stands.
pub enum Tuya {
    /// Asking for the user code; `expired` after a QR code went unscanned.
    Code { expired: bool },
    /// The code was sent; the QR code is on its way.
    Starting,
    /// The QR code, drawn by the shell from a token it checked, never
    /// fetched.
    Qr { image: slint::Image },
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
pub fn tuya_summary(imported: usize, added: usize, missing: bool, ready: usize) -> Vec<String> {
    if imported == 0 {
        return vec![t("tuya_none")];
    }
    let mut out = Vec::new();
    if added > 0 {
        out.push(tn("tuya_added", added));
    }
    if ready > 0 {
        out.push(tn("tuya_ready", ready));
    }
    if missing {
        out.push(t("tuya_missing"));
    }
    if out.is_empty() {
        out.push(t("tuya_already"));
    }
    out
}

/// A failed pairing in words: the button not pressed in time, or why it
/// did not work.
pub fn failure_text(e: &AppError) -> String {
    if e.code == "pair_waiting" {
        t("pair_expired")
    } else {
        format!(
            "{} {}",
            t("pair_failed"),
            error_text(crate::i18n::current(), e)
        )
    }
}

/// Whether a device waits for its button.
pub fn waiting(l: &Lueur) -> bool {
    l.pages
        .bulbs
        .pairing
        .as_ref()
        .is_some_and(|p| matches!(p.step, PairStep::Waiting))
}

/// Pair with a Hue bridge or Nanoleaf controller: the page asks the user to
/// press its button and counts down, while Lueur keeps asking the device
/// for up to 30 seconds. The outcome stays on the page until it is closed.
pub fn pair(l: &mut Lueur, device: DiscoveredDevice) {
    if waiting(l) {
        return;
    }
    if let Some(old) = l.pages.bulbs.pairing.take() {
        old.cancelled.store(true, Ordering::Relaxed);
    }
    let until = Instant::now() + PAIR_WINDOW;
    let cancelled = Arc::new(AtomicBool::new(false));
    l.pages.bulbs.pairing = Some(Pairing {
        device: device.clone(),
        step: PairStep::Waiting,
        until,
        cancelled: Arc::clone(&cancelled),
    });
    l.pages.bulbs.detail_open = false;
    tick();
    let shell = Arc::clone(&l.shell);
    let stop = Arc::clone(&cancelled);
    rt::spawn(
        move || {
            loop {
                match service::pair_device(&shell, device.clone()) {
                    Ok(()) => break Ok(()),
                    Err(e) => {
                        if e.code != "pair_waiting"
                            || Instant::now() > until
                            || stop.load(Ordering::Relaxed)
                        {
                            break Err(e);
                        }
                        std::thread::sleep(Duration::from_millis(1500));
                    }
                }
            }
        },
        move |result| {
            rt::with(|l| {
                let paired = result.is_ok();
                // Even when cancelled meanwhile: the shell keeps the pairing,
                // so the config here must catch up with it.
                if paired {
                    l.reload_config();
                }
                let current = !cancelled.load(Ordering::Relaxed);
                if current && let Some(p) = &mut l.pages.bulbs.pairing {
                    p.step = match result {
                        Ok(()) => PairStep::Paired,
                        Err(e) => PairStep::Failed(e),
                    };
                }
                // Its lamps now show up: search for them.
                if paired && current {
                    scan(l, SEARCH_MS, false, Some(Box::new(|_| {})));
                }
            })
        },
    );
}

/// Redraw the countdown four times a second while a device waits for its
/// button.
fn tick() {
    slint::Timer::single_shot(Duration::from_millis(250), || {
        if rt::with(|l| waiting(l)) {
            tick();
        }
    });
}

/// Close the pairing panel.
pub fn end(l: &mut Lueur) {
    if let Some(p) = l.pages.bulbs.pairing.take() {
        p.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Retry with the device as last found, in case it moved meanwhile.
pub fn retry(l: &mut Lueur) {
    let Some(device) = l.pages.bulbs.pairing.as_ref().map(|p| p.device.clone()) else {
        return;
    };
    let d = l
        .pages
        .bulbs
        .discovered
        .iter()
        .find(|x| x.id == device.id)
        .cloned()
        .unwrap_or(device);
    pair(l, d);
}

/// A new sign-in, or none: the one under way, if any, stops. Returns the
/// number of the new one.
fn next_run(l: &mut Lueur) -> u64 {
    l.pages.bulbs.tuya_run.fetch_add(1, Ordering::Relaxed) + 1
}

fn is_run(l: &Lueur, run: u64) -> bool {
    l.pages.bulbs.tuya_run.load(Ordering::Relaxed) == run
}

/// Set the code field's words. Only to tidy or empty it: while the user
/// types, the field keeps what they type.
fn set_code(code: &str) {
    if let Some(ui) = rt::ui() {
        ui.global::<BulbsState>().set_code(code.into());
    }
}

/// Open the Smart Life sign-in, at the user-code step.
pub fn open_tuya(l: &mut Lueur) {
    next_run(l);
    l.pages.bulbs.tuya = Some(Tuya::Code { expired: false });
    l.pages.bulbs.code_problem = None;
    l.pages.bulbs.typed_code.clear();
    set_code("");
}

pub fn close_tuya(l: &mut Lueur) {
    next_run(l);
    l.pages.bulbs.tuya = None;
}

/// Continue: the code as typed, without the spaces and dashes people put
/// in to read it, checked, then sent.
pub fn submit_code(l: &mut Lueur, typed: &str) {
    if !matches!(l.pages.bulbs.tuya, Some(Tuya::Code { .. })) {
        return;
    }
    let code = normalize_code(typed);
    set_code(&code);
    l.pages.bulbs.typed_code = code.clone();
    l.pages.bulbs.code_problem = user_code_problem(&code);
    if l.pages.bulbs.code_problem.is_none() {
        sign_in(l, code);
    }
}

/// Sign in to Smart Life with the user code: show the QR code, wait (three
/// minutes at most) for it to be scanned, import the bulbs, then find and
/// add them (see [`finish_import`]).
fn sign_in(l: &mut Lueur, code: String) {
    let run = next_run(l);
    l.pages.bulbs.tuya = Some(Tuya::Starting);
    let shell = Arc::clone(&l.shell);
    let runs = Arc::clone(&l.pages.bulbs.tuya_run);
    // Cancelled or replaced meanwhile, a failure is no longer news.
    let failed = move |e: AppError| {
        rt::post(move || {
            rt::with(|l| {
                if is_run(l, run) {
                    l.pages.bulbs.tuya = Some(Tuya::Code { expired: false });
                    l.fail(&e);
                }
            })
        });
    };
    std::thread::spawn(move || {
        let TuyaLogin { token, svg } = match service::tuya_login_start(&code) {
            Ok(login) => login,
            Err(e) => return failed(e),
        };
        rt::post(move || {
            rt::with(|l| {
                if !is_run(l, run) {
                    return;
                }
                match slint::Image::load_from_svg_data(svg.as_bytes()) {
                    Ok(image) => l.pages.bulbs.tuya = Some(Tuya::Qr { image }),
                    Err(_) => {
                        l.pages.bulbs.tuya = Some(Tuya::Code { expired: false });
                        l.fail(&AppError::internal("the QR code cannot be drawn"));
                    }
                }
            })
        });
        let deadline = Instant::now() + QR_WINDOW;
        loop {
            if runs.load(Ordering::Relaxed) != run {
                return;
            }
            match service::tuya_login_poll(&shell, &token, &code) {
                Err(e) => return failed(e),
                Ok(Some(imported)) => {
                    // Imported even if the page moved on meanwhile: the shell
                    // keeps the keys, so the config here must catch up.
                    rt::post(move || {
                        rt::with(|l| {
                            l.reload_config();
                            if is_run(l, run) {
                                finish_import(l, imported, run);
                            }
                        })
                    });
                    return;
                }
                Ok(None) => {
                    if Instant::now() > deadline {
                        rt::post(move || {
                            rt::with(|l| {
                                if is_run(l, run) {
                                    l.pages.bulbs.tuya = Some(Tuya::Code { expired: true });
                                }
                            })
                        });
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });
}

/// Look again for the Smart Life bulbs that did not answer.
pub fn search_again(l: &mut Lueur) {
    let Some(Tuya::Done { imported, .. }) = l.pages.bulbs.tuya else {
        return;
    };
    let run = next_run(l);
    finish_import(l, imported, run);
}

/// The Smart Life bulbs found after an import, still waiting for a light.
pub fn ready(l: &Lueur) -> Vec<DiscoveredDevice> {
    let Some(Tuya::Done { ready, .. }) = &l.pages.bulbs.tuya else {
        return Vec::new();
    };
    l.pages
        .bulbs
        .discovered
        .iter()
        .filter(|d| ready.contains(&d.id) && !is_bound(l, &d.id))
        .cloned()
        .collect()
}

/// Add the Smart Life bulbs left for the user to place, as new lights.
pub fn add_ready(l: &mut Lueur) {
    // Some may have been given to a light by hand meanwhile.
    let named: Vec<_> = ready(l)
        .into_iter()
        .map(|d| {
            let name = device_name(&l.pages.bulbs, &d);
            (d, name)
        })
        .collect();
    let n = named.len();
    actions::add_devices(l, named, false);
    if let Some(Tuya::Done { added, ready, .. }) = &mut l.pages.bulbs.tuya {
        *added += n;
        ready.clear();
    }
}

/// After a Smart Life import: look for the bulbs on the network and add the
/// ones found to the room, so importing really ends with bulbs in the room.
/// When lights drawn without a bulb are waiting, adding new lights would
/// duplicate them: the user is asked to pick instead, with a button to add
/// them all anyway.
fn finish_import(l: &mut Lueur, imported: usize, run: u64) {
    if !is_run(l, run) {
        return;
    }
    l.pages.bulbs.tuya = Some(Tuya::Adding);
    let done = move |l: &mut Lueur| complete_import(l, imported, run);
    if imported > 0 {
        scan(l, 6000, false, Some(Box::new(done)));
    } else {
        done(l);
    }
}

fn complete_import(l: &mut Lueur, imported: usize, run: u64) {
    if !is_run(l, run) {
        return;
    }
    let found: Vec<DiscoveredDevice> = l
        .pages
        .bulbs
        .discovered
        .iter()
        .filter(|d| d.kind.family() == "tuya")
        .cloned()
        .collect();
    let ready: Vec<DiscoveredDevice> = found
        .iter()
        .filter(|d| d.paired && !is_bound(l, &d.id))
        .cloned()
        .collect();
    let lights_waiting = {
        let c = &l.config;
        c.room.lights.iter().any(|x| c.binding_for(&x.id).is_none())
    };
    let added = if lights_waiting { 0 } else { ready.len() };
    if added > 0 {
        let named: Vec<_> = ready
            .iter()
            .map(|d| (d.clone(), device_name(&l.pages.bulbs, d)))
            .collect();
        actions::add_devices(l, named, false);
    }
    l.pages.bulbs.tuya = Some(Tuya::Done {
        imported,
        added,
        ready: if added > 0 {
            Vec::new()
        } else {
            ready.iter().map(|d| d.id.clone()).collect()
        },
        // Every Smart Life bulb heard, bound or not, against every one
        // imported: fewer heard means some did not answer, though not how
        // many, since bound ones may come from another account.
        missing: found.len() < imported,
    });
}

/// What the page reads out as the pairing or the import moves on.
pub fn said(l: &Lueur) -> String {
    if let Some(p) = &l.pages.bulbs.pairing {
        return match &p.step {
            PairStep::Waiting => t(if p.hue() { "pair_hue" } else { "pair_nanoleaf" }),
            PairStep::Paired => fill(
                &t("paired"),
                &[("name", &device_name(&l.pages.bulbs, &p.device))],
            ),
            PairStep::Failed(e) => failure_text(e),
        };
    }
    match &l.pages.bulbs.tuya {
        Some(Tuya::Adding) => t("tuya_adding"),
        Some(Tuya::Done {
            imported,
            added,
            missing,
            ..
        }) => tuya_summary(*imported, *added, *missing, ready(l).len()).join(" "),
        Some(Tuya::Code { expired: true }) => t("tuya_expired"),
        _ => String::new(),
    }
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
