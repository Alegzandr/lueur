//! The light shows' loop, and the music show's ears.

use super::ambilight::pace;
use super::{ConnectedLight, LiveSettings, on_walls, push_zoned, spans};
use crate::audio::AudioTap;
use lueur_core::config::AudioSource;
use lueur_core::safety::FlashGuard;
use lueur_core::show::{Show, ShowKind};
use lueur_core::stage::{Analyzer, Fixture, Levels};
use lueur_core::yeelight::Effect;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The soonest a show computes a frame for something due: sooner, and a
/// wake-up costs more than it gains.
const SOONEST: Duration = Duration::from_millis(2);

pub(super) fn run_show(
    kind: ShowKind,
    hearing: Option<Arc<AtomicBool>>,
    lights: &mut [ConnectedLight],
    live: &Mutex<LiveSettings>,
    stop: &AtomicBool,
) {
    let fixtures: Vec<Fixture> = lights
        .iter()
        .flat_map(|l| l.fixtures.iter().copied())
        .collect();
    let spans = spans(lights);
    // The gap each light's bulb keeps between two commands.
    let intervals: Vec<Duration> = lights.iter().map(|l| l.link.interval()).collect();
    let mut show = Show::new(kind, &fixtures);
    let mut guard = FlashGuard::show(fixtures.len());
    let mut ears = match hearing {
        Some(hearing) => {
            let Ok(source) = live.lock().map(|g| g.audio.clone()) else {
                return;
            };
            Some(Listener::new(source, hearing))
        }
        None => None,
    };
    // Most outputs run at 48 kHz; a tap that runs at another rate gets an
    // analyser of its own when it opens.
    let mut sample_rate = 48_000;
    let mut analyzer = Analyzer::new(sample_rate);
    let frame = Duration::from_millis(kind.frame_ms());
    // Each frame fades into the next over the time it has, so the bulbs glide
    // rather than step; a light a beat just struck is the exception, sent
    // sharp.
    let glide = pace(Effect::Smooth, frame.as_millis() as u32, frame);
    // Room for the tap's whole second, so the audio thread never grows it.
    let mut samples = Vec::with_capacity(if ears.is_some() {
        sample_rate as usize
    } else {
        0
    });

    let mut last = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        // The next frame comes at its time, or sooner for a beat due before
        // it: the beat then goes out when it should, not with the frame.
        let wait = show.next_due().map_or(frame, |due| {
            frame.min(Duration::from_secs_f32(due).max(SOONEST))
        });
        std::thread::sleep(wait.saturating_sub(last.elapsed()));
        // A stall (a sleeping laptop, a busy machine) must not fast-forward
        // the show.
        let dt = last.elapsed().as_secs_f32().min(0.25);
        last = Instant::now();
        let Ok((settings, wall, safety)) = live.lock().map(|g| {
            if let Some(ears) = &mut ears {
                ears.follow(&g.audio);
            }
            (g.show, g.wall, g.safety)
        }) else {
            break;
        };
        let levels = match &mut ears {
            Some(ears) => match ears.take_into(&mut samples) {
                Some(rate) => {
                    // A source opened again may run at another rate.
                    if rate != sample_rate {
                        sample_rate = rate;
                        analyzer = Analyzer::new(rate);
                    }
                    analyzer.set_lead(show.lead());
                    analyzer.feed_after(&samples, dt)
                }
                None => Levels::default(),
            },
            None => Levels::default(),
        };
        let mut outputs = show.frame(&settings, dt, levels);
        let asked = on_walls(&mut outputs, wall);
        guard.apply(&mut outputs, dt, safety.flash_guard);
        let sharp = show.sharp();
        // A light is struck when any of its zones is; a hit the guard held
        // back glides in like the rest.
        let struck = |i: usize| {
            spans[i]
                .clone()
                .any(|z| sharp.get(z).copied().unwrap_or(false) && !guard.held(z))
        };
        let pace = |i: usize| {
            if struck(i) {
                (Effect::Sudden, 0)
            } else {
                glide
            }
        };
        // A bulb that takes a command only now and then keeps its next one
        // for a beat due before it could take another.
        let hold = |i: usize| {
            let gap = intervals[i].as_secs_f32();
            gap > 0.0
                && !struck(i)
                && spans[i]
                    .clone()
                    .filter_map(|z| show.due(z))
                    .any(|due| due < gap)
        };
        push_zoned(lights, &outputs, &asked, &spans, pace, hold);
    }
}

/// The music show's ears: a tap on the source the settings name, opened again
/// when they name another one, and when it stops hearing its own (a device
/// unplugged, the application closed), until that comes back.
struct Listener {
    source: AudioSource,
    tap: Option<AudioTap>,
    /// When to try opening the source again, while it has no tap.
    retry: Instant,
    /// Whether it has a tap, for the window to tell.
    hearing: Arc<AtomicBool>,
}

/// How long a source that could not be opened waits before the next try.
const LISTEN_RETRY: Duration = Duration::from_secs(2);

impl Listener {
    /// Opened on the first [`Listener::take_into`], on the show's thread.
    fn new(source: AudioSource, hearing: Arc<AtomicBool>) -> Self {
        Listener {
            source,
            tap: None,
            retry: Instant::now(),
            hearing,
        }
    }

    /// Listen to `source` from now on, if it is not already the one.
    fn follow(&mut self, source: &AudioSource) {
        if *source != self.source {
            self.source = source.clone();
            self.tap = None;
            self.retry = Instant::now();
        }
    }

    /// The sound captured since the last call, into `out`, and its sample
    /// rate. `None` while the source cannot be heard.
    fn take_into(&mut self, out: &mut Vec<f32>) -> Option<u32> {
        if self.tap.as_mut().is_some_and(AudioTap::lost) {
            log::info!("audio source lost, listening again");
            self.tap = None;
            self.retry = Instant::now() + LISTEN_RETRY;
        }
        if self.tap.is_none() && Instant::now() >= self.retry {
            match AudioTap::open(&self.source) {
                Ok(tap) => self.tap = Some(tap),
                Err(e) => {
                    log::debug!("audio unavailable: {e}");
                    self.retry = Instant::now() + LISTEN_RETRY;
                }
            }
        }
        self.hearing.store(self.tap.is_some(), Ordering::Relaxed);
        let tap = self.tap.as_mut()?;
        tap.take_into(out);
        Some(tap.sample_rate)
    }
}
