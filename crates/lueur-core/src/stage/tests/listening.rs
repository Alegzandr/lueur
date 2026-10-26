//! The analyser: the bands, the onsets and the clock.

use super::*;

#[test]
fn a_kick_after_quiet_is_a_beat() {
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let quiet = tone(rate, 60.0, 0.02, 0.04);
    for _ in 0..50 {
        assert!(a.feed(&quiet).beat.is_none());
    }
    let l = a.feed(&tone(rate, 60.0, 0.8, 0.04));
    assert!(l.kick);
    assert!(l.beat.unwrap() > 0.9, "{l:?}");
    assert!(l.energy > 0.9);
}

#[test]
fn a_steady_tone_is_not_a_beat() {
    let rate = 44_100;
    let mut a = Analyzer::new(rate);
    let hum = tone(rate, 80.0, 0.5, 0.04);
    let beats = (0..200).filter(|_| a.feed(&hum).beat.is_some()).count();
    assert!(beats <= 1, "{beats} beats in a hum");
}

#[test]
fn the_tempo_is_found_and_kept() {
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let frames = four_on_the_floor(&mut a, rate, 128.0, 1.0, 12.0);
    let tempo = frames.last().unwrap().tempo.expect("no tempo found");
    assert!((tempo - 60.0 / 128.0).abs() < 0.01, "{tempo}");
    // Once locked, one beat per kick: none doubled, none dropped.
    let beats = frames[frames.len() / 2..]
        .iter()
        .filter(|l| l.beat.is_some())
        .count();
    let expected = (6.0 * 128.0 / 60.0) as usize;
    assert!(
        beats.abs_diff(expected) <= 1,
        "{beats} beats for {expected}"
    );
}

#[test]
fn the_next_track_sets_the_clock_again() {
    // Straight from one track into the next, two thirds as fast: every
    // other kick of the new one falls on a beat of the old, which keeps
    // the old clock sure. Its beats must not play on over the new track
    // for long.
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    four_on_the_floor(&mut a, rate, 120.0, 1.0, 12.0);
    let frames = four_on_the_floor(&mut a, rate, 80.0, 1.0, 12.0);
    let tempo = |l: &Levels| l.tempo.map(|p| 60.0 / p);
    let settled = frames[(8.0 / 0.025) as usize..]
        .iter()
        .all(|l| tempo(l).is_some_and(|bpm| (bpm - 80.0).abs() < 2.0));
    assert!(settled, "{:?}", frames.last().and_then(tempo));
}

#[test]
fn a_pause_between_tracks_forgets_the_beat() {
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    four_on_the_floor(&mut a, rate, 128.0, 1.0, 12.0);
    let silence = vec![0.0; (rate as f32 * 0.025) as usize];
    for _ in 0..(1.6 / 0.025) as usize {
        a.feed(&silence);
    }
    assert!(a.tempo.hits.is_empty());
    let frames = four_on_the_floor(&mut a, rate, 90.0, 1.0, 8.0);
    let tempo = frames.last().unwrap().tempo.expect("no tempo found");
    assert!((tempo - 60.0 / 90.0).abs() < 0.02, "{}", 60.0 / tempo);
}

#[test]
fn the_clock_moving_back_keeps_the_tempo() {
    // The clock rewinds a few seconds in, while the tempo locks.
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    a.time = 2.0 * CLOCK_SPAN - 5.0;
    let frames = four_on_the_floor(&mut a, rate, 128.0, 1.0, 12.0);
    assert!(a.time < CLOCK_SPAN + 10.0, "{}", a.time);
    let tempo = frames.last().unwrap().tempo.expect("tempo lost");
    assert!((tempo - 60.0 / 128.0).abs() < 0.01, "{tempo}");
    let beats = frames[frames.len() / 2..]
        .iter()
        .filter(|l| l.beat.is_some())
        .count();
    let expected = (6.0 * 128.0 / 60.0) as usize;
    assert!(
        beats.abs_diff(expected) <= 1,
        "{beats} beats for {expected}"
    );
}

#[test]
fn a_locked_beat_goes_out_early() {
    // Once the clock is sure, the beat leaves before the kick is heard.
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let frames = four_on_the_floor(&mut a, rate, 120.0, 1.0, 12.0);
    let early = frames[frames.len() / 2..]
        .iter()
        .filter(|l| l.beat.is_some() && !l.kick)
        .count();
    assert!(early >= 8, "only {early} beats played ahead of their kick");
}

#[test]
fn the_volume_knob_does_not_change_the_show() {
    let rate = 48_000;
    let level = |amp: f32| {
        let mut a = Analyzer::new(rate);
        let frames = four_on_the_floor(&mut a, rate, 120.0, amp, 8.0);
        let tail = &frames[frames.len() - 80..];
        tail.iter().map(|l| l.energy).sum::<f32>() / tail.len() as f32
    };
    let (loud, soft) = (level(1.0), level(0.05));
    assert!((loud - soft).abs() < 0.05, "{loud} vs {soft}");
    assert!(loud > 0.5, "{loud}");
}

#[test]
fn turning_the_volume_down_is_soon_forgotten() {
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let before = four_on_the_floor(&mut a, rate, 120.0, 1.0, 10.0);
    let after = four_on_the_floor(&mut a, rate, 120.0, 0.1, 8.0);
    let mean = |f: &[Levels]| f.iter().map(|l| l.energy).sum::<f32>() / f.len() as f32;
    let (was, now) = (
        mean(&before[before.len() - 80..]),
        mean(&after[after.len() - 80..]),
    );
    assert!(now > was - 0.08, "{was} -> {now}");
    // And the beat carried on throughout.
    assert!(after[after.len() / 2..].iter().any(|l| l.beat.is_some()));
}

