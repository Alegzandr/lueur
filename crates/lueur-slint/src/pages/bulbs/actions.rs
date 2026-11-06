//! What the Bulbs page changes in the room: adding lights for bulbs, giving a
//! light a bulb or taking it back, renaming and removing lights, and the
//! identify flash.
//!
//! These are free functions over [`Lueur`] rather than methods, so they
//! cannot clash with what the other pages add to it.

use crate::i18n::{self, Lang, fill};
use crate::rt;
use crate::state::{Lueur, View};
use lueur_core::color::Rgb;
use lueur_core::config::{AppConfig, DeviceBinding};
use lueur_core::device::{DiscoveredDevice, Endpoint};
use lueur_core::geometry::{Light, Room, Vec2};
use lueur_shell::service;
use std::rc::Rc;

/// The longest name a light can have, as the fields that rename it allow.
pub const MAX_LIGHT_NAME: usize = 60;

/// Warm white: an identify flash should read as "this one", never as a
/// colour the effect could have picked.
const IDENTIFY_RGB: Rgb = Rgb::new(255, 190, 120);

/// "Light n" for the smallest n no light of the room is called yet, so a
/// light added after a removal never takes a name already in use.
pub fn fresh_name(lang: Lang, config: &AppConfig) -> String {
    let template = i18n::text(lang, "light_n");
    let name = |n: usize| fill(&template, &[("n", &n.to_string())]);
    let lights = &config.room.lights;
    let n = lights.len() + 1;
    (1..=n)
        .map(name)
        .find(|candidate| lights.iter().all(|l| l.name != *candidate))
        .unwrap_or_else(|| name(n))
}

fn dist(a: Vec2, b: Vec2) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// Where new lights go: in a row just behind the screen, the usual
/// ambilight setup, spread evenly across its width. A spot already taken is
/// skipped, so lights never land on top of each other; the user drags them
/// to their real places afterwards. The same placement as the Room page's.
pub fn fresh_positions(room: &Room, count: usize) -> Vec<Vec2> {
    let s = &room.screen;
    let f = s.facing();
    let r = s.right_axis();
    let at = |u: f32, depth: f32| {
        Vec2::new(
            (s.center.x + r.x * u * s.width + f.x * depth)
                .clamp(20.0, 980.0)
                .round(),
            (s.center.y + r.y * u * s.width + f.y * depth)
                .clamp(20.0, 730.0)
                .round(),
        )
    };
    let mut taken: Vec<Vec2> = room.lights.iter().map(|l| l.pos).collect();
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // Even spread across 120% of the screen width, one row behind it.
        let u = if count == 1 {
            0.0
        } else {
            -0.6 + 1.2 * i as f32 / (count - 1) as f32
        };
        let mut p = at(u, -45.0);
        // Taken: walk outwards along further rows until a free spot turns up.
        let free = |p: Vec2, taken: &[Vec2]| taken.iter().all(|q| dist(p, *q) > 45.0);
        let mut k = 0;
        while !free(p, &taken) && k < 40 {
            let u = ((k % 7) as f32 - 3.0) * 0.3;
            let back = -45.0 - (k / 7) as f32 * 55.0 + if k >= 7 { 140.0 } else { 0.0 };
            p = at(u, back);
            k += 1;
        }
        taken.push(p);
        out.push(p);
    }
    out
}

/// A fresh light id: a random UUID, as the Tauri versions made them.
pub fn new_light_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn new_light(name: String, pos: Vec2) -> Light {
    Light {
        id: new_light_id(),
        name,
        pos,
        height: 0.5,
        zone_override: None,
        enabled: true,
    }
}

fn binding_of(light_id: &str, d: &DiscoveredDevice, name: &str) -> DeviceBinding {
    DeviceBinding {
        light_id: light_id.to_string(),
        device_id: d.id.clone(),
        ip: d.ip.clone(),
        port: d.port,
        model: d.model.clone(),
        name: name.to_string(),
        kind: d.kind.clone(),
    }
}

/// Add found bulbs to the room as new lights, each with the name the page
/// shows for it. `announce` is false when the page already says so.
pub fn add_devices(l: &mut Lueur, devices: Vec<(DiscoveredDevice, String)>, announce: bool) {
    if devices.is_empty() {
        return;
    }
    let spots = fresh_positions(&l.config.room, devices.len());
    let mut names = Vec::with_capacity(devices.len());
    l.edit(|c| {
        for ((d, name), pos) in devices.iter().zip(spots) {
            let light = new_light(name.clone(), pos);
            c.bindings.push(binding_of(&light.id, d, name));
            names.push(light.name.clone());
            c.room.lights.push(light);
        }
    });
    if !announce {
        return;
    }
    let text = if let [name] = names.as_slice() {
        fill(&l.t("added"), &[("name", name)])
    } else {
        fill(&l.t("added_many"), &[("n", &names.len().to_string())])
    };
    l.inform(text);
}

