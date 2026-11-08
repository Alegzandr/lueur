//! What the Bulbs page shows, worded from the state for `bulbs.slint`.

use super::brands::{bulb_label, device_brand, supports_music};
use super::pairing::{
    self, PAIR_WINDOW, PairStep, Tuya, failure_text, tuya_summary, user_code_problem,
};
use super::{Local, check, device_name, first_visit, unbound, waiting_lights};
use crate::i18n::{fill, t, tn};
use crate::models::{keep, reuse};
use crate::state::Lueur;
use crate::{AppWindow, BulbCard, BulbsState, CheckBanner, DeviceRow, PairPanel, TuyaPanel};
use lueur_core::device::common::kelvin_to_rgb;
use lueur_core::scene::{LightState, Tint};
use lueur_shell::tuya_cloud::normalize_code;
use slint::{Color, ComponentHandle, SharedString};
use std::time::{Duration, Instant};

/// The tips open by themselves when a search found nothing at all, until
/// opened or closed by hand.
pub fn tips_open(local: &Local) -> bool {
    local
        .tips_open
        .unwrap_or(local.has_scanned && local.discovered.is_empty())
}

/// Smart Life bulbs are never found by searching alone: say so wherever
/// the list is, until some have been imported.
fn tuya_hint(l: &Lueur) -> bool {
    let b = &l.pages.bulbs;
    b.tuya.is_none()
        && !b.discovered.iter().any(|d| d.kind.family() == "tuya")
        && !l.config.bindings.iter().any(|x| x.kind.family() == "tuya")
}

