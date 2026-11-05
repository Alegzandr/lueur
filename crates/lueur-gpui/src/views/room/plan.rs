//! What the Room page's pieces share: the plan's extent, where
//! things are drawn, what a press lands on, and the names of corners.
//!
//! The mapping of a light to the picture is the engine's own
//! ([`lueur_core::geometry::map_light`]), so the plan cannot drift from what
//! the lights do; only what the drawing needs on top of it lives here. All of
//! it is plain arithmetic in plan units, tested without a window.

use lueur_core::geometry::{Light, LightMapping, Room, Screen, ScreenEdge, Vec2};

// The extent of the plan in plan units. Everything placed is kept inside it.
pub const VIEW_W: f32 = 1000.0;
pub const VIEW_H: f32 = 750.0;
pub const MARGIN: f32 = 12.0;
/// How far in front of the screen the seat handle sits, in plan units.
pub const SEAT: f32 = 120.0;
pub const SCREEN_MIN: f32 = 120.0;
pub const SCREEN_MAX: f32 = 520.0;
// Typed positions are kept in the plan's bounds.
pub const X_MAX: f32 = VIEW_W - MARGIN;
pub const Y_MAX: f32 = VIEW_H - MARGIN;
/// The longest name a light can have, as the field that renames it allows.
pub const MAX_LIGHT_NAME: usize = 60;

/// What is selected besides a light: a light lives in the shared state
/// (other pages link to it); the screen and corners only here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Other {
    Screen,
    Corner(usize),
}

/// A point kept on the plan, in whole units.
pub fn clamp_p(p: Vec2) -> Vec2 {
    Vec2::new(
        p.x.clamp(MARGIN, VIEW_W - MARGIN).round(),
        p.y.clamp(MARGIN, VIEW_H - MARGIN).round(),
    )
}

fn dist(a: Vec2, b: Vec2) -> f32 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// The point of the segment from `a` to `b` closest to `p`.
pub fn closest_on_segment(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    let len2 = if len2 == 0.0 { 1.0 } else { len2 };
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    Vec2::new(a.x + ab.x * t, a.y + ab.y * t)
}

/// Where a light stands across the screen, in screen half-widths: -1 is
/// the left end as the viewer sees it, +1 the right. The engine works it
/// out the same way; its mapping only keeps the zone it leads to.
pub fn lateral(screen: &Screen, pos: Vec2) -> f32 {
    let half = (screen.width * 0.5).max(f32::EPSILON);
    (pos - screen.center).dot(screen.right_axis()) / half
}

/// The spot on the screen a light's dashed line goes to.
pub fn anchor_on_screen(screen: &Screen, pos: Vec2) -> Vec2 {
    let (l, r) = screen.endpoints();
    let t = ((lateral(screen, pos) + 1.0) / 2.0).clamp(0.0, 1.0);
    Vec2::new(l.x + (r.x - l.x) * t, l.y + (r.y - l.y) * t)
}

/// The seat handle: in front of the screen, where "You" sit.
pub fn seat(screen: &Screen) -> Vec2 {
    let f = screen.facing();
    Vec2::new(screen.center.x + f.x * SEAT, screen.center.y + f.y * SEAT)
}

/// The way the screen faces when the seat is dragged to `p`, in whole
/// degrees, or in steps of 15 with `snap`; never -180, which is 180.
pub fn facing_toward(center: Vec2, p: Vec2, snap: bool) -> f32 {
    // facing = (sin θ, cos θ), so θ = atan2(dx, dy).
    let deg = (p.x - center.x).atan2(p.y - center.y).to_degrees();
    let mut deg = if snap {
        (deg / 15.0).round() * 15.0
    } else {
        deg.round()
    };
    if deg <= -180.0 {
        deg += 360.0;
    }
    deg
}

/// The screen's width when an end is dragged to `p`, in steps of 5.
pub fn width_toward(center: Vec2, p: Vec2) -> f32 {
    let half = dist(p, center);
    ((half * 2.0).clamp(SCREEN_MIN, SCREEN_MAX) / 5.0).round() * 5.0
}

