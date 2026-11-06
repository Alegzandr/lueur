//! The column beside the plan, worked out for `room/inspector.slint`: the
//! list of what the room holds, or the inspector of what is selected.

use super::colors::PlanColors;
use super::plan::{self, Other};
use super::{corner_name, selected_key, selected_light};
use crate::i18n::{fill, pct, t};
use crate::models::keep;
use crate::pages::bulbs::brands::bulb_label;
use crate::rt;
use crate::state::Lueur;
use crate::{ListRow, Paint, RoomState};
use lueur_core::color::Rgb;
use lueur_core::geometry::{Light, ScreenEdge, map_light};
use slint::{Color, ComponentHandle};

/// The wall paints offered.
pub const WALLS: [(&str, Rgb); 6] = [
    ("white", Rgb::new(255, 255, 255)),
    ("cream", Rgb::new(250, 240, 215)),
    ("beige", Rgb::new(235, 215, 180)),
    ("blue", Rgb::new(190, 210, 235)),
    ("green", Rgb::new(195, 220, 195)),
    ("pink", Rgb::new(240, 205, 205)),
];

fn rgb(c: Rgb) -> Color {
    Color::from_rgb_u8(c.r, c.g, c.b)
}

/// "zone L, left side", for screen readers.
fn zone_words(edge: ScreenEdge) -> String {
    fill(
        &t("zone_of"),
        &[
            ("letter", &t(plan::letter_key(edge))),
            ("zone", &t(plan::kind_key(edge))),
        ],
    )
}

/// The zone's colour and the ink of its letter, grey for a light left out.
fn zone_marks(
    edge: ScreenEdge,
    off: bool,
    c: &PlanColors,
    theme: &crate::theme::Theme,
) -> (Color, Color) {
    if off {
        (theme.line_strong, theme.text)
    } else {
        (c.zone(edge), c.zone_ink)
    }
}

pub fn project(l: &Lueur, g: &RoomState<'_>) {
    let dark = rt::ui().is_some_and(|ui| ui.global::<crate::Theme>().get_dark());
    let c = PlanColors::of(dark);
    let theme = if dark {
        crate::theme::Theme::dark()
    } else {
        crate::theme::Theme::light()
    };
    let local = &l.pages.room;
    let room = &l.config.room;
    let history = local.history.borrow();
    g.set_can_undo(history.can_undo());
    g.set_can_redo(history.can_redo());
    drop(history);
    g.set_coach(!l.config.interface.room_arranged && !room.lights.is_empty());
    g.set_announcement(local.announcement.as_str().into());
    g.set_focus_inspector(local.focus_inspector);
    g.set_focus_row(local.focus_row.clone().unwrap_or_default().into());
    g.set_focus_row_seq(local.focus_row_seq);
    g.set_corners_open(local.corners_open);
    g.set_exact_open(local.exact_open);

    // The list.
    let lights = room.lights.iter().map(|x| {
        let m = map_light(&room.screen, x);
        let bound = l.config.binding_for(&x.id).is_some();
        let meta = if x.enabled {
            t(plan::kind_key(m.edge))
        } else {
            t("left_out")
        };
        let label = format!(
            "{}, {}{}",
            x.name,
            if x.enabled {
                zone_words(m.edge)
            } else {
                meta.clone()
            },
            if bound {
                String::new()
            } else {
                format!(", {}", t("no_bulb"))
            }
        );
        let (color, ink) = zone_marks(m.edge, !x.enabled, &c, &theme);
        ListRow {
            key: format!("light:{}", x.id).into(),
            name: x.name.as_str().into(),
            meta: meta.into(),
            label: label.into(),
            letter: t(plan::letter_key(m.edge)).into(),
            color,
            ink,
            off: !x.enabled,
            no_bulb: !bound,
        }
    });
    keep(g.get_lights(), |m| g.set_lights(m), lights);
    g.set_screen_meta(
        fill(
            &t("screen_meta"),
            &[
                ("w", &room.screen.width.round().to_string()),
                ("deg", &room.screen.facing_deg.to_string()),
            ],
        )
        .into(),
    );
    // A paint of the user's own shows as one more swatch, picked.
    let picked = WALLS.iter().position(|(_, w)| *w == room.wall);
    let mut paints: Vec<Paint> = WALLS
        .iter()
        .enumerate()
        .map(|(i, (name, w))| Paint {
            color: rgb(*w),
            label: t(&format!("wall_{name}")).into(),
            picked: picked == Some(i),
        })
        .collect();
    if picked.is_none() {
        paints.push(Paint {
            color: rgb(room.wall),
            label: t("wall_custom").into(),
            picked: true,
        });
    }
    keep(g.get_paints(), |m| g.set_paints(m), paints);
    let corner_rows: Vec<ListRow> = if local.corners_open {
        room.walls
            .iter()
            .enumerate()
            .map(|(i, w)| ListRow {
                key: format!("corner:{i}").into(),
                name: corner_name(&room.walls, i).into(),
                meta: format!("{}, {}", w.x, w.y).into(),
                ..Default::default()
            })
            .collect()
    } else {
        Vec::new()
    };
    keep(g.get_corner_rows(), |m| g.set_corner_rows(m), corner_rows);

    // The inspector.
    let light = selected_light(l).and_then(|id| room.lights.iter().find(|x| x.id == id).cloned());
    let mode = if light.is_some() {
        1
    } else {
        match local.other {
            Some(Other::Screen) => 2,
            Some(Other::Corner(_)) => 3,
            None => 0,
        }
    };
    g.set_mode(mode);
    fields(l, g);
    if let Some(light) = &light {
        light_inspector(l, light, &c, &theme, g);
    }
    if mode == 2 {
        screen_inspector(l, g);
    }
    if let Some(Other::Corner(i)) = local.other {
        g.set_heading(corner_name(&room.walls, i).into());
    }
    if let Some(p) = super::selected_pos(l) {
        g.set_exact_summary(format!("{}, {}", p.x, p.y).into());
    }
    g.set_pos_tip(
        fill(
            &t("pos_tip"),
            &[
                ("min", &plan::MARGIN.to_string()),
                ("maxX", &plan::X_MAX.to_string()),
                ("maxY", &plan::Y_MAX.to_string()),
                ("w", &room.screen.width.round().to_string()),
            ],
        )
        .into(),
    );
}

