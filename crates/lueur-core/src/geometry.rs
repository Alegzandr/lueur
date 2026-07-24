//! Room layout and the geometry that maps each light in the room to the region
//! of the screen it should mirror.
//!
//! The user draws their room as a top-down floor plan, drops their screen/PC in
//! it (with a facing direction), and places lights around the room. From that we
//! derive, for every light, a rectangular sample zone in *normalised screen
//! coordinates* (x, y, w, h all in 0..=1, origin top-left). The engine then
//! averages the captured pixels inside that zone to drive the light.
//!
//! The mapping is intentionally honest about what a floor plan can tell us:
//!   * A light's position *along the screen's width* selects a horizontal slice.
//!   * A light sitting beyond the left/right end of the screen mirrors that
//!     whole edge column.
//!   * A light behind the screen mirrors the vertical slice directly behind it.
//! This produces the classic, convincing "bias lighting follows the picture"
//! effect without pretending we know the vertical placement of each lamp.

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

/// How much of the frame edge (as a fraction) a side/edge light samples.
const EDGE_THICKNESS: f32 = 0.14;
/// Half-width of the horizontal slice a "center" (behind-screen) light samples.
const SLICE_HALF_WIDTH: f32 = 0.08;

/// Compute the sample zone for a light given the screen placement.
///
/// Returns the zone plus the broad [`ScreenEdge`] it corresponds to (handy for
/// UI hints). Honours `zone_override` when present.
pub fn zone_for_light(screen: &Screen, light: &Light) -> (SampleZone, ScreenEdge) {
    if let Some(z) = light.zone_override {
        return (z, ScreenEdge::Center);
    }

    let right = screen.right_axis();
    let half = (screen.width * 0.5).max(f32::EPSILON);
    // Signed position of the light along the screen's width axis, normalised so
    // -1 = left endpoint, +1 = right endpoint.
    let rel = light.pos.sub(screen.center);
    let u = rel.dot(right) / half;

    // Map u (-1..1 across the screen) to a horizontal frame coordinate 0..1.
    let x_center = ((u + 1.0) * 0.5).clamp(0.0, 1.0);

    if u <= -1.0 {
        // Beyond the left end of the screen -> mirror the left edge column.
        (
            SampleZone::new(0.0, 0.0, EDGE_THICKNESS, 1.0),
            ScreenEdge::Left,
        )
    } else if u >= 1.0 {
        // Beyond the right end -> mirror the right edge column.
        (
            SampleZone::new(1.0 - EDGE_THICKNESS, 0.0, EDGE_THICKNESS, 1.0),
            ScreenEdge::Right,
        )
    } else {
        // Behind the screen -> mirror the vertical slice at this x.
        let x0 = (x_center - SLICE_HALF_WIDTH).clamp(0.0, 1.0 - 2.0 * SLICE_HALF_WIDTH);
        (
            SampleZone::new(x0, 0.0, 2.0 * SLICE_HALF_WIDTH, 1.0),
            ScreenEdge::Center,
        )
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
            zone_override: None,
            enabled: true,
        }
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
        let (_zone, edge) = zone_for_light(&s, &light_at(100.0, 160.0));
        assert_eq!(edge, ScreenEdge::Left);
    }

    #[test]
    fn light_right_of_screen_maps_to_right_edge() {
        let s = Screen::default();
        let (_zone, edge) = zone_for_light(&s, &light_at(900.0, 160.0));
        assert_eq!(edge, ScreenEdge::Right);
    }

    #[test]
    fn light_behind_center_maps_to_center_slice() {
        let s = Screen::default();
        let (zone, edge) = zone_for_light(&s, &light_at(500.0, 100.0));
        assert_eq!(edge, ScreenEdge::Center);
        // Slice should straddle the horizontal middle of the frame.
        let mid = zone.x + zone.w * 0.5;
        assert!((mid - 0.5).abs() < 0.05, "slice not centered: {mid}");
    }

    #[test]
    fn center_slice_tracks_horizontal_position() {
        let s = Screen::default();
        // right_axis is +X, so a light toward +X is toward the viewer's right
        // -> its slice should sit right-of-centre in the frame.
        let (zone, _e) = zone_for_light(&s, &light_at(580.0, 100.0));
        let mid = zone.x + zone.w * 0.5;
        assert!(mid > 0.5, "expected right-of-centre slice, got {mid}");
    }

    #[test]
    fn zone_override_is_respected() {
        let s = Screen::default();
        let mut l = light_at(500.0, 100.0);
        let z = SampleZone::new(0.2, 0.3, 0.1, 0.1);
        l.zone_override = Some(z);
        let (zone, _e) = zone_for_light(&s, &l);
        assert_eq!(zone, z);
    }

    #[test]
    fn sample_zone_is_clamped_into_frame() {
        let z = SampleZone::new(0.9, 0.9, 0.5, 0.5);
        assert!(z.x + z.w <= 1.0 + 1e-6);
        assert!(z.y + z.h <= 1.0 + 1e-6);
    }
}
