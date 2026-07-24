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
    parse_discovery, DiscoveredDevice, Effect, DEFAULT_CONTROL_PORT, MIN_SMOOTH_DURATION_MS,
};