/// Point an existing light at a found bulb, replacing any it had, and
/// taking the bulb from any light it was on.
pub fn assign_device(l: &mut Lueur, light_id: &str, d: &DiscoveredDevice, name: &str) {
    l.edit(|c| {
        c.bindings
            .retain(|b| b.light_id != light_id && b.device_id != d.id);
        c.bindings.push(binding_of(light_id, d, name));
    });
}

/// Take its bulb from a light, which stays in the room.
pub fn unassign(l: &mut Lueur, light_id: &str) {
    l.edit(|c| c.bindings.retain(|b| b.light_id != light_id));
}

pub fn rename_light(l: &mut Lueur, id: &str, name: &str) {
    let name: String = name.chars().take(MAX_LIGHT_NAME).collect();
    l.edit(|c| {
        if let Some(light) = c.room.lights.iter_mut().find(|x| x.id == id) {
            light.name = name;
        }
    });
}

/// Once its field is left, a light without a name gets back the one it had
/// (`before`), or a fresh one: a nameless light would leave its card, its
/// row and its pin on the plan blank.
pub fn settle_name(l: &mut Lueur, id: &str, before: &str) {
    let Some(light) = l.config.room.lights.iter().find(|x| x.id == id) else {
        return;
    };
    let name = if !light.name.trim().is_empty() {
        light.name.trim().to_string()
    } else if !before.trim().is_empty() {
        before.trim().to_string()
    } else {
        fresh_name(l.lang(), &l.config)
    };
    if name != light.name {
        rename_light(l, id, &name);
    }
}

/// Remove a light and its bulb, with an undo in the toast.
pub fn remove_light(l: &mut Lueur, id: &str) {
    let Some(index) = l.config.room.lights.iter().position(|x| x.id == id) else {
        return;
    };
    let light = l.config.room.lights[index].clone();
    let binding = l.config.binding_for(id).cloned();
    l.edit(|c| {
        c.room.lights.remove(index);
        c.bindings.retain(|b| b.light_id != id);
    });
    if l.selected_light.as_deref() == Some(id) {
        l.selected_light = None;
    }
    let text = fill(&l.t("removed"), &[("name", &light.name)]);
    let undo = Rc::new(move || {
        let (light, binding) = (light.clone(), binding.clone());
        rt::with(|l| {
            l.edit(|c| {
                // Already back, by the plan's own undo: once is enough.
                if c.room.lights.iter().any(|x| x.id == light.id) {
                    return;
                }
                let at = index.min(c.room.lights.len());
                c.room.lights.insert(at, light);
                // The bulb went to another light since: it stays there.
                if let Some(b) = binding
                    && !c.bindings.iter().any(|x| x.device_id == b.device_id)
                {
                    c.bindings.push(b);
                }
            })
        });
    });
    let label = l.t("undo");
    l.inform_with(text, label, undo);
}

/// Flash a bulb warm white, to spot it.
pub fn identify(l: &mut Lueur, endpoint: Endpoint) {
    l.run(
        move |s| service::test_device(s, endpoint, IDENTIFY_RGB),
        |_, ()| {},
    );
}

/// Open the room plan with light `id` picked, to see or move it.
pub fn show_on_plan(l: &mut Lueur, id: &str) {
    l.selected_light = Some(id.to_string());
    l.show(View::Room);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_fresh_and_shaped_like_uuids() {
        let (a, b) = (new_light_id(), new_light_id());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a.as_bytes()[14], b'4');
    }

    #[test]
    fn a_fresh_name_skips_those_in_use() {
        let mut c = AppConfig::default();
        c.room.lights.clear();
        assert_eq!(fresh_name(Lang::En, &c), "Light 1");
        c.room
            .lights
            .push(new_light("Light 1".into(), Vec2::new(0.0, 0.0)));
        c.room
            .lights
            .push(new_light("Light 3".into(), Vec2::new(0.0, 0.0)));
        assert_eq!(fresh_name(Lang::En, &c), "Light 2");
        // Names are compared in the language they are made in.
        assert_eq!(fresh_name(Lang::Fr, &c), "Lumière 1");
    }

    #[test]
    fn new_lights_never_land_on_each_other() {
        let mut c = AppConfig::default();
        let first = fresh_positions(&c.room, 3);
        for p in &first {
            c.room.lights.push(new_light("x".into(), *p));
        }
        let more = fresh_positions(&c.room, 3);
        for p in &more {
            assert!(
                c.room
                    .lights
                    .iter()
                    .all(|l| (p.x - l.pos.x).hypot(p.y - l.pos.y) > 45.0)
            );
        }
    }
}
