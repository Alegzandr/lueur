//! The stage: the parts, the patterns and the colours.

use super::*;

#[test]
fn parts_follow_the_room() {
    let q = Duration::from_millis(0);
    let near = |x: f32, height: f32| Fixture {
        x,
        depth: 0.0,
        height,
        interval: q,
        latency: q,
    };
    // Three alike: all three on the beat, so the patterns can travel.
    let three = [near(-1.0, 0.5), near(0.0, 0.5), near(1.0, 0.5)];
    assert_eq!(cast(&three), vec![Part::Punch; 3]);
    // One at the ceiling takes the snare.
    let up = [near(-1.0, 0.5), near(0.0, 0.9), near(1.0, 0.5)];
    assert_eq!(cast(&up), vec![Part::Punch, Part::Accent, Part::Punch]);
    // A slow bulb, and one across the room, wash.
    let hue = Fixture {
        interval: Duration::from_millis(200),
        ..near(0.0, 0.5)
    };
    let far = Fixture {
        depth: 4.0,
        ..near(0.0, 0.5)
    };
    assert_eq!(
        cast(&[near(-1.0, 0.5), hue, far]),
        vec![Part::Punch, Part::Wash, Part::Wash]
    );
    // Someone always has the beat.
    assert_eq!(cast(&[hue, far]), vec![Part::Wash, Part::Punch]);
    assert_eq!(cast(&[hue]), vec![Part::Punch]);
}

#[test]
fn zones_spread_along_the_wall() {
    let f = Fixture::row(1)[0];
    let zones = f.zones(3);
    let xs: Vec<f32> = zones.iter().map(|z| z.x).collect();
    assert_eq!(xs, vec![-ZONE_SPAN, 0.0, ZONE_SPAN]);
    assert_eq!(f.zones(1), vec![f]);
}

#[test]
fn silence_is_calm() {
    let mut a = Analyzer::new(44_100);
    let l = a.feed(&vec![0.0; 1764]);
    assert_eq!(l, Levels::default());
    let mut s = Stage::new(&Fixture::row(1));
    let out = s.frame(&ShowSettings::default(), l, 0.025);
    assert!(out[0].bright() < 20);
}

#[test]
fn a_drop_gives_way_after_a_phrase() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    settle(&mut stage, 0.8);
    stage.drop(&s, &beat(1.0));
    // A phrase found late keeps setting the count back.
    for _ in 0..PHRASE_BEATS + 1 {
        stage.beat = stage.beat.min(4);
        stage.frame(&s, beat(1.0), 0.025);
        for _ in 0..19 {
            stage.frame(&s, between(), 0.025);
        }
    }
    assert_eq!(stage.section, Section::Groove);
}

#[test]
fn the_patterns_move_across_the_room() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    let frames = groove(&mut stage, &s, 48);
    // Somewhere along the way, one light was hit alone: a chase.
    let alone = frames.iter().any(|f| {
        let lit = f.iter().filter(|o| o.bright() > 55).count();
        lit == 1
    });
    assert!(alone, "never a light on its own");
    // And somewhere, all three at once.
    assert!(frames.iter().any(|f| f.iter().all(|o| o.bright() > 55)));
}

#[test]
fn a_chase_walks_left_to_right() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    stage.look = Pattern::Chase;
    let mut order = Vec::new();
    for _ in 0..3 {
        let f = stage.frame(&s, beat(1.0), 0.025);
        let top = (0..3).max_by_key(|&i| f[i].bright()).unwrap();
        order.push(top);
        for _ in 0..19 {
            stage.frame(&s, between(), 0.025);
        }
    }
    assert_eq!(order, vec![1, 2, 0]);
}

#[test]
fn the_colours_hold_through_a_phrase() {
    // On every beat the punches strike in the phrase's hit colour, from
    // the harmony playing; it holds for the whole phrase and moves on
    // as the next one lands. Over a few minutes, every colour of every
    // harmony strikes.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    let beats = PHRASE_BEATS * PHRASES_PER_HARMONY * PARTY.len() as u32;
    let mut seen = Vec::new();
    let mut last: Option<(u32, Rgb)> = None;
    for k in 0..beats as usize * 20 {
        let l = if k % 20 == 0 { beat(1.0) } else { between() };
        let frame = stage.frame(&s, l, 0.025);
        if k % 20 != 0 {
            continue;
        }
        let harmony = stage.harmony().map(full_chroma);
        for o in frame.iter().filter(|o| o.bright() > 55) {
            let c = rgb(*o);
            assert!(harmony.contains(&c), "{c:?} is not in {harmony:?}");
            seen.push(c);
        }
        if stage.pattern() != Pattern::Together {
            continue;
        }
        let c = rgb(frame[0]);
        assert!(frame.iter().all(|o| rgb(*o) == c), "the punches disagree");
        match last {
            Some((p, before)) if p == stage.phrases => assert_eq!(c, before),
            Some((_, before)) => assert_ne!(c, before, "a new phrase, the same colour"),
            None => {}
        }
        last = Some((stage.phrases, c));
    }
    assert!(
        PARTY
            .iter()
            .flatten()
            .all(|c| seen.contains(&full_chroma(*c)))
    );
}

