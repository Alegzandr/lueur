//! Undo and redo for the room plan, after `src/lib/history.svelte.ts`.
//!
//! A step is a snapshot of the room and of which bulb drives which light
//! (removing a light drops its bulb too). Changes made in one go (a drag, a
//! slider, typing a name) settle into a single step: the page closes a step
//! once the plan has been quiet for [`SETTLE`], and never during a drag.

use lueur_core::config::{AppConfig, DeviceBinding};
use lueur_core::geometry::Room;
use std::time::Duration;

/// At most this many steps are kept.
const LIMIT: usize = 100;
/// Quiet time after which a burst of changes becomes a step.
pub const SETTLE: Duration = Duration::from_millis(500);

/// The plan at one step.
#[derive(Debug, Clone)]
pub struct Snap {
    pub room: Room,
    pub bindings: Vec<DeviceBinding>,
}

impl Snap {
    pub fn of(c: &AppConfig) -> Self {
        Snap {
            room: c.room.clone(),
            bindings: c.bindings.clone(),
        }
    }

    /// What a step compares: the room and the pairings, not the bulbs'
    /// addresses, which a background search updates on its own.
    fn same_plan(&self, other: &Snap) -> bool {
        self.room == other.room
            && self.bindings.len() == other.bindings.len()
            && self
                .bindings
                .iter()
                .zip(&other.bindings)
                .all(|(a, b)| a.light_id == b.light_id && a.device_id == b.device_id)
    }
}

#[derive(Default)]
pub struct History {
    past: Vec<Snap>,
    future: Vec<Snap>,
    /// The state the newest step ended on.
    base: Option<Snap>,
    /// The plan moved since `base`, in a step not closed yet.
    dirty: bool,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty() || self.dirty
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty() && !self.dirty
    }

    /// Start from the plan as it is, unless it is where the history left
    /// off (the plan was not touched elsewhere since).
    pub fn track(&mut self, now: &Snap) {
        if self.base.as_ref().is_some_and(|b| b.same_plan(now)) {
            return;
        }
        self.past.clear();
        self.future.clear();
        self.base = Some(now.clone());
        self.dirty = false;
    }

    /// The plan may have changed: true when a step is open, which the page
    /// closes with [`History::commit`] once the plan rests.
    pub fn observe(&mut self, now: &Snap) -> bool {
        self.dirty = self.base.as_ref().is_some_and(|b| !b.same_plan(now));
        self.dirty
    }

    /// Close the open step, if the plan moved since the last one.
    pub fn commit(&mut self, now: &Snap) {
        let Some(base) = self.base.take_if(|b| !b.same_plan(now)) else {
            return;
        };
        self.past.push(base);
        if self.past.len() > LIMIT {
            self.past.remove(0);
        }
        self.future.clear();
        self.base = Some(now.clone());
        self.dirty = false;
    }

    /// The step before, to put back with [`restore`].
    pub fn undo(&mut self, now: &Snap) -> Option<Snap> {
        self.commit(now);
        let s = self.past.pop()?;
        if let Some(base) = self.base.replace(s.clone()) {
            self.future.push(base);
        }
        self.dirty = false;
        Some(s)
    }

    /// The step after, to put back with [`restore`].
    pub fn redo(&mut self, now: &Snap) -> Option<Snap> {
        self.commit(now);
        let s = self.future.pop()?;
        if let Some(base) = self.base.replace(s.clone()) {
            self.past.push(base);
        }
        self.dirty = false;
        Some(s)
    }
}

/// Put a step back, keeping the addresses bulbs are known at now.
pub fn restore(c: &mut AppConfig, s: &Snap) {
    c.room = s.room.clone();
    c.bindings = s
        .bindings
        .iter()
        .map(|b| {
            let mut b = b.clone();
            if let Some(now) = c.bindings.iter().find(|x| x.device_id == b.device_id) {
                b.ip = now.ip.clone();
                b.port = now.port;
                b.kind = now.kind.clone();
            }
            b
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use lueur_core::geometry::Vec2;

    fn moved(c: &mut AppConfig, x: f32) {
        c.room.screen.center = Vec2::new(x, 160.0);
    }

    #[test]
    fn a_burst_of_changes_is_one_step() {
        let mut c = AppConfig::default();
        let mut h = History::default();
        h.track(&Snap::of(&c));
        assert!(!h.can_undo());
        moved(&mut c, 400.0);
        assert!(h.observe(&Snap::of(&c)));
        moved(&mut c, 300.0);
        assert!(h.observe(&Snap::of(&c)));
        assert!(h.can_undo() && !h.can_redo());
        h.commit(&Snap::of(&c));
        let back = h.undo(&Snap::of(&c)).unwrap();
        assert_eq!(back.room.screen.center.x, 500.0);
        restore(&mut c, &back);
        assert!(!h.can_undo() && h.can_redo());
        let again = h.redo(&Snap::of(&c)).unwrap();
        assert_eq!(again.room.screen.center.x, 300.0);
    }

    #[test]
    fn undo_closes_the_open_step_first_and_redo_ends_with_a_new_change() {
        let mut c = AppConfig::default();
        let mut h = History::default();
        h.track(&Snap::of(&c));
        moved(&mut c, 400.0);
        h.observe(&Snap::of(&c));
        // Not committed yet: undo still goes back over it.
        let back = h.undo(&Snap::of(&c)).unwrap();
        restore(&mut c, &back);
        assert!(h.can_redo());
        moved(&mut c, 200.0);
        h.observe(&Snap::of(&c));
        assert!(!h.can_redo());
        h.commit(&Snap::of(&c));
        assert!(h.redo(&Snap::of(&c)).is_none());
    }

    #[test]
    fn a_plan_changed_elsewhere_starts_afresh() {
        let mut c = AppConfig::default();
        let mut h = History::default();
        h.track(&Snap::of(&c));
        moved(&mut c, 400.0);
        h.observe(&Snap::of(&c));
        h.commit(&Snap::of(&c));
        // Back where it left off: the history stays.
        h.track(&Snap::of(&c));
        assert!(h.can_undo());
        moved(&mut c, 100.0);
        h.track(&Snap::of(&c));
        assert!(!h.can_undo());
    }

    #[test]
    fn a_restored_step_keeps_the_addresses_known_now() {
        let mut c = AppConfig::default();
        c.bindings.push(DeviceBinding {
            light_id: "l".into(),
            device_id: "d".into(),
            ip: "192.168.1.2".into(),
            port: 1,
            model: String::new(),
            name: String::new(),
            kind: Default::default(),
        });
        let then = Snap::of(&c);
        c.bindings[0].ip = "192.168.1.9".into();
        restore(&mut c, &then);
        assert_eq!(c.bindings[0].ip, "192.168.1.9");
        // A new address alone is not a step.
        assert!(then.same_plan(&Snap::of(&c)));
    }
}