/// Fill the inspector's fields for what is picked, or bring them up to
/// date with a move made elsewhere (the plan, the arrows). A field the
/// keyboard is in keeps what the user typed.
fn fields(l: &Lueur, g: &RoomState<'_>) {
    let local = &l.pages.room;
    let key = selected_key(l);
    let pos = super::selected_pos(l);
    let fresh = *local.fields_for.borrow() != key;
    if fresh {
        *local.fields_for.borrow_mut() = key.clone();
    }
    let (Some(_), Some(pos)) = (key, pos) else {
        return;
    };
    let put =
        |focused: bool, now: slint::SharedString, v: f32, set: &dyn Fn(slint::SharedString)| {
            let text = v.to_string();
            if (fresh || !focused) && now.as_str() != text {
                set(text.into());
            }
        };
    put(g.get_x_focused() && !fresh, g.get_x_text(), pos.x, &|s| {
        g.set_x_text(s)
    });
    put(g.get_y_focused() && !fresh, g.get_y_text(), pos.y, &|s| {
        g.set_y_text(s)
    });
    if let Some(light) =
        selected_light(l).and_then(|id| l.config.room.lights.iter().find(|x| x.id == id))
    {
        if fresh {
            *local.name_before.borrow_mut() = light.name.clone();
        }
        if (fresh || !g.get_name_focused()) && g.get_name_text().as_str() != light.name {
            g.set_name_text(light.name.as_str().into());
        }
    }
}

fn light_inspector(
    l: &Lueur,
    light: &Light,
    c: &PlanColors,
    theme: &crate::theme::Theme,
    g: &RoomState<'_>,
) {
    let scr = &l.config.room.screen;
    let m = map_light(scr, light);
    let lang = l.lang();
    // The screen as it sits on a desk, with the part this light mirrors,
    // after the inspector's small SVG (168 by 106 units, drawn at 1.25).
    const S: f32 = 1.25;
    let (gw, gh) = (160.0 * S, 90.0 * S);
    g.set_zone_x(4.0 * S + m.zone.x * gw + 3.0 * S);
    g.set_zone_y(4.0 * S + m.zone.y * gh + 3.0 * S);
    g.set_zone_w((m.zone.w * gw - 6.0 * S).max(4.0 * S));
    g.set_zone_h((m.zone.h * gh - 6.0 * S).max(4.0 * S));
    g.set_zone_fill(0.22 + 0.28 * (1.0 - m.spread));
    let (color, _) = zone_marks(m.edge, !light.enabled, c, theme);
    g.set_zone_color(color);
    g.set_zone_letter(t(plan::letter_key(m.edge)).into());
    g.set_light_off(!light.enabled);
    let says = if light.enabled {
        t(plan::describe(&m))
    } else {
        t("left_out")
    };
    g.set_says_label(format!("{}, {says}", zone_words(m.edge)).into());
    g.set_says(says.into());
    g.set_height(light.height);
    g.set_height_text(t(plan::height_key(light.height)).into());
    g.set_height_reset(fill(&t("reset_to"), &[("v", &t("h_level"))]).into());
    g.set_follows(light.enabled);
    g.set_gain_text(pct(lang, m.gain).into());
    g.set_response_text(pct(lang, m.responsiveness).into());
    g.set_bulb(
        l.config
            .binding_for(&light.id)
            .map(|b| bulb_label(b, Some(&light.name)))
            .unwrap_or_default()
            .into(),
    );
}

fn screen_inspector(l: &Lueur, g: &RoomState<'_>) {
    let scr = l.config.room.screen;
    let first = lueur_core::geometry::Screen::default();
    g.set_facing(scr.facing_deg);
    g.set_facing_text(format!("{}°", scr.facing_deg).into());
    g.set_facing_reset(fill(&t("reset_to"), &[("v", &format!("{}°", first.facing_deg))]).into());
    g.set_screen_width(scr.width);
    g.set_width_text(scr.width.round().to_string().into());
    g.set_width_reset(fill(&t("reset_to"), &[("v", &first.width.to_string())]).into());
    g.set_width_min(plan::SCREEN_MIN);
    g.set_width_max(plan::SCREEN_MAX);
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::config::DeviceBinding;

    #[test]
    fn a_bulb_is_named_by_its_name_and_brand_never_its_address() {
        let mut b = DeviceBinding {
            light_id: "l".into(),
            device_id: "d".into(),
            ip: "192.168.1.2".into(),
            port: 1,
            model: "color4".into(),
            name: String::new(),
            kind: Default::default(),
        };
        assert_eq!(bulb_label(&b, Some("Desk")), "Yeelight color4");
        b.name = "Desk".into();
        assert_eq!(bulb_label(&b, Some("Desk")), "Yeelight color4");
        b.name = "Lamp".into();
        assert_eq!(bulb_label(&b, Some("Desk")), "Lamp · Yeelight color4");
        assert!(!bulb_label(&b, Some("Desk")).contains("192"));
    }

    #[test]
    fn the_paints_offered_include_white() {
        assert_eq!(WALLS[0].1, Rgb::WHITE);
    }
}
