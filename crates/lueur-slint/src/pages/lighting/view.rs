//! What the Lighting page shows, worded from the state: every list and
//! value of the `LightingState` global.

use super::model::{
    self, ANIMATED, Look, MAX_KELVIN, MIN_KELVIN, PACED, PALETTE, Room, current_feel,
    distinct_looks, kelvin_rgb, photo_name, picture_of, rgb_hue, scene_named, show_pace, times,
    tint_rgb,
};
use crate::i18n::{fill, pct, t, tn};
use crate::models::{keep, old, reuse};
use crate::state::Lueur;
use crate::theme::oklch;
use crate::{
    AppWindow, Choice, Disc, LightCard, LightingState, ModeTile, SceneCard, SetupStep, Slice,
    Swatch, ZoneRow,
};
use lueur_core::config::AudioSource;
use lueur_core::scene::{MOODS, NATURAL, Tint};
use slint::{Color, ComponentHandle, SharedString};
use std::collections::BTreeSet;

fn rgb8([r, g, b]: [u8; 3], a: f32) -> Color {
    Color::from_argb_u8((a.clamp(0.0, 1.0) * 255.0).round() as u8, r, g, b)
}

/// The two ends of a mode's swatch: a picture's amber, violet and blue for
/// the screen, each show's own colours.
fn swatch_ends(mode: &str) -> ([u8; 3], [u8; 3]) {
    match mode {
        "music" => ([0xff, 0x00, 0x50], [0x00, 0x78, 0xff]),
        "rainbow" => ([0xff, 0x96, 0x00], [0x1e, 0x32, 0xff]),
        "sunset" => ([0xff, 0x8c, 0x0a], [0xbe, 0x1e, 0xb4]),
        "ocean" => ([0x14, 0x23, 0xff], [0x00, 0xeb, 0xaf]),
        "candle" => ([0xff, 0xc4, 0x6a], [0xff, 0x4e, 0x04]),
        _ => ([0xd9, 0x7a, 0x2b], [0x3b, 0x7b, 0xe0]),
    }
}

/// How a light glows now, for its disc.
fn disc(room: &Room, id: &str, active: Color) -> Disc {
    let bound = room.bound(id);
    if !bound {
        return Disc {
            kind: 0,
            ..Default::default()
        };
    }
    if !room.on(id) {
        return Disc {
            kind: 1,
            ..Default::default()
        };
    }
    if room.driven(id) {
        let (a, b) = swatch_ends(room.mode_of(id).unwrap_or("ambilight"));
        return Disc {
            kind: 2,
            a: rgb8(a, 1.0),
            b: rgb8(b, 1.0),
            halo: active.with_alpha(0.45),
        };
    }
    let s = room.state(id);
    let c = tint_rgb(s.tint);
    let strength = (40.0 + 0.6 * f32::from(s.bright)) / 100.0;
    Disc {
        kind: 3,
        a: rgb8(c, strength),
        b: Color::default(),
        halo: rgb8(c, 0.45),
    }
}

fn photo(path: &str) -> slint::Image {
    crate::images::photo(photo_name(path))
}

fn ss(s: impl AsRef<str>) -> SharedString {
    SharedString::from(s.as_ref())
}

/// The sleep timer's words: its state, the minutes on the clock, its name
/// for screen readers and the note under the panel.
fn sleep(l: &Lueur, g: &LightingState) {
    let left = l
        .sleep
        .as_ref()
        .map(|(status, at)| (status.left as f32 - at.elapsed().as_secs_f32()).max(0.0));
    let title = t("sleep_timer");
    let state = match left {
        None => t("sleep_in"),
        Some(s) if s <= 60.0 => t("sleep_fading"),
        Some(s) => fill(
            &t("sleep_left"),
            &[("n", &((s / 60.0).ceil() as u32).to_string())],
        ),
    };
    let short = left.map(|s| {
        let n = if s <= 60.0 {
            "<1".to_string()
        } else {
            ((s / 60.0).ceil() as u32).to_string()
        };
        format!("{n} min")
    });
    // Lueur keeps the computer awake while a timer runs, and stays open
    // even when closing the window would quit it.
    let note = if left.is_some() {
        if l.config.behavior.minimize_to_tray {
            t("sleep_keep_on")
        } else {
            format!("{} {}", t("sleep_keep_on"), t("sleep_stays_open"))
        }
    } else {
        t("sleep_timer_tip")
    };
    g.set_sleep_running(left.is_some());
    g.set_sleep_short(ss(short.unwrap_or_default()));
    g.set_sleep_label(ss(match left {
        None => title,
        Some(_) => format!("{title}: {state}"),
    }));
    g.set_sleep_state(ss(state));
    g.set_sleep_note(ss(note));
}

