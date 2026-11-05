//! What the Bulbs page changes in the room: adding lights for bulbs, giving a
//! light a bulb or taking it back, renaming and removing lights, and the
//! identify flash.
//!
//! These are free functions over [`Lueur`] rather than methods, so they
//! cannot clash with what the other pages add to it.

use crate::i18n::{self, Lang, fill};
use crate::state::{Lueur, View, model};
use gpui::{App, Context, Window};
use lueur_core::color::Rgb;
use lueur_core::config::{AppConfig, DeviceBinding};
use lueur_core::device::{DiscoveredDevice, Endpoint};
use lueur_core::geometry::{Light, Vec2};
use lueur_shell::service;
use std::rc::Rc;

/// The longest name a light can have, as the fields that rename it allow.
pub const MAX_LIGHT_NAME: usize = 60;

/// Warm white: an identify flash should read as "this one", never as a
/// colour the effect could have picked.
const IDENTIFY_RGB: Rgb = Rgb::new(255, 190, 120);

/// "Light n" for the smallest n no light of the room is called yet.
pub fn fresh_name(lang: Lang, config: &AppConfig) -> String {
    let template = i18n::text(lang, "light_n");
    crate::views::room::plan::fresh_name(&config.room.lights, |n| {
        fill(&template, &[("n", &n.to_string())])
    })
}

/// Where new lights go: see the Room page's `fresh_positions`.
pub fn fresh_positions(config: &AppConfig, count: usize) -> Vec<Vec2> {
    crate::views::room::plan::fresh_positions(&config.room, count)
}

fn new_light(name: String, pos: Vec2) -> Light {
    Light {
        id: crate::views::room::plan::new_light_id(),
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
pub fn add_devices(
    l: &mut Lueur,
    devices: Vec<(DiscoveredDevice, String)>,
    announce: bool,
    cx: &mut Context<Lueur>,
) {
    if devices.is_empty() {
        return;
    }
    let spots = fresh_positions(&l.config, devices.len());
    let mut names = Vec::with_capacity(devices.len());
    l.edit(cx, |c| {
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
    l.inform(text, cx);
}

/// Point an existing light at a found bulb, replacing any it had, and
/// taking the bulb from any light it was on.
pub fn assign_device(
    l: &mut Lueur,
    light_id: &str,
    d: &DiscoveredDevice,
    name: &str,
    cx: &mut Context<Lueur>,
) {
    l.edit(cx, |c| {
        c.bindings
            .retain(|b| b.light_id != light_id && b.device_id != d.id);
        c.bindings.push(binding_of(light_id, d, name));
    });
}

/// Take its bulb from a light, which stays in the room.
pub fn unassign(l: &mut Lueur, light_id: &str, cx: &mut Context<Lueur>) {
    l.edit(cx, |c| c.bindings.retain(|b| b.light_id != light_id));
}

pub fn rename_light(l: &mut Lueur, id: &str, name: &str, cx: &mut Context<Lueur>) {
    let name: String = name.chars().take(MAX_LIGHT_NAME).collect();
    l.edit(cx, |c| {
        if let Some(light) = c.room.lights.iter_mut().find(|x| x.id == id) {
            light.name = name;
        }
    });
}

/// Once its field is left, a light without a name gets back the one it had
/// (`before`), or a fresh one: a nameless light would leave its card, its
/// row and its pin on the plan blank.
pub fn settle_name(l: &mut Lueur, id: &str, before: &str, cx: &mut Context<Lueur>) {
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
        rename_light(l, id, &name, cx);
    }
}

/// Remove a light and its bulb, with an undo in the toast.
pub fn remove_light(l: &mut Lueur, id: &str, cx: &mut Context<Lueur>) {
    let Some(index) = l.config.room.lights.iter().position(|x| x.id == id) else {
        return;
    };
    let light = l.config.room.lights[index].clone();
    let binding = l.config.binding_for(id).cloned();
    l.edit(cx, |c| {
        c.room.lights.remove(index);
        c.bindings.retain(|b| b.light_id != id);
    });
    if l.selected_light.as_deref() == Some(id) {
        l.selected_light = None;
    }
    let text = fill(&l.t("removed"), &[("name", &light.name)]);
    let undo = Rc::new(move |_: &mut Window, cx: &mut App| {
        let (light, binding) = (light.clone(), binding.clone());
        model(cx).update(cx, |l, cx| {
            l.edit(cx, |c| {
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
    l.inform_with(text, label, undo, cx);
}

/// Flash a bulb warm white, to spot it.
pub fn identify(l: &mut Lueur, endpoint: Endpoint, cx: &mut Context<Lueur>) {
    l.run(
        cx,
        move |s| service::test_device(s, endpoint, IDENTIFY_RGB),
        |_, (), _| {},
    );
}

/// Open the room plan with light `id` picked, to see or move it.
pub fn show_on_plan(l: &mut Lueur, id: &str, cx: &mut Context<Lueur>) {
    l.selected_light = Some(id.to_string());
    l.show(View::Room, cx);
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_fresh_and_shaped_like_uuids() {
        let (a, b) = (
            crate::views::room::plan::new_light_id(),
            crate::views::room::plan::new_light_id(),
        );
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
        let first = fresh_positions(&c, 3);
        for p in &first {
            c.room.lights.push(new_light("x".into(), *p));
        }
        let more = fresh_positions(&c, 3);
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
