//! Every loop running, at most one per mode.

use super::{Handle, LiveSettings, Mode};
use lueur_core::device::relink::Link;
use std::sync::{Mutex, MutexGuard};

/// Every loop running, at most one per mode.
#[derive(Default)]
pub struct Effects {
    running: Mutex<Vec<Handle>>,
    /// Held across a whole change (stop, connect, start), so two changes
    /// asked at once cannot both plan from the same lights.
    change: Mutex<()>,
}

impl Effects {
    fn lock(&self) -> MutexGuard<'_, Vec<Handle>> {
        // A loop that ended by itself (no capture, no sound) is forgotten here,
        // once it has settled the bulbs: dropping it then waits for nothing.
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        running.retain(Handle::is_running);
        running
    }

    /// Serialise a change to what runs. Not re-entrant: callers holding it
    /// use the functions that take it as given.
    pub fn change(&self) -> MutexGuard<'_, ()> {
        self.change.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_running(&self) -> bool {
        !self.lock().is_empty()
    }

    /// What runs: each mode, the lights it drives and how many answered.
    pub fn groups(&self) -> Vec<(Mode, Vec<String>, usize)> {
        self.lock()
            .iter()
            .map(|h| (h.mode, h.light_ids.clone(), h.connected_lights))
            .collect()
    }

    pub fn modes(&self) -> Vec<Mode> {
        self.lock().iter().map(|h| h.mode).collect()
    }

    /// The mode driving a light, if any.
    pub fn mode_of(&self, light_id: &str) -> Option<Mode> {
        self.lock()
            .iter()
            .find(|h| h.light_ids.iter().any(|id| id == light_id))
            .map(|h| h.mode)
    }

    /// Every room light some loop drives.
    pub fn driven_ids(&self) -> Vec<String> {
        self.lock()
            .iter()
            .flat_map(|h| h.light_ids.iter().cloned())
            .collect()
    }

    /// Whether the music show waits for its source to come back.
    pub fn waiting_for_sound(&self) -> bool {
        self.lock().iter().any(Handle::waiting_for_sound)
    }

    /// Every driven light whose bulb is not taking frames, and why.
    pub fn away(&self) -> Vec<(String, Link)> {
        self.lock()
            .iter()
            .flat_map(|h| h.away().map(|(id, link)| (id.clone(), link)))
            .collect()
    }

    /// Take the loops `which` picks out of the running set, still running.
    pub fn take(&self, which: impl Fn(&Handle) -> bool) -> Vec<Handle> {
        let mut running = self.lock();
        let (taken, kept) = running.drain(..).partition(|h| which(h));
        *running = kept;
        taken
    }

    /// Add a loop. One already playing its mode is replaced.
    pub fn push(&self, handle: Handle) {
        let mut running = self.lock();
        running.retain(|h| h.mode != handle.mode);
        running.push(handle);
    }

    pub fn update_live(&self, settings: LiveSettings) {
        for h in self.lock().iter() {
            h.update_live(settings.clone());
        }
    }
}