fn setup(l: &Lueur, g: &LightingState) {
    let bound = l.config.bindings.len();
    let arranged = l.config.interface.room_arranged;
    let live = l.status.running;
    let pending = l.pages.lighting.last_step_pending.get();
    let shown = !(bound > 0 && arranged && !pending);
    g.set_setup_shown(shown);
    if !shown {
        return;
    }
    let steps = [
        (bound > 0, "step1_title", "step1_btn"),
        (arranged, "step2_title", "step2_btn"),
        (live, "step3_title", "start_ambilight"),
    ];
    let next = steps.iter().position(|s| !s.0);
    // The room as it will be once the next step is done: dark until there
    // are bulbs, lit softly once they are in, then following the screen.
    let picture = match next {
        Some(0) | None => "off",
        Some(1) => "relax",
        _ => "ambilight",
    };
    g.set_setup_picture(photo(picture_of(picture)));
    let rows = steps
        .iter()
        .enumerate()
        .map(|(i, &(done, title, action))| SetupStep {
            title: ss(t(title)),
            done,
            next: Some(i) == next,
            action: ss(t(action)),
            enabled: !(i == 2 && (l.busy || bound == 0)),
        });
    keep(g.get_steps(), |m| g.set_steps(m), rows);
}

fn lights(l: &Lueur, g: &LightingState, active: Color) {
    let room = Room::of(l);
    let lang = l.lang();
    let pickable = l
        .config
        .room
        .lights
        .iter()
        .filter(|x| room.bound(&x.id))
        .count();
    // Picking only means something with two lights or more.
    let selectable = pickable > 1;
    let whole = room.picked().is_none();
    g.set_selectable(selectable);
    g.set_whole(whole);
    let cards = l.config.room.lights.iter().map(|light| {
        let id = light.id.as_str();
        let name = light.name.clone();
        let bound = room.bound(id);
        let mode = room.mode_of(id);
        let other = if bound { room.elsewhere(id) } else { None };
        let picked = selectable && !whole && l.target.iter().any(|x| x == id);
        let on = room.on(id);
        // What it does, in words.
        let (sub, sub_tip, live, plain) = if !bound {
            (t("no_bulb"), String::new(), false, false)
        } else if mode.is_some() && other.is_some() {
            let host = other.clone().unwrap_or_default();
            if host.is_empty() {
                (t("held_by_other"), t("held_other_tip"), false, false)
            } else {
                (
                    fill(&t("held_by"), &[("host", &host)]),
                    fill(&t("held_tip"), &[("host", &host)]),
                    false,
                    false,
                )
            }
        } else if mode.is_some() && room.reconnecting(id) {
            (t("reconnecting"), t("reconnecting_tip"), false, false)
        } else if mode.is_none() && other.as_ref().is_some_and(|h| !h.is_empty()) {
            let host = other.clone().unwrap_or_default();
            (
                fill(&t("elsewhere_sub"), &[("host", &host)]),
                String::new(),
                false,
                false,
            )
        } else {
            (
                room.doing_of(id).name(lang),
                String::new(),
                mode.is_some(),
                true,
            )
        };
        let bright = if plain && on {
            pct(lang, f32::from(room.bright_of(id)) / 100.0)
        } else {
            String::new()
        };
        let labelled = |what: &str| fill(&t("labelled"), &[("name", &name), ("what", what)]);
        // The one button on the card: a place for a bulb, taking a bulb
        // back, or the light's power.
        let (action, label, text, tip) = if !bound {
            let tip = t("assign");
            (0, labelled(&tip), String::new(), tip)
        } else if mode.is_some() && other.is_some() {
            let label = t("take_over");
            (1, labelled(&label), label, t("take_over_tip"))
        } else {
            let what = t(if on { "turn_off" } else { "turn_on" });
            (2, labelled(&what), String::new(), what)
        };
        let mut description = if bright.is_empty() {
            sub.clone()
        } else {
            format!("{sub}, {bright}")
        };
        if !sub_tip.is_empty() {
            description = format!("{description}. {sub_tip}");
        }
        LightCard {
            id: ss(id),
            name: ss(&name),
            sub: ss(sub),
            sub_tip: ss(sub_tip),
            live,
            bright: ss(bright),
            bound,
            picked,
            selectable,
            action,
            action_label: ss(label),
            action_text: ss(text),
            action_tip: ss(tip),
            busy: action == 2 && l.power_busy.iter().any(|x| x == id),
            disc: disc(&room, id, active),
            description: ss(description),
        }
    });
    keep(g.get_lights(), |m| g.set_lights(m), cards);
}

