//! Opening a stream to every light an effect drives, all at once.

use super::{ConnectedLight, status_changed};
use crate::devices;
use crate::error::{AppError, AppResult};
use lueur_core::config::AppConfig;
use lueur_core::device::LightStream;
use lueur_core::device::lease::{self, Leases};
use lueur_core::device::relink::{Link, OnChange, Opener, Relink};
use lueur_core::geometry::map_zones;
use lueur_core::stage::Fixture;
use std::collections::HashMap;
use std::sync::Arc;

/// What became of one bulb when its effect started.
pub(super) enum Opened {
    Stream(Box<dyn LightStream>),
    /// Busy: another computer drives it, or it said something else does.
    Busy(Link),
    Failed(std::io::Error),
}

/// Open a stream to each of `ids` that has a bulb, in room order. `hosts`
/// counts every light that will be driven once this starts (see
/// [`per_host`](super::per_host)). A bulb another computer drives, or that
/// says something else streams to it, is left alone: its light joins the
/// effect, waiting, and picks up once the bulb is free.
///
/// Returns an error if there are none, or if none of the bulbs accept a
/// stream; individual failures are tolerated.
pub(super) fn connect(
    config: &AppConfig,
    ids: &[String],
    hosts: &HashMap<String, usize>,
    leases: &Arc<Leases>,
) -> AppResult<(Vec<ConnectedLight>, Vec<String>)> {
    // Resolve the lights that have a device binding, in room order. Each
    // one's screen mapping waits for its stream, which says how many zones
    // it plays as.
    let targets: Vec<_> = config
        .room
        .lights
        .iter()
        .filter(|l| ids.contains(&l.id))
        .filter_map(|l| {
            let binding = config.binding_for(&l.id)?;
            let siblings = hosts.get(&binding.ip).copied().unwrap_or(1);
            Some((binding, l, siblings))
        })
        .collect();
    if targets.is_empty() {
        return Err(AppError::new("no_active_lights"));
    }

    // How each bulb is opened, now and again whenever it drops.
    let openers: Vec<Opener> = targets
        .iter()
        .map(|(binding, _, siblings)| {
            let endpoint = binding.endpoint();
            let siblings = *siblings;
            Arc::new(move |polite| devices::open_stream(&endpoint, siblings, polite)) as Opener
        })
        .collect();
    let gates: Vec<_> = targets
        .iter()
        .map(|(binding, _, _)| leases.gate(&binding.device_id))
        .collect();
    // Open every bulb at once: an unplugged one costs its whole timeout, and
    // waiting on each in turn would make a few of them feel like a hang.
    let opened: Vec<Opened> = std::thread::scope(|scope| {
        let handles: Vec<_> = openers
            .iter()
            .zip(&gates)
            .map(|(open, gate)| {
                scope.spawn(move || {
                    if !gate.may_drive() {
                        return Opened::Busy(Link::Held);
                    }
                    match open(true) {
                        Ok(stream) => Opened::Stream(stream),
                        Err(e) if lease::is_taken(&e) => Opened::Busy(Link::Taken),
                        Err(e) => Opened::Failed(e),
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Opened::Failed(std::io::Error::other("bulb thread panicked"))
                })
            })
            .collect()
    });
    let mut connected = Vec::new();
    let mut light_ids = Vec::new();
    let mut failed = Vec::new();
    let mut details = Vec::new();
    let changed: OnChange = Arc::new(status_changed);
    for ((((binding, light, siblings), result), open), gate) in
        targets.into_iter().zip(opened).zip(openers).zip(gates)
    {
        let link = match result {
            Opened::Stream(conn) => Relink::new(conn, open),
            Opened::Busy(link) => {
                log::info!(light = light.id.as_str(); "a light's bulb is busy, it waits for it");
                let endpoint = binding.endpoint();
                let interval = devices::interval_of(&endpoint, siblings);
                Relink::idle(open, interval, link).answering(devices::latency_of(&endpoint))
            }
            Opened::Failed(e) => {
                gate.release();
                failed.push(light.name.clone());
                details.push(format!("{} ({}): {e}", light.name, binding.ip));
                continue;
            }
        };
        let fixture = Fixture::place(&config.room.screen, light, link.interval(), link.latency());
        connected.push(ConnectedLight {
            id: light.id.clone(),
            mappings: map_zones(&config.room.screen, light, link.zones(), link.upright()),
            fixtures: fixture.zones(link.zones()),
            link: link
                .gated(gate)
                .named(light.id.as_str())
                .on_change(Arc::clone(&changed)),
        });
        light_ids.push(light.id.clone());
    }
    if connected.is_empty() {
        return Err(AppError::new("music_mode_failed")
            .subject(failed.join(", "))
            .detail(details.join("; ")));
    }
    Ok((connected, light_ids))
}
