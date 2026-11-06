//! The plan, worked out in the stage's pixels for `room/plan.slint`.
//!
//! Positions go through the frame that fits the room to the stage, while
//! strokes, handles and text keep a constant size in pixels. Shapes are
//! handed over as SVG path commands; the pools, corners and pins as lists.

use super::colors::PlanColors;
use super::plan::{self, Frame, Other, Target};
use super::selected_light;
use crate::i18n::{fill, t};
use crate::models::keep;
use crate::rt;
use crate::state::Lueur;
use crate::{Corner, Dash, Pin, Pool, RoomState};
use lueur_core::geometry::{LightMapping, ScreenEdge, Vec2, map_light};
use slint::{Color, ComponentHandle};
use std::fmt::Write as _;

/// A polygon or polyline as path commands; empty for fewer than two points.
pub fn poly(points: &[(f32, f32)], closed: bool) -> String {
    let mut s = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        let _ = write!(s, "{}{x:.1} {y:.1} ", if i == 0 { "M " } else { "L " });
    }
    if points.len() < 2 {
        return String::new();
    }
    if closed {
        s.push('Z');
    }
    s
}

/// A dashed line from `a` to `b`, `on` pixels drawn then `off` skipped.
pub fn dashes(a: (f32, f32), b: (f32, f32), on: f32, off: f32) -> String {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    if len < 0.5 {
        return String::new();
    }
    let (ux, uy) = (dx / len, dy / len);
    let mut s = String::new();
    let mut d = 0.0;
    while d < len {
        let e = (d + on).min(len);
        let _ = write!(
            s,
            "M {:.1} {:.1} L {:.1} {:.1} ",
            a.0 + ux * d,
            a.1 + uy * d,
            a.0 + ux * e,
            a.1 + uy * e
        );
        d += on + off;
    }
    s
}

/// The dotted grid, every 25 plan units, anchored to the plan: each dot a
/// small filled octagon, 2.2px across.
fn grid(f: &Frame, w: f32, h: f32) -> String {
    let step = 25.0;
    let vb = f.vb;
    let x0 = ((vb.x - 12.5) / step).floor() * step + 12.5;
    let y0 = ((vb.y - 12.5) / step).floor() * step + 12.5;
    let mut s = String::new();
    let mut y = y0;
    while y <= vb.y + vb.h {
        let mut x = x0;
        while x <= vb.x + vb.w {
            let (px, py) = f.to_px(Vec2::new(x, y));
            if (-2.0..=w + 2.0).contains(&px) && (-2.0..=h + 2.0).contains(&py) {
                let (a, b) = (1.1, 0.46);
                let _ = write!(
                    s,
                    "M {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} L {:.2} {:.2} Z ",
                    px - b,
                    py - a,
                    px + b,
                    py - a,
                    px + a,
                    py - b,
                    px + a,
                    py + b,
                    px + b,
                    py + a,
                    px - b,
                    py + a,
                    px - a,
                    py + b,
                    px - a,
                    py - b
                );
            }
            x += step;
        }
        y += step;
    }
    s
}

fn with_alpha(c: Color, a: f32) -> Color {
    c.with_alpha(a.clamp(0.0, 1.0))
}

