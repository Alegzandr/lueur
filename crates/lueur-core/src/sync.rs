//! Keeping lights that answer at different speeds together.
//!
//! A Yeelight shows a command in a few hundredths of a second; a Hue lamp
//! behind its bridge takes several times that. Sent at once, the quick one
//! would light first and the room would ripple where it should strike. So
//! each light is given its wait: the quick ones hold their frames back until
//! the slowest catches up ([`Lag`]). The music goes further and sends its
//! beats early by that much (see [`crate::stage::Stage::lead`]), so all of
//! them land on the kick; the ambilight cannot see the picture coming, and
//! only keeps its lights together.
//!
//! Lights too slow to strike (a Hue lamp over REST, which only glides) are
//! not waited for: holding everyone else back for them would cost the whole
//! room its snap.

use crate::color::BulbOutput;
use crate::stage::{Fixture, MOST_LEAD};
use std::collections::VecDeque;

/// Seconds early a beat goes out for `fixtures`, and how long each then
/// waits on it, when the sound reaches the ear `delay` seconds after it is
/// heard here (0 for the ambilight, whose lights only wait on each other).
///
/// The lead is the longest any quick light takes to show a command (or any
/// light at all, when none is quick), less the delay; never below nothing,
/// never beyond [`MOST_LEAD`]. Each light then waits what its own takes
/// short of that, plus whatever the delay asks beyond it.
pub fn timing(fixtures: &[Fixture], delay: f32) -> (f32, Vec<f32>) {
    let leads: Vec<f32> = fixtures
        .iter()
        .map(|f| f.latency.as_secs_f32() - delay)
        .collect();
    let any_quick = fixtures.iter().any(Fixture::quick);
    let lead = fixtures
        .iter()
        .zip(&leads)
        .filter(|(f, _)| f.quick() || !any_quick)
        .map(|(_, &l)| l)
        .fold(0.0, f32::max)
        .min(MOST_LEAD);
    let waits = leads.iter().map(|l| (lead - l).max(0.0)).collect();
    (lead, waits)
}

/// What a light shows, held back until its time (see [`Lag`]).
#[derive(Debug, Clone, Copy)]
struct Held {
    /// When it was made, seconds since the lights began.
    at: f64,
    output: BulbOutput,
    sharp: bool,
    /// Whether the light has been given it.
    sent: bool,
}

/// Holds each light's frames back by its own wait (see [`timing`]).
#[derive(Debug, Clone, Default)]
pub struct Lag {
    t: f64,
    /// Each light's frames still to come, behind the one it shows.
    queues: Vec<VecDeque<Held>>,
}

impl Lag {
    /// The frame each light shows now, given this frame's `outputs`, made
    /// `dt` seconds after the last, and each light's wait, seconds. `sharp`
    /// comes in as this frame's and leaves as whether what each light now
    /// shows (or a frame skipped on the way to it) was struck sharp.
    pub fn run(
        &mut self,
        dt: f32,
        outputs: Vec<BulbOutput>,
        sharp: &mut [bool],
        waits: &[f32],
    ) -> Vec<BulbOutput> {
        self.t += f64::from(dt.max(0.0));
        self.queues.resize_with(outputs.len(), VecDeque::new);
        let t = self.t;
        outputs
            .into_iter()
            .enumerate()
            .map(|(i, output)| {
                let q = &mut self.queues[i];
                q.push_back(Held {
                    at: t,
                    output,
                    sharp: sharp.get(i).copied().unwrap_or(false),
                    sent: false,
                });
                // A hair of slack, so a wait of nothing never holds a frame.
                let due = t - f64::from(waits.get(i).copied().unwrap_or(0.0)) + 1e-6;
                let mut struck = false;
                while q.len() > 1 && q[1].at <= due {
                    if let Some(gone) = q.pop_front() {
                        struck |= gone.sharp && !gone.sent;
                    }
                }
                let shown = &mut q[0];
                struck |= shown.sharp && !shown.sent;
                shown.sent = true;
                if let Some(s) = sharp.get_mut(i) {
                    *s = struck;
                }
                shown.output
            })
            .collect()
    }

    /// Seconds until light `i`, waiting `wait`, is due a sharp frame it
    /// holds.
    pub fn next_sharp(&self, i: usize, wait: f32) -> Option<f32> {
        let held = self.queues.get(i)?.iter().skip(1).find(|h| h.sharp)?;
        Some((held.at + f64::from(wait) - self.t).max(0.0) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;
    use std::time::Duration;

    fn fixture(latency_ms: u64, interval_ms: u64) -> Fixture {
        Fixture {
            latency: Duration::from_millis(latency_ms),
            interval: Duration::from_millis(interval_ms),
            ..Fixture::row(1)[0]
        }
    }

    #[test]
    fn the_quick_lights_wait_for_the_slowest_of_them() {
        let (lead, waits) = timing(&[fixture(45, 0), fixture(70, 40)], 0.0);
        assert!((lead - 0.07).abs() < 1e-6);
        assert!(
            (waits[0] - 0.025).abs() < 1e-6 && waits[1] == 0.0,
            "{waits:?}"
        );
        // A lamp that only glides is not waited for.
        let (lead, waits) = timing(&[fixture(45, 0), fixture(120, 200)], 0.0);
        assert!((lead - 0.045).abs() < 1e-6);
        assert_eq!(waits, vec![0.0, 0.0]);
        // Unless nothing is quicker.
        assert!((timing(&[fixture(120, 200)], 0.0).0 - 0.12).abs() < 1e-6);
    }

    #[test]
    fn a_late_sound_holds_every_light_back() {
        // Bluetooth: the sound reaches the ear a fifth of a second late, so
        // nothing goes early and every light waits.
        let (lead, waits) = timing(&[fixture(45, 0), fixture(70, 40)], 0.2);
        assert_eq!(lead, 0.0);
        assert!((waits[0] - 0.155).abs() < 1e-6 && (waits[1] - 0.13).abs() < 1e-6);
        // No further ahead than the clock can see.
        assert_eq!(timing(&[fixture(500, 0)], 0.0).0, MOST_LEAD);
    }

    #[test]
    fn a_held_frame_comes_out_on_time_and_sharp() {
        let mut lag = Lag::default();
        let dark = BulbOutput::Color {
            rgb: Rgb::new(255, 0, 0),
            bright: 1,
        };
        let lit = BulbOutput::Color {
            rgb: Rgb::new(255, 0, 0),
            bright: 100,
        };
        let waits = [0.05, 0.0];
        let mut sharp = [false, false];
        lag.run(0.01, vec![dark, dark], &mut sharp, &waits);
        let mut sharp = [true, true];
        let out = lag.run(0.01, vec![lit, lit], &mut sharp, &waits);
        assert_eq!(out, vec![dark, lit]);
        assert_eq!(sharp, [false, true]);
        assert!(
            lag.next_sharp(0, waits[0])
                .is_some_and(|d| (d - 0.05).abs() < 1e-3)
        );
        let mut shown_at = None;
        for k in 1..=8 {
            let mut sharp = [false, false];
            let out = lag.run(0.01, vec![dark, dark], &mut sharp, &waits);
            if out[0] == lit {
                shown_at = Some(k);
                assert!(sharp[0], "sent sharp when its time comes");
            }
        }
        assert_eq!(shown_at, Some(5));
    }
}
