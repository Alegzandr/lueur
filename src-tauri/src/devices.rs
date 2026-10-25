//! One entry point for every bulb family.
//!
//! [`lueur_core::device`] drives the families it can speak with the standard
//! library alone; Tuya needs an encrypted transport the core leaves to the
//! shell (`tuya.rs`). Commands and the runner go through here and never care
//! which is which.

use crate::tuya;
use lueur_core::color::Rgb;
use lueur_core::config::DeviceBinding;
use lueur_core::device::{
    self, DeviceKind, DiscoveredDevice, Endpoint, LightStream, Pairings, tuya::ID_PREFIX,
};
use std::collections::HashMap;
use std::io;
use std::time::Duration;

fn is_tuya(ep: &Endpoint) -> bool {
    matches!(ep.kind, DeviceKind::Tuya { .. })
}

/// Open a per-frame stream to `ep` (see [`device::open_stream`] for
/// `siblings` and `polite`).
pub fn open_stream(
    ep: &Endpoint,
    siblings: usize,
    polite: bool,
) -> io::Result<Box<dyn LightStream>> {
    let opened = if is_tuya(ep) {
        tuya::open_stream(ep)
    } else {
        device::open_stream(ep, siblings, polite)
    };
    if let Err(e) = &opened {
        log::debug!(
            family = ep.kind.family(), id = ep.id.as_str(), ip = ep.ip.as_str();
            "cannot open a stream: {e}"
        );
    }
    opened
}

/// The pace a stream to `ep` would keep, without opening one.
pub fn interval_of(ep: &Endpoint, siblings: usize) -> Duration {
    if is_tuya(ep) {
        tuya::FRAME_INTERVAL
    } else {
        device::interval_of(&ep.kind, siblings)
    }
}

pub fn set_power(ep: &Endpoint, on: bool) -> io::Result<()> {
    if is_tuya(ep) {
        tuya::set_power(ep, on)
    } else {
        device::set_power(ep, on)
    }
}

pub fn identify(ep: &Endpoint, color: Rgb) -> io::Result<()> {
    if is_tuya(ep) {
        tuya::identify(ep, color)
    } else {
        device::identify(ep, color)
    }
}

/// Search every family at once. `bindings` tell the Tuya search where each
/// bulb was last, which is where it looks first.
pub fn discover(
    timeout: Duration,
    pairings: &Pairings,
    bindings: &[DeviceBinding],
) -> io::Result<Vec<DiscoveredDevice>> {
    let hints: HashMap<String, (String, String)> = bindings
        .iter()
        .filter_map(|b| match &b.kind {
            DeviceKind::Tuya { version, .. } => Some((
                b.device_id.strip_prefix(ID_PREFIX)?.to_string(),
                (b.ip.clone(), version.clone()),
            )),
            _ => None,
        })
        .collect();
    let (core, tuya) = std::thread::scope(|scope| {
        let tuya = scope.spawn(|| tuya::discover(timeout, &pairings.tuya, &hints));
        let core = device::discover(timeout, pairings);
        (core, tuya.join().unwrap_or_default())
    });
    let mut found = match core {
        Ok(found) => found,
        // Every LAN family failed; Tuya's own result still stands.
        Err(e) if tuya.is_empty() => return Err(e),
        Err(_) => Vec::new(),
    };
    found.extend(tuya);
    Ok(found)
}
