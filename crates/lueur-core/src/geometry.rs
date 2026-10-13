//! Room layout and the geometry that maps each light in the room to the region
//! of the screen it should mirror.
//!
//! The user draws their room as a top-down floor plan, drops their screen/PC in
//! it (with a facing direction), and places lights around the room. From that we
//! derive, for every light, a rectangular sample zone in *normalised screen
//! coordinates* (x, y, w, h all in 0..=1, origin top-left). The engine then
//! averages the captured pixels inside that zone to drive the light.
//!
//! The mapping reads three things off the plan instead of one:
//!   * **Lateral position** — where the light sits along the screen's width
//!     selects the horizontal centre of its zone.
//!   * **Distance** — how far the light is *in front of* the screen plane, and
//!     how far it overshoots the screen's ends, widens the zone continuously.
//!     A lamp tucked behind the panel mirrors a tight slice; one standing next
//!     to the player mirrors a broad region; one behind the couch mirrors
//!     essentially the whole picture, which is what reads as ambient light
//!     rather than a nervous, flickering slice.
//!   * **Height** — a per-light 0 (floor) … 1 (ceiling) value the user sets,
//!     which selects the vertical band. It is the one thing a top-down plan
//!     genuinely cannot know, so we ask for it rather than pretending.
//!
//! Distance also shapes *how* the light behaves, not just what it looks at:
//! far lamps are dimmed and smoothed harder, because indirect light that snaps
//! at 60 Hz from across the room reads as a bug, not as atmosphere.

use serde::{Deserialize, Serialize};

/// A 2D point/vector in room coordinates. Units are arbitrary (the editor works
/// in a normalised 0..1000 space); only ratios matter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    pub fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }

    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn len(self) -> f32 {
        self.dot(self).sqrt()
    }

    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l <= f32::EPSILON {
            Vec2::new(0.0, 0.0)
        } else {
            Vec2::new(self.x / l, self.y / l)
        }
    }
}

/// The screen/monitor placed in the room, seen from above.
///
/// It is a line segment of a given `width`, centred at `center`, oriented so
/// that `facing` (a unit-ish vector) points from the screen toward the viewer.
/// `facing_deg` is stored for the UI; `facing()` derives the vector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub center: Vec2,
    /// Physical width of the screen in room units.
    pub width: f32,
    /// Direction the screen faces (toward the player), in degrees.
    /// 0° points along +Y (down the plan / toward a couch below the screen).
    pub facing_deg: f32,
}

impl Default for Screen {
    fn default() -> Self {
        // Sit the screen near the top wall, facing down into the room where the
        // player sits. right_axis is then +X, so the viewer's right matches the
        // plan's right — the intuitive default.
        Screen {
            center: Vec2::new(500.0, 160.0),
            width: 300.0,
            facing_deg: 0.0,
        }
    }
}

impl Screen {
    /// Unit vector the screen faces (toward the viewer).
    pub fn facing(&self) -> Vec2 {
        let rad = self.facing_deg.to_radians();
        Vec2::new(rad.sin(), rad.cos())
    }

    /// Unit vector along the screen's width, pointing from its left end (as the
    /// viewer sees it) to its right end. This is `facing` rotated +90°, which
    /// keeps left/right matching the viewer's perspective.
    pub fn right_axis(&self) -> Vec2 {
        let f = self.facing();
        // Rotate facing by -90° so the viewer's right maps to screen-right.
        Vec2::new(f.y, -f.x)
    }

    /// Left and right endpoints of the screen segment (viewer's left / right).
    pub fn endpoints(&self) -> (Vec2, Vec2) {
        let r = self.right_axis();
        let half = self.width * 0.5;
        let left = Vec2::new(self.center.x - r.x * half, self.center.y - r.y * half);
        let right = Vec2::new(self.center.x + r.x * half, self.center.y + r.y * half);
        (left, right)
    }
}

/// Which broad region of the screen a light mirrors. Useful for the UI to draw a
/// hint and for tests to assert intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenEdge {
    Left,
    Right,
    /// Directly behind the screen — mirrors a vertical slice at some x.
    Center,
}

/// A rectangular region of the captured frame, in normalised coordinates
/// (0..=1, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SampleZone {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl SampleZone {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        // Clamp to the unit square so a bad layout can never index out of frame.
        let x = x.clamp(0.0, 1.0);
        let y = y.clamp(0.0, 1.0);
        let w = w.clamp(0.0, 1.0 - x);
        let h = h.clamp(0.0, 1.0 - y);
        SampleZone { x, y, w, h }
    }
}

