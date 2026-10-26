//! # lueur-core
//!
//! The GUI-free heart of Lueur, a low-latency ambilight for smart bulbs.
//!
//! It is split into small, independently testable modules:
//!
//! * [`color`]: colour types, linear-space averaging and temporal smoothing.
//! * [`geometry`]: the room/floor-plan model and the mapping from a light's
//!   physical position to the region of the screen it mirrors.
//! * [`engine`]: turning a captured [`engine::Frame`] into one colour per light.
//! * [`yeelight`]: the Yeelight LAN protocol, discovery and music-mode control.
//! * [`device`]: every bulb family (Yeelight, WiZ, LIFX, Govee, Hue, Nanoleaf)
//!   behind one stream / power / identify interface, plus their discovery, and
//!   the data points a Tuya bulb takes (its transport lives in the shell).
//! * [`scene`]: lights held steady by hand, and the built-in moods.
//! * [`show`]: light shows without a screen: palette fades, a candle, music.
//! * [`stage`]: the music show: listening to the track, and playing it on
//!   the lights like a stage.
//! * [`sync`]: keeping lights that answer at different speeds together.
//! * [`safety`]: the photosensitivity protection every mode's output goes
//!   through: no light flashes more than three times a second.
//! * [`config`]: persisted settings, read from and written to the data
//!   directory the shell picks.
//!
//! The desktop shell (`src-tauri`) supplies real screen capture and the webview;
//! everything colour- and protocol-critical lives here so it can be unit-tested
//! without a display server.

pub mod color;
pub mod config;
pub mod device;
pub mod engine;
pub mod geometry;
pub mod safety;
pub mod scene;
pub mod show;
pub mod stage;
pub mod sync;
pub mod yeelight;

pub use color::Rgb;
pub use engine::{Ambilight, EngineSettings, Frame, PixelFormat};
pub use geometry::{Light, LightMapping, Room, SampleZone, Screen, ScreenEdge, Vec2, map_light};

/// Serde default for flags that start out on.
pub(crate) fn default_true() -> bool {
    true
}
