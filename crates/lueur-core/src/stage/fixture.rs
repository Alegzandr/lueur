//! The lights as the stage sees them, and the part each plays.

use super::LEAD;
use crate::geometry::{Light, Screen};
use std::time::Duration;

/// Commands this close together or closer make a bulb quick enough to punch
/// the beat; slower ones wash the room instead.
const QUICK: Duration = Duration::from_millis(60);

/// Further in front of the screen than this, in screen half-widths, a light
/// is lighting the room rather than the stage: it washes.
const FAR: f32 = 2.5;

/// A light as the stage sees it: where it stands, and how fast it answers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fixture {
    /// Across the room as the viewer sees it, in screen half-widths: -1 is
    /// the screen's left end, 1 its right.
    pub x: f32,
    /// How far in front of the screen, in screen half-widths; negative is
    /// behind it.
    pub depth: f32,
    /// 0 on the floor, 1 at the ceiling.
    pub height: f32,
    /// The shortest gap the bulb takes between two commands.
    pub interval: Duration,
    /// How long a command takes to show: the trip to the bulb and its LEDs'
    /// own rise.
    pub latency: Duration,
}

/// How far either side of where it stands a light with several zones
/// spreads them, in screen half-widths: a strip or a few panels, a stretch
/// of wall rather than the whole room.
pub(super) const ZONE_SPAN: f32 = 0.5;

impl Fixture {
    /// Where `light` stands, seen from the seat facing `screen`.
    pub fn place(screen: &Screen, light: &Light, interval: Duration, latency: Duration) -> Self {
        let half = (screen.width * 0.5).max(f32::EPSILON);
        let rel = light.pos - screen.center;
        Fixture {
            x: rel.dot(screen.right_axis()) / half,
            depth: rel.dot(screen.facing()) / half,
            height: light.height.clamp(0.0, 1.0),
            interval,
            latency,
        }
    }

    /// The `n` zones of a light that colours several parts of itself (a
    /// strip's segments, a layout's panels), left to right, spread across
    /// the wall around where it stands: on the stage each is a light of its
    /// own, so a chase runs along the strip.
    pub fn zones(self, n: usize) -> Vec<Fixture> {
        if n <= 1 {
            return vec![self];
        }
        (0..n)
            .map(|i| Fixture {
                x: self.x + ZONE_SPAN * (2.0 * i as f32 / (n - 1) as f32 - 1.0),
                ..self
            })
            .collect()
    }

    /// `n` quick lights in a row behind the screen, left to right: what a
    /// room with no plan looks like.
    pub fn row(n: usize) -> Vec<Fixture> {
        (0..n)
            .map(|i| Fixture {
                x: if n > 1 {
                    -1.0 + 2.0 * i as f32 / (n - 1) as f32
                } else {
                    0.0
                },
                depth: 0.0,
                height: 0.5,
                interval: Duration::ZERO,
                latency: Duration::from_secs_f32(LEAD),
            })
            .collect()
    }

    /// Whether the bulb answers fast enough to strike the beat.
    pub fn quick(&self) -> bool {
        self.interval <= QUICK
    }
}

/// What a light plays on the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    /// Punches the beat, in patterns that move across the room.
    Punch,
    /// Bathes the room in the harmony's colour, swelling with the bass.
    Wash,
    /// Answers the snare and shimmers with the hi-hats.
    Accent,
}

/// Give every light its part.
///
/// The quick lights by the screen are the stage: they punch. Slow ones (a
/// Hue bridge cannot keep up with a beat) and those far out in the room
/// wash. With three punching lights or more, one at the ceiling takes the
/// snare instead, and with four or more the highest does: enough are left
/// to make the patterns travel. There is always someone on the beat.
pub(super) fn cast(fixtures: &[Fixture]) -> Vec<Part> {
    let mut parts: Vec<Part> = fixtures
        .iter()
        .map(|f| {
            if f.quick() && f.depth <= FAR {
                Part::Punch
            } else {
                Part::Wash
            }
        })
        .collect();
    if !parts.contains(&Part::Punch) {
        // The quickest takes the beat, the nearest of those.
        let best = (0..fixtures.len()).min_by(|&a, &b| {
            let (fa, fb) = (&fixtures[a], &fixtures[b]);
            fa.interval
                .cmp(&fb.interval)
                .then(fa.depth.total_cmp(&fb.depth))
        });
        if let Some(i) = best {
            parts[i] = Part::Punch;
        }
    }
    let punches: Vec<usize> = (0..fixtures.len())
        .filter(|&i| parts[i] == Part::Punch)
        .collect();
    // The highest, the one nearest the middle if they are level.
    let highest = punches.iter().copied().max_by(|&a, &b| {
        let (fa, fb) = (&fixtures[a], &fixtures[b]);
        fa.height
            .total_cmp(&fb.height)
            .then(fb.x.abs().total_cmp(&fa.x.abs()))
    });
    if let Some(i) = highest {
        let up_high = punches.len() >= 3 && fixtures[i].height >= 0.8;
        if up_high || punches.len() >= 4 {
            parts[i] = Part::Accent;
        }
    }
    parts
}