/// A single light placed in the room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Light {
    /// Stable id used to bind this placement to a physical device.
    pub id: String,
    /// Friendly label shown in the UI.
    pub name: String,
    /// Position in the room (floor plan) coordinates.
    pub pos: Vec2,
    /// Vertical placement in the room: 0 = on the floor, 1 = at the ceiling,
    /// 0.5 = level with the middle of the screen. A floor plan can't express
    /// this, so the user sets it; it selects which horizontal band of the
    /// picture the light mirrors.
    #[serde(default = "default_height")]
    pub height: f32,
    /// Optional manual override of the auto-computed sample zone.
    #[serde(default)]
    pub zone_override: Option<SampleZone>,
    /// Whether this light participates in the ambilight effect.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_height() -> f32 {
    0.5
}

/// The full room layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    /// Room outline as a closed polygon of wall corners (floor plan). Purely
    /// cosmetic for the mapping, but drives the editor and future occlusion.
    #[serde(default)]
    pub walls: Vec<Vec2>,
    pub screen: Screen,
    #[serde(default)]
    pub lights: Vec<Light>,
}

impl Default for Room {
    fn default() -> Self {
        Room {
            walls: vec![
                Vec2::new(50.0, 50.0),
                Vec2::new(950.0, 50.0),
                Vec2::new(950.0, 700.0),
                Vec2::new(50.0, 700.0),
            ],
            screen: Screen::default(),
            lights: Vec::new(),
        }
    }
}

/// Everything the engine needs to drive one light, derived from the plan.
///
/// [`LightMapping::zone`] says *what* to look at; `gain` and `responsiveness`
/// say *how* to render it. The last two are what stop a lamp on the far side of
/// the room from behaving like a monitor backlight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LightMapping {
    /// Region of the frame this light averages.
    pub zone: SampleZone,
    /// Broad classification, for UI hints only.
    pub edge: ScreenEdge,
    /// How diffuse this light's view is: 0 = tight slice (right behind the
    /// panel), 1 = essentially the whole picture (across the room).
    pub spread: f32,
    /// Brightness multiplier applied on top of the user's setting. Falls off
    /// with distance so distant lamps stay a glow, not a spotlight.
    pub gain: f32,
    /// Multiplier on the smoothing factor. Below 1 = slower, dreamier response,
    /// which is how distant indirect light actually behaves.
    pub responsiveness: f32,
}

impl LightMapping {
    /// A mapping that samples `zone` verbatim, with no distance shaping. Used
    /// for manual zone overrides and by tests.
    pub fn direct(zone: SampleZone) -> Self {
        LightMapping {
            zone,
            edge: ScreenEdge::Center,
            spread: 0.0,
            gain: 1.0,
            responsiveness: 1.0,
        }
    }
}

/// Half-width of the tightest zone: a light right behind the panel mirrors a
/// slice this wide on each side of its position.
const MIN_HALF_W: f32 = 0.08;
/// Half-height of the tightest zone. Looser than the horizontal one because a
/// user-set height is a coarser signal than a dragged floor-plan position.
const MIN_HALF_H: f32 = 0.28;
/// A half-extent of 0.5 covers the whole frame on that axis.
const MAX_HALF: f32 = 0.5;
/// Extra half-extent per screen half-width of distance in front of the panel.
/// At ~2.4 half-widths (roughly where a player sits) the zone is the full frame.
const FRONT_WIDEN: f32 = 0.21;
/// Extra half-extent per screen half-width of lateral overshoot past an end.
const LATERAL_WIDEN: f32 = 0.14;
/// Distance (in screen half-widths) at which attenuation reaches its halfway
/// point. Tuned so lights around the sofa are noticeably softened.
const FALLOFF: f32 = 2.0;
/// Floor for the distance brightness multiplier — never fully dim a light out.
const MIN_GAIN: f32 = 0.55;
/// Floor for the distance smoothing multiplier.
const MIN_RESPONSIVENESS: f32 = 0.35;