fn scenes(l: &Lueur, g: &LightingState) {
    let room = Room::of(l);
    let local = &l.pages.lighting;
    let current = g.get_scenes();
    let cards: Vec<SceneCard> = l
        .config
        .scenes
        .iter()
        .enumerate()
        .map(|(i, scene)| {
            // A scene as the room it makes: the photograph of each thing its
            // lights do, side by side, in the order of the room.
            let looks: Vec<Look> = distinct_looks(
                l.config.room.lights.iter().filter_map(|x| {
                    scene
                        .lights
                        .get(&x.id)
                        .filter(|_| room.bound(&x.id))
                        .map(Look::of_scene)
                }),
                3,
            );
            let slices: Vec<Slice> = looks
                .iter()
                .map(|look| Slice {
                    picture: photo(look.picture),
                    tinted: look.tint.is_some(),
                    tint: rgb8(look.tint.unwrap_or_default(), 1.0),
                })
                .collect();
            let pending = l.pending.as_deref() == Some(scene.id.as_str());
            SceneCard {
                id: ss(&scene.id),
                name: ss(&scene.name),
                on: room.shows_scene(scene),
                pending,
                disabled: l.busy && !pending,
                slices: reuse(&old(&current, i).slices, slices),
                confirming: local.confirming.as_deref() == Some(scene.id.as_str()),
                ask: ss(fill(&t("scene_delete_ask"), &[("name", &scene.name)])),
                delete_label: ss(fill(&t("scene_delete"), &[("name", &scene.name)])),
            }
        })
        .collect();
    keep(current, |m| g.set_scenes(m), cards);
    let typed = local.typed.trim();
    let replacing = (!typed.is_empty())
        .then(|| scene_named(&l.config.scenes, typed))
        .flatten();
    g.set_naming_confirm(ss(t(if replacing.is_some() {
        "scene_replace"
    } else {
        "scene_save_confirm"
    })));
    g.set_naming_hint(ss(match replacing {
        Some(s) => fill(&t("scene_replace_tip"), &[("name", &s.name)]),
        None => t("scene_save_tip"),
    }));
}

fn selection(l: &Lueur, g: &LightingState, active: Color) {
    let room = Room::of(l);
    let lang = l.lang();
    let pickable: Vec<_> = l
        .config
        .room
        .lights
        .iter()
        .filter(|x| room.bound(&x.id))
        .collect();
    let name = if room.picked().is_none() {
        if pickable.len() == 1 {
            pickable[0].name.clone()
        } else {
            t("whole_room")
        }
    } else {
        let picked: Vec<_> = pickable
            .iter()
            .filter(|x| l.target.contains(&x.id))
            .collect();
        if picked.len() == 1 {
            picked[0].name.clone()
        } else {
            tn("n_lights", picked.len())
        }
    };
    let ids = room.mood_targets();
    let summary = room.summary(&ids, lang);
    let now = t("mode_now");
    g.set_selection_label(ss(match &summary {
        Some((text, _)) => format!("{name}. {now} {text}"),
        None => name.clone(),
    }));
    g.set_selection_name(ss(name));
    g.set_selection_live(summary.as_ref().is_some_and(|(_, live)| *live));
    g.set_selection_summary(ss(summary.map(|(s, _)| s).unwrap_or_default()));
    let discs = ids.iter().take(5).map(|id| disc(&room, id, active));
    keep(g.get_selection_discs(), |m| g.set_selection_discs(m), discs);
}