#[test]
fn a_hit_fades_back_into_the_rooms_colour() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(1));
    stage.enter(Section::Groove);
    let struck = rgb(stage.frame(&s, beat(1.0), 0.025)[0]);
    let mut rest = struck;
    for _ in 0..19 {
        rest = rgb(stage.frame(&s, between(), 0.025)[0]);
    }
    let (back, hit, _) = stage.roles();
    assert_eq!(struck, full_chroma(hit));
    assert_ne!(rest, struck);
    // Nearly home by the next beat.
    let (b, r) = (full_chroma(back), rest);
    let off =
        b.r.abs_diff(r.r)
            .max(b.g.abs_diff(r.g))
            .max(b.b.abs_diff(r.b));
    assert!(off < 40, "{r:?} is far from {b:?}");
}

#[test]
fn a_hit_lasts_as_long_as_the_beat() {
    // A slow track's hits fade more slowly than a fast one's, and each
    // is nearly out before its next beat.
    let s = ShowSettings::default();
    let level_after = |period: f32, secs: f32| {
        let mut stage = Stage::new(&Fixture::row(1));
        stage.enter(Section::Groove);
        let quiet = Levels {
            tempo: Some(period),
            ..Levels::default()
        };
        stage.frame(&s, quiet, 0.025);
        stage.frame(
            &s,
            Levels {
                beat: Some(1.0),
                kick: true,
                ..quiet
            },
            0.025,
        );
        let mut last = 0;
        for _ in 0..(secs / 0.025) as usize {
            last = stage.frame(&s, quiet, 0.025)[0].bright();
        }
        last
    };
    assert!(level_after(0.8, 0.3) > level_after(0.4, 0.3));
    assert!(level_after(0.4, 0.4) < 15);
}

#[test]
fn the_rings_move_from_the_middle_out_and_back() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(5));
    // Five alike at the same height: one takes the snare, four punch.
    let punches: Vec<usize> = (0..5)
        .filter(|&i| stage.lamps[i].part == Part::Punch)
        .collect();
    assert_eq!(punches.len(), 4);
    stage.enter(Section::Groove);
    for (pattern, first) in [(Pattern::CentreOut, [1, 2]), (Pattern::OutsideIn, [0, 3])] {
        stage.look = pattern;
        stage.beat = 0;
        assert_eq!(stage.pattern(), pattern);
        // The first beat of the phrase is beat 1: the second ring, then
        // back to the first.
        let mut lit = Vec::new();
        for _ in 0..2 {
            let f = stage.frame(&s, beat(1.0), 0.025);
            // Off the one, the beats strike a little softer.
            let on: Vec<usize> = (0..punches.len())
                .filter(|&r| f[punches[r]].bright() > 40)
                .collect();
            lit.push(on);
            for _ in 0..19 {
                stage.frame(&s, between(), 0.025);
            }
        }
        let other: Vec<usize> = (0..4).filter(|r| !first.contains(r)).collect();
        assert_eq!(lit[0], other, "{pattern:?}");
        assert_eq!(lit[1], first.to_vec(), "{pattern:?}");
    }
    assert_eq!(ring(0, 3), (1, 2));
    assert_eq!(ring(1, 3), (0, 2));
    assert_eq!(ring(2, 4), (0, 2));
}

#[test]
fn a_build_quickens_as_it_climbs() {
    let s = ShowSettings {
        flashes: false,
        ..ShowSettings::default()
    };
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Build);
    let steps = |stage: &mut Stage, secs: f32| {
        let before = stage.roll;
        for _ in 0..(secs / 0.025) as usize {
            let l = Levels {
                energy: 0.6,
                ..Levels::default()
            };
            stage.frame(&s, l, 0.025);
            assert_eq!(stage.section, Section::Build);
        }
        stage.roll - before
    };
    let early = steps(&mut stage, 2.0);
    stage.held = 6.0;
    let late = steps(&mut stage, 2.0);
    assert!((3..=5).contains(&early), "{early} steps early on");
    assert!(late >= 3 * early, "{early} then {late}");
}

#[test]
fn loud_music_still_pulses() {
    // Compressed music never leaves the top of its range; the beat must
    // still stand out against it.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(1));
    stage.enter(Section::Groove);
    let loud = Levels {
        energy: 1.0,
        bass: 1.0,
        ..Levels::default()
    };
    let mut between = 0;
    for _ in 0..40 {
        between = stage.frame(&s, loud, 0.025)[0].bright();
    }
    let hit = Levels {
        beat: Some(1.0),
        kick: true,
        ..loud
    };
    // On the one of the bar, which strikes hardest.
    stage.beat = PHRASE_BEATS - 1;
    let flash = stage.frame(&s, hit, 0.025)[0].bright();
    assert!(flash >= s.bright - 5 && flash <= s.bright, "{flash}");
    assert!(flash - between >= 20, "{between} -> {flash}");
    assert!(stage.sharp()[0]);
}