// --- Zones ---

/// The three zones, in their order across the screen.
pub const EDGES: [ScreenEdge; 3] = [ScreenEdge::Left, ScreenEdge::Center, ScreenEdge::Right];

/// The key of a zone's letter (L, C, R in English; G, C, D in French): a
/// zone is never told by its colour alone.
pub fn letter_key(edge: ScreenEdge) -> &'static str {
    match edge {
        ScreenEdge::Left => "zone_letter_left",
        ScreenEdge::Center => "zone_letter_center",
        ScreenEdge::Right => "zone_letter_right",
    }
}

/// The key of a zone said in words.
pub fn kind_key(edge: ScreenEdge) -> &'static str {
    match edge {
        ScreenEdge::Left => "map_left",
        ScreenEdge::Center => "map_slice",
        ScreenEdge::Right => "map_right",
    }
}

/// The key of what a mapping does, said in words.
pub fn describe(m: &LightMapping) -> &'static str {
    if m.spread > 0.85 {
        "map_whole"
    } else if m.spread > 0.45 {
        "map_broad"
    } else {
        kind_key(m.edge)
    }
}

/// The key of a light's height in words.
pub fn height_key(v: f32) -> &'static str {
    if v < 0.18 {
        "h_floor"
    } else if v < 0.42 {
        "h_low"
    } else if v < 0.58 {
        "h_level"
    } else if v < 0.82 {
        "h_high"
    } else {
        "h_ceiling"
    }
}

// --- Naming corners ---

const WHERE: [[&str; 3]; 3] = [
    ["where_top_left", "where_top", "where_top_right"],
    ["where_left", "where_middle", "where_right"],
    ["where_bottom_left", "where_bottom", "where_bottom_right"],
];

/// The key of where corner `i` sits in the room as drawn ("top right"), by
/// the third of the room it falls in.
pub fn corner_where(walls: &[Vec2], i: usize) -> Option<&'static str> {
    let p = walls.get(i)?;
    let third = |v: f32, all: &mut dyn Iterator<Item = f32>| {
        let (lo, hi) = all.fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let f = (v - lo) / (hi - lo).max(1.0);
        if f < 1.0 / 3.0 {
            0
        } else if f > 2.0 / 3.0 {
            2
        } else {
            1
        }
    };
    let row = third(p.y, &mut walls.iter().map(|q| q.y));
    let col = third(p.x, &mut walls.iter().map(|q| q.x));
    Some(WHERE[row][col])
}

// --- Editing ---

/// The wall to split for "Add corner": the longest one, as its index (the
/// wall from corner `i` to the next).
pub fn longest_wall(walls: &[Vec2]) -> usize {
    let n = walls.len();
    let mut best = 0;
    let mut best_len = -1.0;
    for i in 0..n {
        let len = dist(walls[i], walls[(i + 1) % n]);
        if len > best_len {
            best_len = len;
            best = i;
        }
    }
    best
}

