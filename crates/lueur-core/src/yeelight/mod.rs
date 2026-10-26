//! Yeelight support: protocol encoding, LAN discovery and TCP control.
//!
//! The design keeps *what to say* (pure, in [`protocol`]) separate from *how to
//! say it* (sockets, in [`connection`] and [`discovery`]) so the fiddly wire
//! format is fully unit-tested.

pub mod connection;
pub mod discovery;
pub mod protocol;

pub use connection::{MusicConnection, YeelightControl};
pub use discovery::discover;
pub use protocol::{
    DEFAULT_CONTROL_PORT, DiscoveredDevice, Effect, MIN_SMOOTH_DURATION_MS, parse_discovery,
};

/// How long a command on the music channel takes to show: a TCP write on
/// the LAN, and the LEDs' own rise.
pub const LATENCY: std::time::Duration = std::time::Duration::from_millis(45);