#[test]
fn a_breakdown_stays_quieter_than_the_groove() {
    // The kick drops out and only the hi-hats carry on over a soft pad,
    // far quieter: that is the music, not the volume knob, and the show
    // must still read it as quieter long into the breakdown.
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let groove = four_on_the_floor(&mut a, rate, 120.0, 1.0, 10.0);
    let frame = (rate as f32 * 0.025) as usize;
    let mut breakdown = Vec::new();
    for k in 0..(16.0 / 0.025) as usize {
        let mut chunk: Vec<f32> = (0..frame)
            .map(|i| {
                let t = (k * frame + i) as f32 / rate as f32;
                0.02 * (TAU * 55.0 * t).sin()
            })
            .collect();
        if k % 10 == 0 {
            let hat = hiss(0.08, frame / 2, k as u32 + 1);
            for (s, h) in chunk.iter_mut().zip(hat) {
                *s += h;
            }
        }
        breakdown.push(a.feed(&chunk));
    }
    assert!(breakdown.iter().any(|l| l.hat.is_some()), "no hats heard");
    let mean = |f: &[Levels]| f.iter().map(|l| l.energy).sum::<f32>() / f.len() as f32;
    let (loud, quiet) = (
        mean(&groove[groove.len() - 80..]),
        mean(&breakdown[breakdown.len() - 80..]),
    );
    assert!(
        quiet < loud - 0.15,
        "the breakdown reads {quiet}, the groove {loud}"
    );
}

#[test]
fn the_bands_hear_their_own_instruments() {
    // A bass line and a hum, a hi-hat every fourth frame and a snare
    // every sixteenth, one 25 ms frame at a time. No kick.
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let mut snare_band = Biquad::new(Pass::Band, rate as f32, 2200.0, 0.7);
    let mut hat_band = Biquad::new(Pass::High, rate as f32, 9000.0, 0.7);
    let (mut kicks, mut snares, mut hats) = (0, 0, 0);
    for frame in 0..96u32 {
        // Whole cycles in a frame, so the frames join without a click.
        let bass = tone(rate, 40.0, 0.2, 0.025);
        let hum = tone(rate, 400.0, 0.01, 0.025);
        let mut chunk: Vec<f32> = bass.iter().zip(&hum).map(|(a, b)| a + b).collect();
        let n = chunk.len();
        if frame % 16 == 8 {
            for (c, s) in chunk.iter_mut().zip(hiss(0.6, n, frame + 1)) {
                *c += snare_band.run(s);
            }
        } else if frame % 4 == 0 {
            for (c, s) in chunk.iter_mut().zip(hiss(0.4, n, frame + 7)) {
                *c += hat_band.run(s);
            }
        }
        let l = a.feed(&chunk);
        kicks += usize::from(l.kick);
        snares += usize::from(l.snare.is_some());
        hats += usize::from(l.hat.is_some());
    }
    assert_eq!(kicks, 0, "a kick in the treble");
    assert!((4..=6).contains(&snares), "{snares} snares for 5");
    assert!(hats >= 14, "{hats} hats for 19");
}

#[test]
fn a_track_without_a_kick_still_finds_its_tempo() {
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let frames = unplugged(&mut a, rate, 15.0);
    assert!(
        frames.iter().all(|l| !l.kick),
        "a kick in a band without drums"
    );
    let tempo = frames.last().unwrap().tempo.expect("no tempo found");
    assert!((tempo - 0.6).abs() < 0.01, "{tempo}");
    // Its beats come from the clock, one a beat.
    let beats = frames[frames.len() / 2..]
        .iter()
        .filter(|l| l.beat.is_some())
        .count();
    let expected = (7.5 / 0.6) as usize;
    assert!(
        beats.abs_diff(expected) <= 1,
        "{beats} beats for {expected}"
    );
}

#[test]
fn the_clock_reads_on_between_packets_of_sound() {
    // The sound arrives 40 ms at a time and the show looks every 10 ms:
    // a beat due between two packets goes out then, not with the next
    // packet, and none is played twice.
    let rate = 48_000;
    let mut a = Analyzer::new(rate);
    let packet = (rate as f32 * 0.04) as usize;
    let beat = 0.5;
    let total = (12.0 * rate as f32) as usize;
    let (mut between, mut beats) = (0, 0);
    let mut chunk = Vec::with_capacity(packet);
    for i in 0..total {
        let t = i as f32 / rate as f32;
        let kick = if t % beat < 0.08 { 0.7 } else { 0.03 };
        chunk.push(kick * (TAU * 55.0 * t).sin());
        if chunk.len() < packet {
            continue;
        }
        let late = t > 6.0;
        let fired = a.feed_after(&chunk, 0.01).beat.is_some();
        beats += usize::from(late && fired);
        chunk.clear();
        for _ in 0..3 {
            let fired = a.feed_after(&[], 0.01).beat.is_some();
            between += usize::from(late && fired);
            beats += usize::from(late && fired);
        }
    }
    assert!(between >= 4, "only {between} beats between packets");
    let expected = (6.0 / beat) as usize;
    assert!(
        beats.abs_diff(expected) <= 1,
        "{beats} beats for {expected}"
    );
}