fn edge_index(e: ScreenEdge) -> usize {
    match e {
        ScreenEdge::Left => 0,
        ScreenEdge::Center => 1,
        ScreenEdge::Right => 2,
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
    g.set_plan_bg(c.bg);
    g.set_grid(c.grid);
    g.set_floor(c.floor);
    g.set_wall(c.wall);
    g.set_pin_shadow(c.pin_shadow);
    g.set_bezel(c.bezel);
    g.set_zone_ink(c.zone_ink);

    let (w, h) = (g.get_stage_width(), g.get_stage_height());
    let local = &l.pages.room;
    let room = &l.config.room;
    let vb = local.held.unwrap_or_else(|| plan::fit(room, w, h));
    let f = Frame::new(vb, (0.0, 0.0), w, h);
    let k = f.k;
    let to = |p: Vec2| f.to_px(p);
    let scr = &room.screen;
    let maps: Vec<LightMapping> = room.lights.iter().map(|x| map_light(scr, x)).collect();
    let selected = selected_light(l);

    g.set_grid_commands(
        if w > 0.0 {
            grid(&f, w, h)
        } else {
            String::new()
        }
        .into(),
    );
    let walls: Vec<(f32, f32)> = room.walls.iter().map(|p| to(*p)).collect();
    let shadow: Vec<(f32, f32)> = walls.iter().map(|(x, y)| (*x, y + 3.0)).collect();
    g.set_shadow_commands(poly(&shadow, true).into());
    g.set_floor_commands(poly(&walls, true).into());
    g.set_walls_commands(poly(&walls, true).into());

    let pools = room
        .lights
        .iter()
        .zip(&maps)
        .filter(|(x, _)| x.enabled)
        .map(|(x, m)| {
            let (px, py) = to(x.pos);
            Pool {
                x: px,
                y: py,
                r: (70.0 + 70.0 * m.spread) / k,
                color: c.zone(m.edge),
                opacity: 0.5 + 0.5 * m.gain,
            }
        });
    keep(g.get_pools(), |m| g.set_pools(m), pools);

    let (left, right) = scr.endpoints();
    let seat = plan::seat(scr);
    g.set_cone_commands(poly(&[to(seat), to(left), to(right)], true).into());

    // Which part of the screen each light looks at.
    let lines = room.lights.iter().zip(&maps).map(|(x, m)| {
        let a = plan::anchor_on_screen(scr, x.pos);
        let opacity = if x.enabled {
            0.25 + 0.45 * m.gain
        } else {
            0.12
        };
        Dash {
            commands: dashes(to(x.pos), to(a), 2.0, 5.0).into(),
            color: with_alpha(c.zone(m.edge), opacity),
        }
    });
    keep(g.get_dashes(), |m| g.set_dashes(m), lines);

    g.set_seat_line_commands(dashes(to(scr.center), to(seat), 4.0, 5.0).into());
    let (sx, sy) = to(seat);
    g.set_seat_x(sx);
    g.set_seat_y(sy);
    g.set_seat_hovered(local.hover == Some(Target::Seat));

    let (lp, rp) = (to(left), to(right));
    g.set_screen_commands(poly(&[lp, rp], false).into());
    g.set_screen_left_x(lp.0);
    g.set_screen_left_y(lp.1);
    g.set_screen_right_x(rp.0);
    g.set_screen_right_y(rp.1);
    g.set_screen_selected(local.other == Some(Other::Screen));
    let facing = scr.facing();
    let sc = to(scr.center);
    g.set_screen_label_x(sc.0 + facing.x * 20.0);
    g.set_screen_label_y(sc.1 + facing.y * 20.0);

    let corners = room.walls.iter().enumerate().map(|(i, p)| {
        let (x, y) = to(*p);
        Corner {
            x,
            y,
            selected: local.other == Some(Other::Corner(i)),
            hovered: local.hover == Some(Target::Corner(i)),
        }
    });
    keep(g.get_corners(), |m| g.set_corners(m), corners);

    let letters = plan::EDGES.map(|e| t(plan::letter_key(e)));
    let pins = room.lights.iter().zip(&maps).map(|(x, m)| {
        let (px, py) = to(x.pos);
        let zone = fill(
            &t("zone_of"),
            &[
                ("letter", &letters[edge_index(m.edge)]),
                ("zone", &t(plan::kind_key(m.edge))),
            ],
        );
        let label = if x.enabled {
            format!("{}, {zone}", x.name)
        } else {
            format!("{}, {}", x.name, t("left_out"))
        };
        let (pin, ink, name_ink) = if x.enabled {
            (c.zone(m.edge), c.zone_ink, theme.text)
        } else {
            (theme.line_strong, theme.text, theme.text_3)
        };
        Pin {
            id: x.id.as_str().into(),
            name: x.name.as_str().into(),
            label: label.into(),
            letter: letters[edge_index(m.edge)].as_str().into(),
            x: px,
            y: py,
            color: pin,
            ink,
            name_color: name_ink,
            selected: selected.as_deref() == Some(x.id.as_str()),
            hovered: local.hover == Some(Target::Light(x.id.clone())),
            name_dy: if plan::label_below(scr, x.pos, k) {
                20.0
            } else {
                -20.0
            },
        }
    });
    keep(g.get_pins(), |m| g.set_pins(m), pins);

    g.set_cursor(if local.drag.is_some() {
        2
    } else {
        match local.hover {
            Some(Target::Width) => 3,
            Some(Target::Wall) => 4,
            Some(_) => 1,
            None => 0,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_read_as_path_commands() {
        assert_eq!(poly(&[(0.0, 0.0)], true), "");
        assert_eq!(
            poly(&[(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)], true),
            "M 0.0 0.0 L 10.0 0.0 L 10.0 5.0 Z"
        );
    }

    #[test]
    fn a_dashed_line_is_cut_in_pieces_along_it() {
        let d = dashes((0.0, 0.0), (20.0, 0.0), 2.0, 5.0);
        // 0-2, 7-9, 14-16: three pieces, none past the end.
        assert_eq!(d.matches('M').count(), 3);
        assert!(dashes((1.0, 1.0), (1.0, 1.0), 2.0, 5.0).is_empty());
    }
}
