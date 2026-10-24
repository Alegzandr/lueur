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
//! * [`device`] — every bulb family (Yeelight, WiZ, LIFX, Govee, Hue, Nanoleaf)
//!   behind one stream / power / identify interface, plus their discovery.
//! * [`scene`] — lights held steady by hand: built-in moods and saved scenes.
//! * [`config`] — persisted settings and portable data-dir resolution.
//!
//! The desktop shell (`src-tauri`) supplies real screen capture and the webview;
//! everything colour- and protocol-critical lives here so it can be unit-tested
//! without a display server.

pub mod color;
pub mod config;
pub mod device;
pub mod engine;
pub mod geometry;
pub mod scene;
pub mod yeelight;

pub use color::Rgb;
pub use engine::{Ambilight, EngineSettings, Frame, PixelFormat};
pub use geometry::{Light, LightMapping, Room, SampleZone, Screen, ScreenEdge, Vec2, map_light};

/// Compute the mapping for every light in a room, in light order.
///
/// This is the bridge the engine consumes each frame: the room defines *where*
/// the lights are, this yields *what* each should sample and *how* it should
/// behave. Disabled lights are skipped by the caller via [`Light::enabled`];
/// here we return a mapping for every light so indices line up with the room's
/// light list.
pub fn room_mappings(room: &Room) -> Vec<LightMapping> {
    room.lights
        .iter()
        .map(|l| map_light(&room.screen, l))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_mappings_has_one_per_light() {
        let mut room = Room::default();
        room.lights.push(Light {
            id: "a".into(),
            name: "a".into(),
            pos: Vec2::new(100.0, 200.0),
            height: 0.5,
            zone_override: None,
            enabled: true,
        });
        room.lights.push(Light {
            id: "b".into(),
            name: "b".into(),
            pos: Vec2::new(900.0, 200.0),
            height: 0.5,
            zone_override: None,
            enabled: true,
        });
        assert_eq!(room_mappings(&room).len(), 2);
    }
}