fn modes(l: &Lueur, g: &LightingState) {
    let room = Room::of(l);
    let pending = l.pending.clone();
    let ids = room.mood_targets();
    let names = |ids: &[String]| {
        ids.iter()
            .map(|id| room.name_of(id))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut cell = 0;
    let mut place = |span: i32| {
        let at = cell;
        cell += span;
        at
    };

    // --- Fixed: the moods ---
    let mut fixed = Vec::new();
    for (id, tip) in std::iter::once((NATURAL, Some(t("mood_natural_tip"))))
        .chain(MOODS.iter().map(|m| (m.id, None)))
    {
        let showing: Vec<String> = ids
            .iter()
            .filter(|x| room.mood_of(x) == Some(id))
            .cloned()
            .collect();
        let plays = if !ids.is_empty() && showing.len() == ids.len() {
            2
        } else if showing.is_empty() {
            0
        } else {
            1
        };
        let span = if id == NATURAL { 2 } else { 1 };
        fixed.push(ModeTile {
            id: ss(id),
            name: ss(t(&format!("mood_{id}"))),
            plays,
            tip: ss(if plays == 1 {
                names(&showing)
            } else {
                tip.unwrap_or_default()
            }),
            cell: place(span),
            span,
            animated: false,
            pending: pending.as_deref() == Some(id),
            disabled: pending.is_some(),
            picture: photo(picture_of(id)),
        });
    }

    // --- Animated: the screen and the shows ---
    let targets = room.effect_targets();
    let mut animated = Vec::new();
    for (cell, id) in ANIMATED.into_iter().enumerate() {
        let playing: Vec<String> = targets
            .iter()
            .filter(|x| room.mode_of(x) == Some(id))
            .cloned()
            .collect();
        let plays = if !targets.is_empty() && playing.len() == targets.len() {
            2
        } else if playing.is_empty() {
            0
        } else {
            1
        };
        // Played by some of the lights picked: who, said on hover rather
        // than in the tile, which only carries a dashed ring.
        let tip = match plays {
            2 => t("click_to_stop"),
            1 => format!("{}. {}", names(&playing), t("click_to_extend")),
            _ => String::new(),
        };
        let is_pending = pending.as_deref() == Some(id);
        animated.push(ModeTile {
            id: ss(id),
            name: ss(t(&format!("mode_{id}"))),
            plays,
            tip: ss(tip),
            cell: cell as i32,
            span: 1,
            animated: true,
            pending: is_pending,
            disabled: l.busy && !is_pending,
            picture: photo(picture_of(id)),
        });
    }
    keep(g.get_fixed(), |m| g.set_fixed(m), fixed);
    keep(g.get_animated(), |m| g.set_animated(m), animated);
}

/// The settings of what plays on the lights picked: what of the picture
/// each one mirrors and how fast it follows, the shows' pace, and what the
/// music listens to.
fn mode_settings(l: &Lueur, g: &LightingState, dark: bool) {
    use lueur_core::geometry::ScreenEdge;
    let room = Room::of(l);
    let lang = l.lang();
    let picked = room.mood_targets();
    let modes: Vec<Option<&str>> = picked.iter().map(|id| room.mode_of(id)).collect();
    let screen = l.config.room.screen;
    let screen_lights: Vec<(String, String, lueur_core::geometry::LightMapping)> = picked
        .iter()
        .filter(|id| room.mode_of(id) == Some("ambilight"))
        .filter_map(|id| {
            let light = l.config.room.lights.iter().find(|x| &x.id == id)?;
            Some((
                id.clone(),
                light.name.clone(),
                lueur_core::geometry::map_light(&screen, light),
            ))
        })
        .collect();
    let paced = modes.iter().any(|m| m.is_some_and(|m| PACED.contains(&m)));
    let music = modes.contains(&Some("music"));
    let has_screen = !screen_lights.is_empty();
    g.set_mode_settings(has_screen || paced || music);
    g.set_has_screen(has_screen);
    g.set_paced(paced);
    g.set_music(music);
    // The screen cannot be read: said in words, the colour only marks it.
    g.set_screen_lost(has_screen && l.status.screen_lost);

    let one = picked.len() == 1;
    let ink = if dark {
        oklch(0.2, 0.0, 0.0, 1.0)
    } else {
        oklch(1.0, 0.0, 0.0, 1.0)
    };
    let zones = screen_lights
        .into_iter()
        .enumerate()
        .map(|(i, (id, name, mapping))| {
            let (letter_key, zone_key) = match mapping.edge {
                ScreenEdge::Left => ("zone_letter_left", "map_left"),
                ScreenEdge::Center => ("zone_letter_center", "map_slice"),
                ScreenEdge::Right => ("zone_letter_right", "map_right"),
            };
            let badge = match (mapping.edge, dark) {
                (ScreenEdge::Left, false) => oklch(0.42, 0.14, 262.0, 1.0),
                (ScreenEdge::Center, false) => oklch(0.51, 0.1, 175.0, 1.0),
                (ScreenEdge::Right, false) => oklch(0.625, 0.145, 58.0, 1.0),
                (ScreenEdge::Left, true) => oklch(0.64, 0.13, 252.0, 1.0),
                (ScreenEdge::Center, true) => oklch(0.74, 0.12, 175.0, 1.0),
                (ScreenEdge::Right, true) => oklch(0.87, 0.13, 80.0, 1.0),
            };
            let letter = t(letter_key);
            let what = t(model::describe(&mapping));
            let zone_of = fill(
                &t("zone_of"),
                &[("letter", &letter), ("zone", &t(zone_key))],
            );
            let label = if one { t("mirrors") } else { name.clone() };
            let change = t("change_on_plan");
            ZoneRow {
                id: ss(id),
                description: ss(format!("{label}: {zone_of}, {what}")),
                label: ss(label),
                letter: ss(letter),
                badge,
                ink,
                what: ss(what),
                hdr: i == 0 && l.status.hdr,
                plan_label: ss(fill(&t("labelled"), &[("name", &name), ("what", &change)])),
            }
        });
    keep(g.get_zones(), |m| g.set_zones(m), zones);

    // The three feels, explained together behind the row's info mark.
    let feels: Vec<Choice> = model::FEELS
        .iter()
        .map(|(f, _)| Choice {
            label: ss(t(&format!("feel_{f}"))),
            tip: ss(t(&format!("feel_{f}_tip"))),
        })
        .collect();
    let current = g.get_feels();
    let feels = reuse(&current, feels);
    if current != feels {
        g.set_feels(feels);
    }
    g.set_feel_tip(ss(model::FEELS
        .iter()
        .map(|(f, _)| {
            format!(
                "{}: {}",
                t(&format!("feel_{f}")),
                t(&format!("feel_{f}_tip"))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")));
    let feel = current_feel(&l.config);
    g.set_feel(
        feel.and_then(|f| model::FEELS.iter().position(|(n, _)| *n == f))
            .map_or(-1, |i| i as i32),
    );

    let default = f32::from(lueur_core::show::ShowSettings::default().speed);
    g.set_speed(f32::from(l.config.show.speed));
    g.set_speed_default(default);
    g.set_speed_text(ss(times(lang, show_pace(f32::from(l.config.show.speed)))));
    g.set_speed_reset(ss(fill(
        &t("reset_to"),
        &[("v", &times(lang, show_pace(default)))],
    )));

    g.set_flashes(l.config.show.flashes);
    // What the music listens to, by the name the source had when picked.
    let sound = match &l.config.audio {
        AudioSource::Default => t(if cfg!(windows) {
            "audio_default_output"
        } else {
            "audio_default_input"
        }),
        AudioSource::Device { name, .. } | AudioSource::App { name, .. } => name.clone(),
    };
    let waiting = l.status.music_waiting;
    g.set_listen_waiting(waiting);
    g.set_listens_to(ss(if waiting {
        fill(&t("sound_waiting"), &[("name", &sound)])
    } else {
        sound
    }));
}

/// Which pool of brightness a mode draws from: the screen has its own, the
/// shows share another.
fn pool(mode: Option<&str>) -> Option<&'static str> {
    match mode {
        None => None,
        Some("ambilight") => Some("screen"),
        Some(_) => Some("show"),
    }
}

/// The light the lights picked give: one brightness for all of them, the
/// white or colour of those set by hand, and for a single light, its
/// identify flash and its place on the plan.
fn tune(l: &Lueur, g: &LightingState) {
    let room = Room::of(l);
    let lang = l.lang();
    let ids = room.mood_targets();
    g.set_tune_shown(!ids.is_empty());
    if ids.is_empty() {
        return;
    }
    g.set_single(super::single(l).is_some());

    // The lights on say how bright the selection is; with all of them off,
    // the brightness they come back at.
    let lit: Vec<&String> = ids.iter().filter(|id| room.on(id)).collect();
    let from: Vec<&String> = if lit.is_empty() {
        ids.iter().collect()
    } else {
        lit
    };
    let brightnesses: Vec<u8> = from.iter().map(|id| room.bright_of(id)).collect();
    let brightness = (brightnesses.iter().map(|&b| f32::from(b)).sum::<f32>()
        / brightnesses.len().max(1) as f32)
        .round();
    // Not all as bright: the slider shows their average, and says so.
    let mixed = brightnesses.iter().collect::<BTreeSet<_>>().len() > 1;
    // An effect has one brightness for every light playing it: moving it
    // moves the lights outside the selection playing it too.
    let pools: BTreeSet<&str> = ids.iter().filter_map(|id| pool(room.mode_of(id))).collect();
    let shared: Vec<String> = if pools.is_empty() {
        Vec::new()
    } else {
        l.config
            .bindings
            .iter()
            .map(|b| &b.light_id)
            .filter(|id| {
                !ids.contains(id) && pool(room.mode_of(id)).is_some_and(|p| pools.contains(p))
            })
            .map(|id| room.name_of(id))
            .collect()
    };
    let by_hand: Vec<String> = ids
        .iter()
        .filter(|id| room.mode_of(id).is_none())
        .cloned()
        .collect();
    // An effect goes no dimmer than 5 %: the thumb stops where its light
    // does.
    let min = if by_hand.is_empty() { 5.0 } else { 1.0 };
    let value = brightness.max(min);
    let p = pct(lang, value / 100.0);
    g.set_bright(value);
    g.set_bright_min(min);
    g.set_bright_text(ss(if mixed {
        fill(&t("bright_mixed"), &[("value", &p)])
    } else {
        p
    }));
    g.set_bright_shared(ss(if shared.is_empty() {
        String::new()
    } else {
        fill(&t("bright_shared"), &[("names", &shared.join(", "))])
    }));

    // --- White and colour, for the lights set by hand ---
    let tint = by_hand.first().map(|id| room.state(id).tint);
    g.set_tint_shown(tint.is_some());
    g.set_kelvin_min(f32::from(MIN_KELVIN));
    g.set_kelvin_max(f32::from(MAX_KELVIN));
    g.set_warm_low(rgb8(kelvin_rgb(MIN_KELVIN), 1.0));
    g.set_warm_mid(rgb8(kelvin_rgb(4000), 1.0));
    g.set_warm_high(rgb8(kelvin_rgb(MAX_KELVIN), 1.0));
    match tint {
        Some(Tint::White { kelvin }) => {
            g.set_colour(false);
            g.set_kelvin(f32::from(kelvin));
            g.set_kelvin_text(ss(format!("{kelvin} K")));
        }
        Some(Tint::Color { r, g: gr, b }) => {
            let current = [r, gr, b];
            let in_palette = PALETTE.iter().any(|(c, _)| *c == current);
            let open = l.pages.lighting.custom_colour;
            g.set_colour(true);
            g.set_custom_picked(!in_palette || open);
            g.set_custom_open(open);
            g.set_hue_shown(open || !in_palette);
            let hue = rgb_hue(current).round();
            g.set_hue(hue);
            g.set_hue_text(ss(format!("{hue}°")));
            let palette = PALETTE.iter().map(|(c, key)| Swatch {
                name: ss(t(key)),
                colour: rgb8(*c, 1.0),
                picked: *c == current,
            });
            keep(g.get_palette(), |m| g.set_palette(m), palette);
        }
        None => {}
    }
}

pub fn project(l: &Lueur, ui: &AppWindow) {
    let g = ui.global::<LightingState>();
    let theme = ui.global::<crate::Theme>();
    let (dark, active) = (theme.get_dark(), theme.get_active());
    let room = Room::of(l);
    let bound = l.config.bindings.len();
    g.set_has_bulbs(bound > 0);
    g.set_busy(l.busy);
    let any_on = l.config.bindings.iter().any(|b| room.on(&b.light_id));
    g.set_any_on(any_on);
    g.set_all_off_tip(ss(if any_on {
        t("all_off_tip")
    } else {
        String::new()
    }));
    // Every bulb removed while an effect plays: the one way to stop it
    // left on the page, beside Ctrl+Enter and the tray.
    g.set_show_stop(bound == 0 && l.status.running);
    sleep(l, &g);
    setup(l, &g);
    lights(l, &g, active);
    if bound > 0 {
        scenes(l, &g);
        selection(l, &g, active);
        modes(l, &g);
        mode_settings(l, &g, dark);
        tune(l, &g);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_its_photograph() {
        for id in ANIMATED
            .iter()
            .copied()
            .chain([NATURAL])
            .chain(MOODS.iter().map(|m| m.id))
        {
            let name = photo_name(picture_of(id));
            assert!(
                crate::images::PHOTOS.iter().any(|(n, _)| *n == name),
                "{id}"
            );
        }
    }

    #[test]
    fn the_hue_track_is_the_wheel() {
        // The stops `hue-track` writes in lighting.slint.
        let stops = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0].map(super::model::hue_rgb);
        assert_eq!(stops[0], [255, 0, 0]);
        assert_eq!(stops[1], [255, 255, 0]);
        assert_eq!(stops[4], [0, 0, 255]);
        assert_eq!(stops[6], [255, 0, 0]);
    }
}
