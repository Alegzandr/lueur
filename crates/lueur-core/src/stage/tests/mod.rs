mod following;
mod listening;
mod playing;

use super::analyzer::CLOCK_SPAN;
use super::dsp::{Biquad, Pass};
use super::fixture::*;
use super::pattern::*;
use super::track::*;
use super::*;
use crate::color::full_chroma;
use crate::safety::FlashGuard;
use crate::show::{PARTY, SOFT};
use std::f32::consts::TAU;
use std::time::Duration;

fn tone(rate: u32, freq: f32, amp: f32, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    (0..n)
        .map(|i| amp * (TAU * freq * i as f32 / rate as f32).sin())
        .collect()
}

/// White-ish noise from a fixed seed: a snare or a hat once filtered.
fn hiss(amp: f32, n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed.max(1);
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            amp * ((x >> 8) as f32 / (1u32 << 23) as f32 - 1.0)
        })
        .collect()
}

/// Four to the floor over a quiet pad, `amp` loud, at `bpm`, one frame
/// (25 ms) at a time. Each frame's levels.
fn four_on_the_floor(a: &mut Analyzer, rate: u32, bpm: f32, amp: f32, secs: f32) -> Vec<Levels> {
    let frame = (rate as f32 * 0.025) as usize;
    let beat = 60.0 / bpm;
    let total = (secs * rate as f32) as usize;
    let mut out = Vec::new();
    let mut chunk = Vec::with_capacity(frame);
    for i in 0..total {
        let t = i as f32 / rate as f32;
        let into = t % beat;
        let kick = if into < 0.08 { 0.7 } else { 0.03 };
        chunk.push(amp * kick * (TAU * 55.0 * t).sin());
        if chunk.len() == frame {
            out.push(a.feed(&chunk));
            chunk.clear();
        }
    }
    out
}

fn beat(strength: f32) -> Levels {
    Levels {
        energy: 0.8,
        bass: 0.8,
        beat: Some(strength),
        kick: true,
        ..Levels::default()
    }
}

fn between() -> Levels {
    Levels {
        energy: 0.8,
        bass: 0.6,
        ..Levels::default()
    }
}

fn rgb(o: BulbOutput) -> Rgb {
    match o {
        BulbOutput::Color { rgb, .. } => rgb,
        other => panic!("expected a colour, got {other:?}"),
    }
}

/// Play a groove: a beat every 20 frames (half a second).
fn groove(stage: &mut Stage, s: &ShowSettings, beats: usize) -> Vec<Vec<BulbOutput>> {
    let mut frames = Vec::new();
    for k in 0..beats * 20 {
        let l = if k % 20 == 0 { beat(1.0) } else { between() };
        frames.push(stage.frame(s, l, 0.025));
    }
    frames
}

/// Whether a light shows white, or a colour pale enough to pass for it.
fn whitish(o: &BulbOutput) -> bool {
    match o {
        BulbOutput::White { bright, .. } => *bright >= 20,
        BulbOutput::Color { rgb, bright } => *bright >= 20 && crate::color::saturation(*rgb) < 0.2,
    }
}

/// The most white flashes the room shows in any one second.
///
/// Each light's white (its brightness while it shows white, else
/// nothing) turns when it swings a tenth of the way back, the darker
/// side under 0.8. Lights turning the same way within 0.1 s are one
/// change of the room; a flash is a rise and a fall.
fn room_white_flashes(frames: &[Vec<BulbOutput>], dt: f32) -> usize {
    let mut events: Vec<(f32, i8)> = Vec::new();
    for i in 0..frames[0].len() {
        let level = |f: &Vec<BulbOutput>| {
            if whitish(&f[i]) {
                f32::from(f[i].bright()) / 100.0
            } else {
                0.0
            }
        };
        let (mut ext, mut dir) = (level(&frames[0]), 0i8);
        for (k, f) in frames.iter().enumerate().skip(1) {
            let v = level(f);
            if v - ext >= 0.1 && dir <= 0 && ext < 0.8 {
                (ext, dir) = (v, 1);
                events.push((k as f32 * dt, 1));
            } else if ext - v >= 0.1 && dir >= 0 && v < 0.8 {
                (ext, dir) = (v, -1);
                events.push((k as f32 * dt, -1));
            } else if (dir > 0 && v > ext) || (dir < 0 && v < ext) {
                ext = v;
            }
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut room: Vec<(f32, i8)> = Vec::new();
    for (t, d) in events {
        if !room.iter().any(|&(u, e)| e == d && t - u < 0.1 - 1e-4) {
            room.push((t, d));
        }
    }
    room.iter()
        .map(|&(t, _)| {
            let within = |d: i8| {
                room.iter()
                    .filter(|&&(u, e)| e == d && u <= t && t - u < 1.0)
                    .count()
            };
            within(1).max(within(-1))
        })
        .max()
        .unwrap_or(0)
}

/// A stage that has heard the music at `energy` for a good while.
fn settle(stage: &mut Stage, energy: f32) {
    (stage.short, stage.recent, stage.long) = (energy, energy, energy);
}

/// A band without drums at 100 beats a minute, one 25 ms frame at a
/// time: a strum on the one and the three, a snare (a rim, a clap) on
/// the two and the four. Each frame's levels.
fn unplugged(a: &mut Analyzer, rate: u32, secs: f32) -> Vec<Levels> {
    let beat = 0.6;
    let frame = (rate as f32 * 0.025) as usize;
    let mut snare_band = Biquad::new(Pass::Band, rate as f32, 2200.0, 0.7);
    let noise = hiss(0.5, (secs * rate as f32) as usize, 3);
    let mut out = Vec::new();
    let mut chunk = Vec::with_capacity(frame);
    for (i, &hush) in noise.iter().enumerate() {
        let t = i as f32 / rate as f32;
        let n = (t / beat) as u32;
        let into = t - n as f32 * beat;
        let ring = (-into / 0.15).exp();
        let s = if n.is_multiple_of(2) {
            // A chord of E, well above the kick's band and under the
            // snare's.
            let chord: f32 = [330.0, 415.0, 494.0, 659.0]
                .iter()
                .map(|f| (TAU * f * t).sin())
                .sum();
            0.15 * ring * chord
        } else {
            ring * snare_band.run(hush)
        };
        // A bass drone holds the bottom steady, as an organ or a
        // synth would.
        chunk.push(s + 0.1 * (TAU * 55.0 * t).sin());
        if chunk.len() == frame {
            out.push(a.feed(&chunk));
            chunk.clear();
        }
    }
    out
}

/// A stage of three in a groove, on `pattern`.
fn playing(pattern: Pattern) -> Stage {
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    stage.look = pattern;
    stage
}

/// Play `beats` beats `period` apart at `sound`, one 25 ms frame at a
/// time with the clock sure of it; `hits(j)` says whether beat `j` comes
/// with a kick, and whether a snare lands on it.
fn drive(
    stage: &mut Stage,
    s: &ShowSettings,
    period: f32,
    beats: u32,
    sound: Levels,
    hits: impl Fn(u32) -> (bool, bool),
) -> Vec<Vec<BulbOutput>> {
    let per = (period / 0.025).round() as u32;
    let mut frames = Vec::new();
    for k in 0..beats * per {
        let mut l = Levels {
            tempo: Some(period),
            ..sound
        };
        if k % per == 0 {
            let (kick, snare) = hits(k / per);
            l.beat = Some(1.0);
            l.kick = kick;
            l.snare = snare.then_some(0.9);
        }
        frames.push(stage.frame(s, l, 0.025));
    }
    frames
}
