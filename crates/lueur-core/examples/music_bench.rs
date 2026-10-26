//! Plays a track through the music show and reports where it goes quiet.
//!
//! `ffmpeg -i track.mp3 -ac 1 -ar 48000 -f f32le track.raw`, then
//! `cargo run --release --example music_bench -- track.raw [lights]`.

use lueur_core::color::BulbOutput;
use lueur_core::show::{Show, ShowKind, ShowSettings};
use lueur_core::stage::{Analyzer, Fixture};

fn level(o: &BulbOutput) -> f32 {
    match *o {
        BulbOutput::White { bright, .. } => f32::from(bright) / 100.0,
        BulbOutput::Color { rgb, bright } => {
            f32::from(bright) / 100.0 * f32::from(rgb.r.max(rgb.g).max(rgb.b)) / 255.0
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read");
    let lights: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let verbose = args.iter().any(|a| a == "-v");
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let rate = 48_000;
    let fixtures = Fixture::row(lights);
    let mut show = Show::new(ShowKind::Music, &fixtures);
    let mut analyzer = Analyzer::new(rate);
    let settings = ShowSettings::default();
    let chunk = rate as usize * 25 / 1000;
    let dt = 0.025;
    let mut t = 0.0;
    let mut prev = vec![0.0f32; lights];
    // Per second: sections seen, kicks, beats, the biggest jump any light made.
    let mut sec = 0usize;
    let (mut kicks, mut beats, mut jump, mut energy, mut locked) = (0, 0, 0.0f32, 0.0f32, 0);
    let mut lo = vec![1.0f32; lights];
    let mut hi = vec![0.0f32; lights];
    let mut flat = 0usize;
    let mut bpm = 0.0f32;
    let mut sections = String::new();
    let mut dead = 0usize;
    let mut loud = 0usize;
    for c in samples.chunks(chunk) {
        analyzer.set_lead(show.lead());
        let levels = analyzer.feed_after(c, dt);
        let out = show.frame(&settings, dt, levels);
        for (i, (p, o)) in prev.iter_mut().zip(&out).enumerate() {
            let l = level(o);
            lo[i] = lo[i].min(l);
            hi[i] = hi[i].max(l);
            jump = jump.max(l - *p);
            *p = l;
        }
        kicks += usize::from(levels.kick);
        beats += usize::from(levels.beat.is_some());
        locked += usize::from(levels.tempo.is_some());
        energy = energy.max(levels.energy);
        if let Some(p) = levels.tempo {
            bpm = 60.0 / p;
        }
        let s = show.debug_section();
        if !sections.ends_with(s) {
            sections.push_str(s);
        }
        t += dt;
        if t >= (sec + 1) as f32 {
            let swing = lo.iter().zip(&hi).map(|(a, b)| b - a).fold(0.0, f32::max);
            if energy > 0.5 {
                loud += 1;
                if jump < 0.25 {
                    dead += 1;
                }
                if swing < 0.2 {
                    flat += 1;
                }
            }
            lo.fill(1.0);
            hi.fill(0.0);
            if verbose {
                println!(
                    "{sec:4}s e{energy:.2} kicks{kicks:2} beats{beats:2} lock{locked:2} jump{jump:.2} swing{swing:.2} bpm{bpm:3.0} {sections}{}",
                    if energy > 0.5 && jump < 0.25 {
                        "  <-- DEAD"
                    } else {
                        ""
                    }
                );
            }
            sec += 1;
            (kicks, beats, jump, energy, locked) = (0, 0, 0.0, 0.0, 0);
            bpm = 0.0;
            sections = show.debug_section().to_string();
        }
    }
    println!(
        "{}: {loud}s loud, {dead}s without a hit ({:.0}%), {flat}s still",
        args[1],
        100.0 * dead as f32 / loud.max(1) as f32
    );
}