#[test]
fn a_breakdown_dims_and_a_drop_lands_white() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(2));
    stage.enter(Section::Groove);
    let loud = groove(&mut stage, &s, 8);
    let peak = loud.iter().map(|f| f[0].bright()).max().unwrap();

    // The kick goes: a breakdown.
    let calm = Levels {
        energy: 0.5,
        bass: 0.3,
        ..Levels::default()
    };
    let mut dim = 100;
    for _ in 0..160 {
        dim = stage.frame(&s, calm, 0.025)[0].bright();
    }
    assert_eq!(stage.section, Section::Calm);
    assert!(dim < peak / 2, "{dim} in a breakdown, {peak} on the beat");

    // A snare roll that crowds in and climbs: a build.
    for k in 0..240 {
        let l = Levels {
            energy: 0.5 + 0.4 * k as f32 / 240.0,
            snare: (k % 3 == 0).then_some(0.8),
            ..Levels::default()
        };
        stage.frame(&s, l, 0.025);
    }
    assert_eq!(stage.section, Section::Build);

    // The breath before the drop: dark.
    let hush = Levels {
        energy: 0.05,
        ..Levels::default()
    };
    let held = (0..8)
        .map(|_| stage.frame(&s, hush, 0.025)[0])
        .last()
        .unwrap();
    assert_eq!(stage.section, Section::Hold);
    assert!(held.bright() < 10, "{held:?}");

    // The kick lands: white, everyone, sharp.
    let out = stage.frame(&s, beat(1.0), 0.025);
    assert_eq!(stage.section, Section::Drop);
    for (o, sharp) in out.iter().zip(stage.sharp()) {
        assert!(matches!(o, BulbOutput::White { .. }), "{o:?}");
        assert!(o.bright() >= 75);
        assert!(sharp);
    }
}

#[test]
fn no_white_flashes_when_turned_off() {
    let s = ShowSettings {
        flashes: false,
        ..ShowSettings::default()
    };
    let mut stage = Stage::new(&Fixture::row(2));
    stage.enter(Section::Hold);
    let out = stage.frame(&s, beat(1.0), 0.025);
    assert_eq!(stage.section, Section::Drop);
    assert!(out.iter().all(|o| matches!(o, BulbOutput::Color { .. })));
}

#[test]
fn white_flashes_stay_under_three_a_second() {
    // The top of a build: every snare walks a white flash to the next
    // light, eight a second. Each light flashes only twice a second; the
    // room flashes eight times. With the protection off as on, the
    // show's guard holds the room to three.
    let s = ShowSettings::default();
    let dt = 0.025;
    let mut stage = Stage::new(&Fixture::row(4));
    stage.enter(Section::Build);
    stage.held = 7.0;
    let frames: Vec<Vec<BulbOutput>> = (0..160)
        .map(|k| {
            let l = Levels {
                energy: 0.8,
                snare: (k % 5 == 0).then_some(1.0),
                ..Levels::default()
            };
            stage.frame(&s, l, dt)
        })
        .collect();
    assert_eq!(stage.section, Section::Build);
    let raw = room_white_flashes(&frames, dt);
    assert!(raw > 3, "the roll only flashes {raw} times a second");
    for on in [false, true] {
        let mut guard = FlashGuard::show(4);
        let sent: Vec<Vec<BulbOutput>> = frames
            .iter()
            .map(|f| {
                let mut f = f.clone();
                guard.apply(&mut f, dt, on);
                f
            })
            .collect();
        let n = room_white_flashes(&sent, dt);
        assert!(n <= 3, "{n} white flashes a second, protection on: {on}");
    }
}

#[test]
fn a_slow_bulb_washes_without_flicker() {
    // A Hue next to two Yeelights: the Yeelights punch, the Hue glides.
    let mut fixtures = Fixture::row(3);
    fixtures[1].interval = Duration::from_millis(200);
    let s = ShowSettings::default();
    let mut stage = Stage::new(&fixtures);
    stage.enter(Section::Groove);
    let frames = groove(&mut stage, &s, 16);
    for w in frames.windows(2) {
        let jump = w[0][1].bright().abs_diff(w[1][1].bright());
        assert!(jump <= 20, "the wash jumped by {jump}");
    }
    let swing = |i: usize| {
        let b: Vec<u8> = frames.iter().map(|f| f[i].bright()).collect();
        b.iter().max().unwrap() - b.iter().min().unwrap()
    };
    assert!(swing(0) > 40, "the punch barely moves");
}