/// Compute the full [`LightMapping`] for a light given the screen placement.
///
/// Honours `zone_override` when present, in which case no distance shaping is
/// applied: an explicit zone is a promise the user made, not a hint.
pub fn map_light(screen: &Screen, light: &Light) -> LightMapping {
    if let Some(z) = light.zone_override {
        return LightMapping::direct(z);
    }

    let right = screen.right_axis();
    let half = (screen.width * 0.5).max(f32::EPSILON);
    let rel = light.pos.sub(screen.center);
    // Signed position along the screen's width: -1 = left end, +1 = right end.
    let u = rel.dot(right) / half;
    // Signed depth: positive = on the viewer's side of the screen plane.
    let v = rel.dot(screen.facing()) / half;

    let edge = if u <= -1.0 {
        ScreenEdge::Left
    } else if u >= 1.0 {
        ScreenEdge::Right
    } else {
        ScreenEdge::Center
    };

    // How far the light has escaped the screen: past an end, and out in front.
    // Both widen the zone, continuously — there is no cliff at the screen's end
    // any more, a light just past the corner sees almost what its neighbour
    // just inside does.
    let lateral_out = (u.abs() - 1.0).max(0.0);
    let front = v.max(0.0);
    let widen = FRONT_WIDEN * front + LATERAL_WIDEN * lateral_out;

    let hw = (MIN_HALF_W + widen).min(MAX_HALF);
    let hh = (MIN_HALF_H + widen).min(MAX_HALF);
    let spread = ((hw - MIN_HALF_W) / (MAX_HALF - MIN_HALF_W)).clamp(0.0, 1.0);

    // Centre of the zone: lateral position picks x, user-set height picks y
    // (height 1 = ceiling = top of the frame, so y is inverted).
    let x_center = ((u + 1.0) * 0.5).clamp(0.0, 1.0);
    let y_center = 1.0 - light.height.clamp(0.0, 1.0);
    // Slide the zone so it stays wholly inside the frame rather than being
    // truncated at an edge — a clipped zone would silently shrink its sample.
    let x0 = (x_center - hw).clamp(0.0, (1.0 - 2.0 * hw).max(0.0));
    let y0 = (y_center - hh).clamp(0.0, (1.0 - 2.0 * hh).max(0.0));
    let zone = SampleZone::new(x0, y0, 2.0 * hw, 2.0 * hh);

    // Straight-line distance from the light to the screen segment, in screen
    // half-widths: lateral overshoot and depth combined.
    let dist = (lateral_out * lateral_out + v * v).sqrt();
    let t = dist / FALLOFF;
    let atten = 1.0 / (1.0 + t * t);
    let gain = MIN_GAIN + (1.0 - MIN_GAIN) * atten;
    let responsiveness = MIN_RESPONSIVENESS + (1.0 - MIN_RESPONSIVENESS) * atten;

    LightMapping {
        zone,
        edge,
        spread,
        gain,
        responsiveness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_at(x: f32, y: f32) -> Light {
        Light {
            id: "l".into(),
            name: "l".into(),
            pos: Vec2::new(x, y),
            height: 0.5,
            zone_override: None,
            enabled: true,
        }
    }

    /// A light at the given position and height, on the default screen.
    fn map_at(x: f32, y: f32) -> LightMapping {
        map_light(&Screen::default(), &light_at(x, y))
    }

    #[test]
    fn right_axis_is_perpendicular_to_facing() {
        let s = Screen::default();
        let dot = s.facing().dot(s.right_axis());
        assert!(dot.abs() < 1e-5, "axes not perpendicular: {dot}");
    }

    #[test]
    fn default_screen_faces_into_the_room() {
        // facing_deg 0 -> points along +Y (down the plan, toward the player).
        let s = Screen::default();
        let f = s.facing();
        assert!(f.y > 0.99, "expected facing +Y, got {:?}", f);
    }

    #[test]
    fn light_left_of_screen_maps_to_left_edge() {
        // Default screen centered at (500,160), facing +Y. right_axis is +X, so
        // the viewer's left is -X. A light far in -X is therefore to the LEFT.
        let s = Screen::default();
        assert_eq!(
            map_light(&s, &light_at(100.0, 160.0)).edge,
            ScreenEdge::Left
        );
    }

    #[test]
    fn light_right_of_screen_maps_to_right_edge() {
        let s = Screen::default();
        assert_eq!(
            map_light(&s, &light_at(900.0, 160.0)).edge,
            ScreenEdge::Right
        );
    }

    #[test]
    fn light_behind_center_maps_to_center_slice() {
        let s = Screen::default();
        let m = map_light(&s, &light_at(500.0, 100.0));
        let zone = m.zone;
        assert_eq!(m.edge, ScreenEdge::Center);
        // Slice should straddle the horizontal middle of the frame.
        let mid = zone.x + zone.w * 0.5;
        assert!((mid - 0.5).abs() < 0.05, "slice not centered: {mid}");
    }

    #[test]
    fn center_slice_tracks_horizontal_position() {
        let s = Screen::default();
        // right_axis is +X, so a light toward +X is toward the viewer's right
        // -> its slice should sit right-of-centre in the frame.
        let zone = map_light(&s, &light_at(580.0, 100.0)).zone;
        let mid = zone.x + zone.w * 0.5;
        assert!(mid > 0.5, "expected right-of-centre slice, got {mid}");
    }

    #[test]
    fn zone_override_is_respected() {
        let s = Screen::default();
        let mut l = light_at(500.0, 100.0);
        let z = SampleZone::new(0.2, 0.3, 0.1, 0.1);
        l.zone_override = Some(z);
        let zone = map_light(&s, &l).zone;
        assert_eq!(zone, z);
    }

    #[test]
    fn distance_in_front_widens_the_zone() {
        // Tucked behind the panel -> tight slice. Out where the player sits ->
        // a broad region.
        let behind = map_at(500.0, 100.0);
        let in_front = map_at(500.0, 460.0);
        assert!(
            in_front.zone.w > behind.zone.w * 3.0,
            "expected a much wider zone in front: {} vs {}",
            in_front.zone.w,
            behind.zone.w
        );
        assert!(in_front.spread > behind.spread);
    }

    #[test]
    fn behind_the_player_mirrors_almost_everything() {
        // Far down the room, behind where the player sits.
        let m = map_at(500.0, 700.0);
        assert!(m.zone.w > 0.95 && m.zone.h > 0.95, "got {:?}", m.zone);
        assert!(m.spread > 0.99, "expected full spread, got {}", m.spread);
    }

    #[test]
    fn zone_is_continuous_across_the_screen_end() {
        // Sweep a light past the right end of the screen and assert no jump:
        // the old mapping snapped from a centre slice to an edge column here.
        let s = Screen::default();
        let mut prev: Option<SampleZone> = None;
        let mut x = 620.0;
        while x <= 760.0 {
            let z = map_light(&s, &light_at(x, 160.0)).zone;
            if let Some(p) = prev {
                assert!(
                    (z.x - p.x).abs() < 0.05 && (z.w - p.w).abs() < 0.05,
                    "discontinuity at x={x}: {p:?} -> {z:?}"
                );
            }
            prev = Some(z);
            x += 5.0;
        }
    }

    #[test]
    fn height_selects_the_vertical_band() {
        let s = Screen::default();
        let mut low = light_at(500.0, 100.0);
        low.height = 0.0;
        let mut high = light_at(500.0, 100.0);
        high.height = 1.0;

        let low_z = map_light(&s, &low).zone;
        let high_z = map_light(&s, &high).zone;
        // Floor light looks at the bottom of the picture, ceiling light the top.
        assert!(high_z.y < low_z.y, "{high_z:?} should sit above {low_z:?}");
        assert!(
            (high_z.y - 0.0).abs() < 1e-5,
            "ceiling band should touch the top"
        );
        assert!(
            (low_z.y + low_z.h - 1.0).abs() < 1e-5,
            "floor band should touch the bottom"
        );
    }

    #[test]
    fn far_lights_are_dimmed_and_slowed() {
        let near = map_at(500.0, 150.0);
        let far = map_at(500.0, 700.0);
        assert!(
            (near.gain - 1.0).abs() < 0.05,
            "near light should be full brightness"
        );
        assert!(far.gain < near.gain, "far light should be dimmer");
        assert!(
            far.responsiveness < near.responsiveness,
            "far light should be slower"
        );
        // Never fully dim or freeze a light.
        assert!(far.gain >= MIN_GAIN && far.responsiveness >= MIN_RESPONSIVENESS);
    }

    #[test]
    fn override_skips_distance_shaping() {
        let s = Screen::default();
        let mut l = light_at(500.0, 700.0); // far away: would normally be shaped
        l.zone_override = Some(SampleZone::new(0.2, 0.3, 0.1, 0.1));
        let m = map_light(&s, &l);
        assert_eq!(m.gain, 1.0);
        assert_eq!(m.responsiveness, 1.0);
    }

    #[test]
    fn sample_zone_is_clamped_into_frame() {
        let z = SampleZone::new(0.9, 0.9, 0.5, 0.5);
        assert!(z.x + z.w <= 1.0 + 1e-6);
        assert!(z.y + z.h <= 1.0 + 1e-6);
    }
}
