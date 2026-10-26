//! Plays a track through the music show and reports where it goes quiet,
//! how it dressed the track (the tiers of its looks, its mood, half time,
//! white) and what the analyser heard.
//!
//! `ffmpeg -i track.mp3 -ac 1 -ar 48000 -f f32le track.raw`, then
//! `cargo run --release --example music_bench -- track.raw [lights] [-v] [-l]`:
//! `-v` reports every second, `-l` lists every change of look.

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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
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
    // How the show dressed the track: frames per tier, mood and half time,
    // frames with a light in white, and every change of look.
    let mut tiers = [0usize; 3];
    let (mut soft, mut half, mut white, mut frames) = (0usize, 0usize, 0usize, 0usize);
    let mut looks: Vec<(f32, String)> = Vec::new();
    // What tells the tracks apart: hits a second by instrument, how sure
    // and how steady the tempo is, how the bands play against their loud
    // parts.
    let (mut n_kick, mut n_snare, mut n_hat, mut n_pulse) = (0usize, 0usize, 0usize, 0usize);
    let mut n_lock = 0usize;
    let mut bpms: Vec<f32> = Vec::new();
    let (mut treble, mut mid, mut bass_sum) = (0.0f32, 0.0f32, 0.0f32);
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
        let look = show.debug_look();
        let tag = look.as_bytes();
        tiers[match tag[0] {
            b'g' => 0,
            b'd' => 1,
            _ => 2,
        }] += 1;
        soft += usize::from(tag[1] == b'S');
        half += usize::from(tag[2] == b'H');
        white += usize::from(out.iter().any(|o| matches!(o, BulbOutput::White { .. })));
        frames += 1;
        // The look itself, without the numbers behind it.
        let shown: String = look.split(" kicks").next().unwrap_or("").to_string();
        if looks.last().is_none_or(|(_, l)| *l != shown) {
            looks.push((t, shown));
        }
        n_kick += usize::from(levels.kick);
        n_snare += usize::from(levels.snare.is_some());
        n_hat += usize::from(levels.hat.is_some());
        n_pulse += usize::from(levels.pulse.is_some());
        if let Some(p) = levels.tempo {
            n_lock += 1;
            bpms.push(60.0 / p);
        }
        treble += levels.treble;
        mid += levels.mid;
        bass_sum += levels.bass;
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
                    "{sec:4}s e{energy:.2} kicks{kicks:2} beats{beats:2} lock{locked:2} jump{jump:.2} swing{swing:.2} bpm{bpm:3.0} {sections} | {}{}",
                    show.debug_look(),
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
    let pct = |n: usize| 100.0 * n as f32 / frames.max(1) as f32;
    println!(
        "  looks gentle {:.0}% driving {:.0}% peak {:.0}% | soft {:.0}% | half time {:.0}% | white {:.1}% | {} changes of look",
        pct(tiers[0]),
        pct(tiers[1]),
        pct(tiers[2]),
        pct(soft),
        pct(half),
        pct(white),
        looks.len()
    );
    let secs = frames as f32 * dt;
    let mean = bpms.iter().sum::<f32>() / bpms.len().max(1) as f32;
    // How often the tempo read jumps by more than 5%, a second.
    let jumps = bpms
        .windows(2)
        .filter(|w| (w[1] / w[0] - 1.0).abs() > 0.05)
        .count();
    let f = frames.max(1) as f32;
    println!(
        "  /s kick {:.2} snare {:.2} hat {:.2} pulse {:.2} | locked {:.0}% bpm {:.0} jumps/min {:.1} | bass {:.2} mid {:.2} treble {:.2}",
        n_kick as f32 / secs,
        n_snare as f32 / secs,
        n_hat as f32 / secs,
        n_pulse as f32 / secs,
        pct(n_lock),
        mean,
        60.0 * jumps as f32 / secs,
        bass_sum / f,
        mid / f,
        treble / f,
    );
    if args.iter().any(|a| a == "-l") {
        for (t, look) in &looks {
            println!("  {:5.1}s {look}", t);
        }
    }
}