/// The wall nearest `p`: its index, the closest point on it and how far.
pub fn nearest_wall(walls: &[Vec2], p: Vec2) -> Option<(usize, Vec2, f32)> {
    let n = walls.len();
    (0..n)
        .map(|i| {
            let q = closest_on_segment(p, walls[i], walls[(i + 1) % n]);
            (i, q, dist(p, q))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Where new lights go: in a row just behind the screen, the usual
/// ambilight setup, spread evenly across its width. A spot already taken is
/// skipped, so lights never land on top of each other; the user drags them
/// to their real places afterwards.
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

/// "Light n" for the smallest n no light of the room is called yet, so a
/// light added after a removal never takes a name already in use. `name`
/// spells the name of number n.
pub fn fresh_name(lights: &[Light], name: impl Fn(usize) -> String) -> String {
    let n = lights.len() + 1;
    (1..=n)
        .map(&name)
        .find(|candidate| lights.iter().all(|l| l.name != *candidate))
        .unwrap_or_else(|| name(n))
}

/// A fresh light id: a random UUID, as the Tauri versions made them.
pub fn new_light_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// --- Framing ---

/// The part of the plan shown, in plan units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The view that fits whatever is drawn, so a small room fills the stage,
/// shaped like the stage so the plan is centred, not letterboxed.
pub fn fit(room: &Room, stage_w: f32, stage_h: f32) -> ViewBox {
    let (l, r) = room.screen.endpoints();
    let pts = room
        .walls
        .iter()
        .copied()
        .chain(room.lights.iter().map(|l| l.pos))
        .chain([l, r, seat(&room.screen)]);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let pad = 70.0;
    let mut x = x0 - pad;
    let mut y = y0 - pad;
    let mut w = (x1 + pad - x).max(400.0);
    let mut h = (y1 + pad - y).max(300.0);
    if stage_w > 0.0 && stage_h > 0.0 {
        let aspect = stage_w / stage_h;
        if w / h < aspect {
            let nw = h * aspect;
            x -= (nw - w) / 2.0;
            w = nw;
        } else {
            let nh = w / aspect;
            y -= (nh - h) / 2.0;
            h = nh;
        }
    }
    ViewBox { x, y, w, h }
}

/// The plan as drawn on a stage of `stage_w` by `stage_h` pixels: plan
/// units per pixel (`k`), and the box centred in the stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub vb: ViewBox,
    pub k: f32,
    /// The stage's top-left corner on screen, and the margin centring the
    /// box in it, in pixels.
    pub origin: (f32, f32),
}

impl Frame {
    pub fn new(vb: ViewBox, origin: (f32, f32), stage_w: f32, stage_h: f32) -> Self {
        let k = if stage_w > 0.0 && stage_h > 0.0 {
            (vb.w / stage_w).max(vb.h / stage_h)
        } else {
            1.0
        };
        let ox = origin.0 + (stage_w - vb.w / k) / 2.0;
        let oy = origin.1 + (stage_h - vb.h / k) / 2.0;
        Frame {
            vb,
            k,
            origin: (ox, oy),
        }
    }

    /// A plan point to pixels.
    pub fn to_px(self, p: Vec2) -> (f32, f32) {
        (
            self.origin.0 + (p.x - self.vb.x) / self.k,
            self.origin.1 + (p.y - self.vb.y) / self.k,
        )
    }

    /// A pixel to a plan point.
    pub fn to_plan(self, x: f32, y: f32) -> Vec2 {
        Vec2::new(
            self.vb.x + (x - self.origin.0) * self.k,
            self.vb.y + (y - self.origin.1) * self.k,
        )
    }
}

/// A light's name sits below it, or above when the light is just behind
/// the screen, so names never sit across the screen. `k` is the frame's
/// plan units per pixel, as the distances are measured on screen.
pub fn label_below(screen: &Screen, p: Vec2, k: f32) -> bool {
    let (l, r) = screen.endpoints();
    let q = closest_on_segment(p, l, r);
    let behind = (p.x - q.x).abs() < 70.0 * k && p.y < q.y && q.y - p.y < 120.0 * k;
    !behind
}

// --- What a press lands on ---

/// Something on the plan a press can grab.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Light(String),
    Corner(usize),
    /// An end of the selected screen, which sets its width.
    Width,
    Screen,
    Seat,
    /// A wall, where a double-click adds a corner.
    Wall,
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    dist(p, closest_on_segment(p, a, b))
}

/// What sits under `p`, the topmost first as the plan draws them: lights,
/// corners, the selected screen's ends, the screen, the seat, the walls.
/// The reach of each is in pixels, `k` units each, as drawn.
pub fn hit(room: &Room, p: Vec2, k: f32, screen_selected: bool) -> Option<Target> {
    if let Some(l) = room
        .lights
        .iter()
        .rev()
        .find(|l| dist(p, l.pos) <= 15.0 * k)
    {
        return Some(Target::Light(l.id.clone()));
    }
    if let Some(i) = room
        .walls
        .iter()
        .rposition(|w| (p.x - w.x).abs() <= 7.0 * k && (p.y - w.y).abs() <= 7.0 * k)
    {
        return Some(Target::Corner(i));
    }
    let (l, r) = room.screen.endpoints();
    if screen_selected && (dist(p, l) <= 8.0 * k || dist(p, r) <= 8.0 * k) {
        return Some(Target::Width);
    }
    if dist_to_segment(p, l, r) <= 11.0 * k {
        return Some(Target::Screen);
    }
    if dist(p, seat(&room.screen)) <= 14.0 * k {
        return Some(Target::Seat);
    }
    if let Some((_, _, d)) = nearest_wall(&room.walls, p)
        && d <= 6.0 * k
    {
        return Some(Target::Wall);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(id: &str, x: f32, y: f32) -> Light {
        Light {
            id: id.into(),
            name: id.into(),
            pos: Vec2::new(x, y),
            height: 0.5,
            zone_override: None,
            enabled: true,
        }
    }

    #[test]
    fn points_stay_on_the_plan_in_whole_units() {
        assert_eq!(clamp_p(Vec2::new(-5.0, 900.0)), Vec2::new(12.0, 738.0));
        assert_eq!(clamp_p(Vec2::new(100.4, 99.6)), Vec2::new(100.0, 100.0));
    }

    #[test]
    fn the_seat_turns_the_screen_toward_it() {
        let c = Vec2::new(500.0, 160.0);
        assert_eq!(facing_toward(c, Vec2::new(500.0, 400.0), false), 0.0);
        assert_eq!(facing_toward(c, Vec2::new(700.0, 160.0), false), 90.0);
        // Straight up is 180, never -180.
        assert_eq!(facing_toward(c, Vec2::new(500.0, 0.0), false), 180.0);
        // Shift snaps to 15°.
        assert_eq!(facing_toward(c, Vec2::new(600.0, 400.0), true), 30.0);
        // The default screen's seat is straight in front of it.
        let s = Screen::default();
        assert_eq!(seat(&s), Vec2::new(500.0, 280.0));
    }

    #[test]
    fn the_screen_width_stays_in_bounds_in_steps_of_five() {
        let c = Vec2::new(500.0, 160.0);
        assert_eq!(width_toward(c, Vec2::new(501.0, 160.0)), SCREEN_MIN);
        assert_eq!(width_toward(c, Vec2::new(1000.0, 160.0)), SCREEN_MAX);
        assert_eq!(width_toward(c, Vec2::new(651.0, 160.0)), 300.0);
    }

    #[test]
    fn a_light_points_at_its_spot_on_the_screen() {
        let s = Screen::default();
        let (l, r) = s.endpoints();
        assert_eq!(anchor_on_screen(&s, Vec2::new(0.0, 100.0)), l);
        assert_eq!(anchor_on_screen(&s, Vec2::new(990.0, 100.0)), r);
        assert_eq!(anchor_on_screen(&s, Vec2::new(500.0, 40.0)), s.center);
        assert!(lateral(&s, Vec2::new(300.0, 100.0)) < -1.0);
    }

    #[test]
    fn corners_are_named_by_where_they_sit() {
        let walls = Room::default().walls;
        assert_eq!(corner_where(&walls, 0), Some("where_top_left"));
        assert_eq!(corner_where(&walls, 2), Some("where_bottom_right"));
        let mut more = walls.clone();
        more.insert(1, Vec2::new(500.0, 50.0));
        assert_eq!(corner_where(&more, 1), Some("where_top"));
        assert_eq!(corner_where(&walls, 9), None);
    }

    #[test]
    fn walls_are_found_and_the_longest_split() {
        let walls = Room::default().walls;
        assert_eq!(longest_wall(&walls), 0);
        let (i, q, d) = nearest_wall(&walls, Vec2::new(960.0, 300.0)).unwrap();
        assert_eq!((i, q), (1, Vec2::new(950.0, 300.0)));
        assert!((d - 10.0).abs() < 1e-3);
    }

    #[test]
    fn new_lights_never_land_on_each_other() {
        let mut room = Room::default();
        let first = fresh_positions(&room, 1)[0];
        // One row behind the screen, at its middle.
        assert_eq!(first, Vec2::new(500.0, 115.0));
        room.lights.push(light("a", first.x, first.y));
        let next = fresh_positions(&room, 1)[0];
        assert!(dist(next, first) > 45.0);
        let three = fresh_positions(&Room::default(), 3);
        assert_eq!(three[0].x, 320.0);
        assert_eq!(three[2].x, 680.0);
    }

    #[test]
    fn a_fresh_name_takes_the_first_free_number() {
        let name = |n: usize| format!("Light {n}");
        let mut lights = vec![light("a", 0.0, 0.0), light("b", 0.0, 0.0)];
        lights[0].name = "Light 2".into();
        lights[1].name = "Desk".into();
        assert_eq!(fresh_name(&lights, name), "Light 1");
        lights[1].name = "Light 1".into();
        assert_eq!(fresh_name(&lights, name), "Light 3");
    }

    #[test]
    fn ids_are_fresh_and_shaped_like_uuids() {
        let (a, b) = (new_light_id(), new_light_id());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(a.matches('-').count(), 4);
    }

    #[test]
    fn the_frame_fits_the_stage_and_maps_both_ways() {
        let room = Room::default();
        let vb = fit(&room, 800.0, 600.0);
        assert!((vb.w / vb.h - 800.0 / 600.0).abs() < 1e-3);
        assert!(vb.x <= 50.0 - 70.0 && vb.x + vb.w >= 950.0 + 70.0);
        let f = Frame::new(vb, (10.0, 20.0), 800.0, 600.0);
        let p = Vec2::new(321.0, 123.0);
        let (x, y) = f.to_px(p);
        let back = f.to_plan(x, y);
        assert!((back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3);
    }

    #[test]
    fn a_press_finds_the_topmost_thing() {
        let mut room = Room::default();
        room.lights.push(light("a", 500.0, 115.0));
        let k = 1.0;
        assert_eq!(
            hit(&room, Vec2::new(505.0, 118.0), k, false),
            Some(Target::Light("a".into()))
        );
        assert_eq!(
            hit(&room, Vec2::new(53.0, 48.0), k, false),
            Some(Target::Corner(0))
        );
        let (_, r) = room.screen.endpoints();
        assert_eq!(hit(&room, r, k, true), Some(Target::Width));
        assert_eq!(hit(&room, r, k, false), Some(Target::Screen));
        assert_eq!(hit(&room, seat(&room.screen), k, false), Some(Target::Seat));
        assert_eq!(
            hit(&room, Vec2::new(500.0, 702.0), k, false),
            Some(Target::Wall)
        );
        assert_eq!(hit(&room, Vec2::new(300.0, 400.0), k, false), None);
    }

    #[test]
    fn names_go_above_lights_just_behind_the_screen() {
        let s = Screen::default();
        assert!(!label_below(&s, Vec2::new(500.0, 115.0), 1.0));
        assert!(label_below(&s, Vec2::new(500.0, 400.0), 1.0));
    }

    #[test]
    fn a_mapping_is_said_in_words() {
        let s = Screen::default();
        let near = lueur_core::geometry::map_light(&s, &light("a", 500.0, 150.0));
        assert_eq!(describe(&near), "map_slice");
        let far = lueur_core::geometry::map_light(&s, &light("a", 500.0, 700.0));
        assert_eq!(describe(&far), "map_whole");
        assert_eq!(height_key(0.5), "h_level");
        assert_eq!(height_key(0.9), "h_ceiling");
    }
}
