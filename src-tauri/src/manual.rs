//! Driving lights by hand: a brightness slider, a white, a colour, a scene.
//!
//! Each light being adjusted gets the same per-frame stream the ambilight
//! uses, held open while the user is busy with it and closed a few seconds
//! after the last change. A worker re-sends every light's latest wish on a
//! short tick: streams skip what the bulb already shows and pace themselves
//! to what it tolerates, so a slider dragged across its whole range costs a
//! handful of commands and the value it stops on always lands. A Yeelight
//! goes through music mode for the same reason: its normal control channel
//! allows one command a second.
//!
//! What the hand asks for goes through the photosensitivity protection like
//! every effect: moods or colours switched faster than three times a second
//! are eased rather than flashed.

use crate::devices;
use lueur_core::color::BulbOutput;
use lueur_core::device::{Endpoint, LightStream};
use lueur_core::safety::FlashGuard;
use lueur_core::yeelight::Effect;
use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the worker re-sends what each light should show.
const TICK: Duration = Duration::from_millis(40);
/// A light left alone this long has its stream closed.
const IDLE: Duration = Duration::from_secs(4);
/// The bulb's own fade between two wishes: long enough to look smooth, short
/// enough that a dragged slider does not lag behind the hand.
const FADE_MS: u32 = 150;

struct Session {
    stream: Box<dyn LightStream>,
    target: BulbOutput,
    touched: Instant,
    guard: Guarded,
}

/// One light's wishes, held to the flash budget (see [`FlashGuard`]).
struct Guarded {
    guard: FlashGuard,
    /// When the last output went out.
    last: Option<Instant>,
}

impl Guarded {
    fn new() -> Self {
        Guarded {
            guard: FlashGuard::new(1),
            last: None,
        }
    }

    /// What to send at `now` for `target`; `on` is the user's setting.
    fn next(&mut self, target: BulbOutput, now: Instant, on: bool) -> BulbOutput {
        let dt = self.last.map_or(0.0, |t| (now - t).as_secs_f32());
        self.last = Some(now);
        let mut out = [target];
        self.guard.apply(&mut out, dt, on);
        out[0]
    }
}

enum Slot {
    /// The stream is being opened; the newest wish waits here.
    Opening(BulbOutput),
    Open(Session),
}

/// Every light currently held by hand, keyed by room light id.
pub struct Manual {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    /// Whether the worker is running. It ends once no light is held.
    worker: Arc<Mutex<bool>>,
    /// The photosensitivity protection, as the user last set it.
    protect: Arc<AtomicBool>,
}

impl Default for Manual {
    fn default() -> Self {
        Manual {
            slots: Arc::default(),
            worker: Arc::default(),
            protect: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl Manual {
    /// Follow the photosensitivity protection setting, live.
    pub fn protect(&self, on: bool) {
        self.protect.store(on, Ordering::SeqCst);
    }

    /// Show `output` on the light, opening its stream if needed. Blocks while
    /// the bulb is reached (up to a few seconds for one that is unplugged).
    pub fn set(&self, light_id: &str, ep: &Endpoint, output: BulbOutput) -> io::Result<()> {
        {
            let mut slots = self.lock();
            match slots.get_mut(light_id) {
                Some(Slot::Open(s)) => {
                    s.target = output;
                    s.touched = Instant::now();
                    return Ok(());
                }
                Some(Slot::Opening(wish)) => {
                    *wish = output;
                    return Ok(());
                }
                None => {
                    slots.insert(light_id.to_string(), Slot::Opening(output));
                }
            }
        }

        let opened = devices::set_power(ep, true).and_then(|()| devices::open_stream(ep, 1, false));
        let mut slots = self.lock();
        let stream = match opened {
            Ok(stream) => stream,
            Err(e) => {
                slots.remove(light_id);
                return Err(e);
            }
        };
        // Switched off or taken by the ambilight while we were connecting:
        // that later decision wins, and the stream is dropped unused.
        let Some(Slot::Opening(target)) = slots.remove(light_id) else {
            return Ok(());
        };
        slots.insert(
            light_id.to_string(),
            Slot::Open(Session {
                stream,
                target,
                touched: Instant::now(),
                guard: Guarded::new(),
            }),
        );
        drop(slots);
        self.ensure_worker();
        Ok(())
    }

    /// Stop holding a light, so a power command or the ambilight can have it.
    pub fn release(&self, light_id: &str) {
        self.lock().remove(light_id);
    }

    /// Stop holding every light.
    pub fn release_all(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        // A panic mid-push leaves nothing half-written worth refusing over.
        self.slots.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn ensure_worker(&self) {
        let mut running = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if *running {
            return;
        }
        let slots = Arc::clone(&self.slots);
        let worker = Arc::clone(&self.worker);
        let protect = Arc::clone(&self.protect);
        let spawned = std::thread::Builder::new()
            .name("lueur-manual".into())
            .spawn(move || run(&slots, &worker, &protect));
        *running = spawned.is_ok();
    }
}

fn run(slots: &Mutex<HashMap<String, Slot>>, worker: &Mutex<bool>, protect: &AtomicBool) {
    loop {
        std::thread::sleep(TICK);
        let on = protect.load(Ordering::SeqCst);
        let mut slots = slots.lock().unwrap_or_else(|p| p.into_inner());
        slots.retain(|_, slot| match slot {
            Slot::Opening(_) => true,
            Slot::Open(s) => {
                // A bulb that stops answering is let go; the next change
                // opens a fresh stream.
                let output = s.guard.next(s.target, Instant::now(), on);
                s.touched.elapsed() < IDLE && s.stream.push(output, Effect::Smooth, FADE_MS).is_ok()
            }
        });
        if slots.is_empty() {
            // Still holding `slots`, so no light can be added unseen: the
            // next one finds the flag down and starts a new worker.
            *worker.lock().unwrap_or_else(|p| p.into_inner()) = false;
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mood(bright: u8) -> BulbOutput {
        BulbOutput::White {
            kelvin: 4000,
            bright,
        }
    }

    /// Daylight and Night swapped on every tick, `on` or not: what each
    /// tick sends, as brightness.
    fn toggled(on: bool) -> Vec<u8> {
        let mut held = Guarded::new();
        let start = Instant::now();
        (0..100u32)
            .map(|k| {
                let wish = if k % 2 == 0 { mood(100) } else { mood(8) };
                held.next(wish, start + TICK * k, on).bright()
            })
            .collect()
    }

    #[test]
    fn moods_switched_fast_are_eased() {
        // Unprotected, every switch lands; protected, the swings soon stop
        // reaching all the way: no more strobing between the two.
        let free = toggled(false);
        assert!(free.windows(2).all(|w| w[0].abs_diff(w[1]) == 92));
        let held = toggled(true);
        let late = &held[50..];
        let swing = late.iter().max().unwrap() - late.iter().min().unwrap();
        assert!(swing < 92, "still strobing: {late:?}");
        assert!(
            late.windows(2).all(|w| w[0].abs_diff(w[1]) < 15),
            "{late:?}"
        );
    }

    #[test]
    fn a_single_change_lands_at_once() {
        let mut held = Guarded::new();
        let start = Instant::now();
        assert_eq!(held.next(mood(8), start, true), mood(8));
        assert_eq!(held.next(mood(100), start + TICK, true), mood(100));
    }
}
