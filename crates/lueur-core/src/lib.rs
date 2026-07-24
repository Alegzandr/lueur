//! # lueur-core
//!
//! The GUI-free heart of Lueur, a low-latency ambilight for smart bulbs.
//!
//! It is split into small, independently testable modules:
//!
//! * [`color`] — colour types, linear-space averaging and temporal smoothing.
//! * [`geometry`] — the room/floor-plan model and the mapping from a light's
//!   physical position to the region of the screen it mirrors.
//! * [`engine`] — turning a captured [`engine::Frame`] into one colour per light.
//! * [`yeelight`] — the Yeelight LAN protocol, discovery and music-mode control.
//! * [`config`] — persisted settings and portable data-dir resolution.
//!
//! The desktop shell (`src-tauri`) supplies real screen capture and the webview;
//! everything colour- and protocol-critical lives here so it can be unit-tested
//! without a display server.

pub mod color;
pub mod config;
pub mod engine;
pub mod geometry;
pub mod yeelight;

pub use color::Rgb;
pub use engine::{Ambilight, EngineSettings, Frame, PixelFormat};
pub use geometry::{zone_for_light, Light, Room, SampleZone, Screen, ScreenEdge, Vec2};

/// Compute the sample zone for every light in a room, in light order.
///
/// This is the bridge the engine consumes each frame: the room defines *where*
/// the lights are, this yields *what* each should sample. Disabled lights are
/// skipped by the caller via [`Light::enabled`]; here we return a zone for every
/// light so indices line up with the room's light list.
pub fn room_zones(room: &Room) -> Vec<SampleZone> {
    room.lights
        .iter()
        .map(|l| zone_for_light(&room.screen, l).0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_zones_has_one_per_light() {
        let mut room = Room::default();
        room.lights.push(Light {
            id: "a".into(),
            name: "a".into(),
            pos: Vec2::new(100.0, 200.0),
            zone_override: None,
            enabled: true,
        });
        room.lights.push(Light {
            id: "b".into(),
            name: "b".into(),
            pos: Vec2::new(900.0, 200.0),
            zone_override: None,
            enabled: true,
        });
        assert_eq!(room_zones(&room).len(), 2);
    }
}