fn hex(v: u32) -> Color {
    Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The two ends of the swatch a mode is drawn with, as the Lighting page
/// draws them.
fn mode_swatch(mode: &str) -> (Color, Color) {
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

/// A light's disc: no bulb, off, or what it gives (its white or colour,
/// fainter as it dims, or the gradient of the effect it plays). What a
/// bulb gives is content, so its real colour belongs here.
fn light_dot(l: &Lueur, light_id: &str, active: Color) -> (i32, Color, Color, Color) {
    let none = Color::default();
    if l.config.binding_for(light_id).is_none() {
        return (0, none, none, none);
    }
    let driven = l.status.running && l.status.light_ids.iter().any(|x| x == light_id);
    let state: LightState = l.config.states.get(light_id).copied().unwrap_or_default();
    if !driven && !state.on {
        return (1, none, none, none);
    }
    if driven {
        let mode = l
            .status
            .groups
            .iter()
            .find(|g| g.light_ids.iter().any(|x| x == light_id))
            .map(|g| g.mode)
            .unwrap_or("ambilight");
        let (from, to) = mode_swatch(mode);
        return (2, from, to, active.with_alpha(0.45));
    }
    let c = match state.tint {
        Tint::White { kelvin } => kelvin_to_rgb(kelvin),
        Tint::Color { r, g, b } => lueur_core::color::Rgb::new(r, g, b),
    };
    let alpha = (40.0 + 0.6 * state.bright as f32) / 100.0;
    let fill = Color::from_argb_f32(
        alpha,
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
    );
    let halo = Color::from_argb_u8(115, c.r, c.g, c.b);
    (2, fill, fill, halo)
}

fn pair_panel(l: &Lueur) -> PairPanel {
    let Some(p) = &l.pages.bulbs.pairing else {
        return PairPanel::default();
    };
    let name = device_name(&l.pages.bulbs, &p.device);
    let hue = p.hue();
    let mut panel = PairPanel {
        shown: true,
        title: fill(&t("pair_title"), &[("name", &name)]).into(),
        ..Default::default()
    };
    match &p.step {
        PairStep::Waiting => {
            let left = p.until.saturating_duration_since(Instant::now());
            panel.step = 0;
            panel.say = t(if hue { "pair_hue" } else { "pair_nanoleaf" }).into();
            panel.why = t(if hue { "pair_why" } else { "pair_why_nanoleaf" }).into();
            panel.left = left.as_secs_f32() / PAIR_WINDOW.as_secs_f32();
            panel.timer = if left > Duration::ZERO {
                let n = left.as_millis().div_ceil(1000).to_string();
                format!(
                    "{} · {}",
                    fill(&t("pair_left"), &[("n", &n)]),
                    t("pair_waiting")
                )
            } else {
                // Time is up, but the last answer from the device is still
                // on its way.
                t("pair_checking")
            }
            .into();
        }
        PairStep::Paired => {
            panel.step = 1;
            panel.say = fill(&t("paired"), &[("name", &name)]).into();
        }
        PairStep::Failed(e) => {
            panel.step = 2;
            panel.say = failure_text(e).into();
            panel.detail = e.detail.clone().unwrap_or_default().into();
        }
    }
    panel
}

fn tuya_panel(l: &Lueur, before: &TuyaPanel) -> TuyaPanel {
    let b = &l.pages.bulbs;
    let mut panel = TuyaPanel {
        qr: before.qr.clone(),
        ..Default::default()
    };
    let lines: Vec<SharedString>;
    match &b.tuya {
        None => {
            panel.lines = reuse(&before.lines, Vec::new());
            return panel;
        }
        Some(Tuya::Code { expired }) => {
            panel.step = 1;
            panel.expired = *expired;
            // Characters a code can never hold are pointed out while typing;
            // a code too short only on Continue, since it is short until
            // finished.
            let typed = normalize_code(&b.typed_code);
            let shown = b
                .code_problem
                .or((user_code_problem(&typed) == Some("tuya_code_chars"))
                    .then_some("tuya_code_chars"));
            panel.problem = shown.map(t).unwrap_or_default().into();
            lines = Vec::new();
        }
        Some(Tuya::Starting) => {
            panel.step = 2;
            lines = Vec::new();
        }
        Some(Tuya::Qr { image }) => {
            panel.step = 3;
            panel.qr = image.clone();
            lines = Vec::new();
        }
        Some(Tuya::Adding) => {
            panel.step = 4;
            lines = Vec::new();
        }
        Some(Tuya::Done {
            imported,
            added,
            missing,
            ..
        }) => {
            let ready = pairing::ready(l).len();
            panel.step = 5;
            lines = tuya_summary(*imported, *added, *missing, ready)
                .into_iter()
                .map(Into::into)
                .collect();
            if ready > 0 {
                panel.add_label = tn("tuya_add_n", ready).into();
            }
            panel.show_plan = ready == 0 && *added > 0;
            panel.show_search = *missing;
        }
    }
    panel.lines = reuse(&before.lines, lines);
    panel
}

fn device_rows(l: &Lueur) -> Vec<DeviceRow> {
    let b = &l.pages.bulbs;
    let list = unbound(l);
    // Addresses mean nothing to most people: a bulb is told apart by its
    // name, brand and model, and its address only shows when two share a
    // name.
    let names: Vec<String> = list.iter().map(|d| device_name(b, d)).collect();
    let has_waiting = !waiting_lights(l).is_empty();
    let pair_waiting = pairing::waiting(l);
    list.iter()
        .zip(&names)
        .map(|(d, name)| {
            let shared = names.iter().filter(|n| *n == name).count() > 1;
            let mut meta = device_brand(d);
            if shared {
                meta.push_str(&format!(" · {}", d.ip));
            }
            // What stops it from doing all a bulb can.
            let warn = if d.faster {
                t("pair_faster")
            } else if !d.paired {
                t("needs_pairing")
            } else if !supports_music(d) {
                t("no_music")
            } else {
                String::new()
            };
            let this_one = pair_waiting && b.pairing.as_ref().is_some_and(|p| p.device.id == d.id);
            let action = if !d.paired {
                0
            } else if has_waiting {
                1
            } else {
                2
            };
            DeviceRow {
                id: d.id.as_str().into(),
                name: name.as_str().into(),
                meta: meta.into(),
                warn: warn.into(),
                paired: d.paired,
                action,
                pair_label: t(if this_one {
                    "pairing"
                } else if d.faster {
                    "pair_again"
                } else {
                    "pair"
                })
                .into(),
                pair_tip: t(if d.faster {
                    "pair_faster_tip"
                } else {
                    "pair_tip"
                })
                .into(),
                pair_blocked: pair_waiting,
                pairing: this_one,
                faster: d.faster,
                use_label: fill(&t("labelled"), &[("name", name), ("what", &t("use_for"))]).into(),
            }
        })
        .collect()
}

fn light_cards(l: &Lueur, active: Color) -> Vec<BulbCard> {
    let c = &l.config;
    let labels: Vec<String> = c
        .bindings
        .iter()
        .map(|b| {
            let light = c.room.lights.iter().find(|x| x.id == b.light_id);
            bulb_label(b, light.map(|x| x.name.as_str()))
        })
        .collect();
    c.room
        .lights
        .iter()
        .map(|light| {
            let labelled = |what: &str| -> SharedString {
                fill(&t("labelled"), &[("name", &light.name), ("what", &t(what))]).into()
            };
            let binding = c.binding_for(&light.id);
            // Addresses only show to tell apart two bulbs described alike.
            let meta = match binding {
                Some(b) => {
                    let label = bulb_label(b, Some(&light.name));
                    if labels.iter().filter(|x| **x == label).count() > 1 {
                        format!("{label}\u{a0}· {}", b.ip)
                    } else {
                        label
                    }
                }
                None => t("pick_above"),
            };
            let (dot, dot_from, dot_to, dot_halo) = light_dot(l, &light.id, active);
            BulbCard {
                id: light.id.as_str().into(),
                name: light.name.as_str().into(),
                meta: meta.into(),
                bound: binding.is_some(),
                dot,
                dot_from,
                dot_to,
                dot_halo,
                plan_label: labelled("show_on_plan"),
                identify_label: labelled("identify"),
                unassign_label: labelled("unassign"),
                remove_label: labelled("remove_light"),
            }
        })
        .collect()
}

pub fn project(l: &Lueur, ui: &AppWindow) {
    let b = &l.pages.bulbs;
    // Asked for once the state is free again.
    if !b.visited || check::news_due(l) {
        slint::Timer::single_shot(Duration::ZERO, || {
            crate::rt::with(|l| {
                first_visit(l);
                check::note_news(l);
            })
        });
    }
    let g = ui.global::<BulbsState>();
    let has_lights = !l.config.room.lights.is_empty();
    let waiting = !waiting_lights(l).is_empty();
    let fresh = unbound(l).into_iter().filter(|d| d.paired).count();

    g.set_search_label(
        t(if b.scanning {
            "searching"
        } else if b.has_scanned {
            "search_again"
        } else {
            "search"
        })
        .into(),
    );
    g.set_search_primary(!b.has_scanned);
    g.set_scanning(b.scanning);
    g.set_add_all_shown(fresh > 1 && !waiting);
    g.set_tuya_busy(
        b.tuya
            .as_ref()
            .is_some_and(|tu| !matches!(tu, Tuya::Done { .. })),
    );
    g.set_said(
        format!("{} {}", pairing::said(l), b.check_news)
            .trim()
            .into(),
    );

    let checks = check::shown(l).into_iter().map(|d| CheckBanner {
        id: d.id.as_str().into(),
        text: check::say(&d).into(),
        needs_pairing: d.needs_pairing,
    });
    keep(g.get_checks(), |m| g.set_checks(m), checks);
    g.set_check_busy(b.check_busy);

    let panel = pair_panel(l);
    if g.get_pairing() != panel {
        g.set_pairing(panel);
    }
    g.set_detail_open(b.detail_open);
    let tuya = tuya_panel(l, &g.get_tuya());
    if g.get_tuya() != tuya {
        g.set_tuya(tuya);
    }

    let rows = device_rows(l);
    g.set_network_shown(!rows.is_empty() || !has_lights);
    g.set_found_count(
        if b.has_scanned && !b.scanning {
            tn("n_found", rows.len())
        } else {
            String::new()
        }
        .into(),
    );
    g.set_empty_kind(if b.scanning {
        0
    } else if b.has_scanned {
        1
    } else {
        2
    });
    g.set_tuya_hint(tuya_hint(l));
    keep(g.get_devices(), |m| g.set_devices(m), rows);
    let mut choices: Vec<SharedString> = waiting_lights(l)
        .into_iter()
        .map(|(_, name)| name.into())
        .collect();
    choices.push(t("new_light").into());
    keep(g.get_use_choices(), |m| g.set_use_choices(m), choices);
    g.set_tips_open(tips_open(b));

    g.set_has_lights(has_lights);
    let active = ui.global::<crate::Theme>().get_active();
    let cards = light_cards(l, active);
    keep(g.get_lights(), |m| g.set_lights(m), cards);
}
