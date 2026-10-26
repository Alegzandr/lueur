//! The stage following the track: its sections, phrases, looks and moods.

use super::*;

#[test]
fn a_quick_light_waits_for_the_slow_one() {
    // Two lights struck together: the one that shows a command in no
    // time waits a tenth of a second for the other, and is sent sharp
    // when it does.
    let mut fixtures = Fixture::row(2);
    fixtures[0].latency = Duration::ZERO;
    fixtures[1].latency = Duration::from_millis(100);
    let s = ShowSettings::default();
    let mut stage = Stage::new(&fixtures);
    stage.enter(Section::Groove);
    for _ in 0..4 {
        stage.frame(&s, between(), 0.025);
    }
    let struck = stage.frame(&s, beat(1.0), 0.025);
    assert!((stage.lead() - 0.1).abs() < 1e-6);
    assert!(struck[1].bright() > 55 && stage.sharp()[1]);
    assert!(struck[0].bright() < 30 && !stage.sharp()[0]);
    assert!(stage.due(0).is_some_and(|d| d > 0.05 && d <= 0.1));
    let mut shown_at = None;
    for k in 1..=6 {
        let f = stage.frame(&s, between(), 0.025);
        if f[0].bright() > 55 && shown_at.is_none() {
            shown_at = Some(k);
            assert!(stage.sharp()[0]);
        }
    }
    assert_eq!(shown_at, Some(4));
}

#[test]
fn the_phrase_is_set_right_where_the_sound_turns() {
    // The count starts wherever the clock found the beat; the track's
    // phrase starts at the 22nd beat heard, where the hats and the
    // vocals come in. A few beats on, the count runs from there.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    let verse = Levels {
        energy: 0.6,
        bass: 0.6,
        mid: 0.2,
        treble: 0.1,
        ..Levels::default()
    };
    let chorus = Levels {
        energy: 0.9,
        bass: 0.9,
        mid: 0.6,
        treble: 0.7,
        ..Levels::default()
    };
    for j in 1..=60u32 {
        let sound = if j < 22 { verse } else { chorus };
        let struck = Levels {
            beat: Some(1.0),
            kick: true,
            ..sound
        };
        stage.frame(&s, struck, 0.025);
        if j >= 28 {
            assert_eq!(stage.beat, (j - 22) % PHRASE_BEATS, "beat {j}");
        }
        for _ in 0..19 {
            stage.frame(&s, sound, 0.025);
        }
    }
    // And a steady groove is never set anywhere.
    let mut steady = Stage::new(&Fixture::row(3));
    steady.enter(Section::Groove);
    for j in 1..=60u32 {
        steady.frame(&s, beat(1.0), 0.025);
        assert_eq!(steady.beat, j % PHRASE_BEATS);
        for _ in 0..19 {
            steady.frame(&s, between(), 0.025);
        }
    }
}

#[test]
fn a_crash_on_a_jump_starts_a_phrase() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    // The verse sat at 0.6; the band jumps up as the cymbal crashes.
    let verse = Levels {
        energy: 0.6,
        ..between()
    };
    settle(&mut stage, verse.energy);
    for k in 0..7 * 20 {
        let l = if k % 20 == 0 {
            Levels {
                beat: Some(1.0),
                kick: true,
                ..verse
            }
        } else {
            verse
        };
        stage.frame(&s, l, 0.025);
    }
    assert_eq!(stage.beat, 7);
    let phrases = stage.phrases;
    let crash = Levels {
        energy: 1.0,
        bass: 1.0,
        beat: Some(1.0),
        kick: true,
        hat: Some(1.0),
        ..Levels::default()
    };
    stage.frame(&s, crash, 0.025);
    assert_eq!(stage.phrases, phrases + 1);
    assert_eq!(stage.beat, 0);
}

#[test]
fn the_kick_coming_back_starts_a_phrase() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    settle(&mut stage, 0.6);
    groove(&mut stage, &s, 5);
    let calm = Levels {
        energy: 0.5,
        ..Levels::default()
    };
    for _ in 0..260 {
        stage.frame(&s, calm, 0.025);
    }
    assert_eq!(stage.section, Section::Calm);
    let phrases = stage.phrases;
    stage.frame(&s, beat(1.0), 0.025);
    assert_eq!(stage.section, Section::Groove);
    assert_eq!((stage.phrases, stage.beat), (phrases + 1, 0));
}

#[test]
fn a_kick_or_two_missed_is_not_a_breakdown() {
    // The bass hides a few kicks: the groove carries on through them,
    // and losing the beat a little longer picks it up in the same
    // colours.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    settle(&mut stage, 0.8);
    groove(&mut stage, &s, 5);
    let between = Levels {
        energy: 0.8,
        ..Levels::default()
    };
    for _ in 0..60 {
        stage.frame(&s, between, 0.025);
    }
    assert_eq!(stage.section, Section::Groove);
    for _ in 0..60 {
        stage.frame(&s, between, 0.025);
    }
    assert_eq!(stage.section, Section::Calm);
    let (phrases, beat_was) = (stage.phrases, stage.beat);
    stage.frame(&s, beat(1.0), 0.025);
    assert_eq!(stage.section, Section::Groove);
    assert_eq!(stage.phrases, phrases);
    assert_eq!(stage.beat, beat_was + 1);
}

#[test]
fn a_sure_clock_carries_the_groove_without_a_kick() {
    // The clock is sure and the music holds its level: its beats keep
    // the groove going when the kick cannot be heard.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    settle(&mut stage, 0.8);
    groove(&mut stage, &s, 5);
    for k in 0..(10.0 / 0.025) as usize {
        let l = Levels {
            energy: 0.8,
            beat: (k % 20 == 0).then_some(0.8),
            tempo: Some(0.5),
            ..Levels::default()
        };
        stage.frame(&s, l, 0.025);
        assert_eq!(stage.section, Section::Groove, "at frame {k}");
    }
    // The level falls away: a breakdown, clock or not.
    for k in 0..(6.0 / 0.025) as usize {
        let l = Levels {
            energy: 0.3,
            beat: (k % 20 == 0).then_some(0.8),
            tempo: Some(0.5),
            ..Levels::default()
        };
        stage.frame(&s, l, 0.025);
    }
    assert_eq!(stage.section, Section::Calm);
}

#[test]
fn a_track_without_a_kick_grooves_on_the_clock() {
    // Beats from the clock alone, no kick ever, the music carrying on
    // at its level: a groove from the first.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    settle(&mut stage, 0.7);
    let mut peak = 0;
    for k in 0..(24.0 / 0.025) as usize {
        let l = Levels {
            energy: 0.7,
            bass: 0.5,
            beat: (k % 24 == 0).then_some(0.8),
            tempo: Some(0.6),
            ..Levels::default()
        };
        let f = stage.frame(&s, l, 0.025);
        let t = k as f32 * 0.025;
        if t > 1.0 {
            assert_eq!(stage.section, Section::Groove, "at {t}");
        }
        if t > 20.0 {
            peak = peak.max(f.iter().map(|o| o.bright()).max().unwrap());
        }
    }
    assert_eq!(stage.section, Section::Groove);
    // A beat at 0.8 strikes in its colour at full chroma, which the
    // brightest hues dim a little: well clear of the dark between.
    assert!(peak >= 50, "the beats never punched: {peak}");
}

#[test]
fn without_beats_the_colours_drift() {
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(1));
    let first = rgb(stage.frame(&s, Levels::default(), 0.025)[0]);
    let mut last = first;
    for _ in 0..400 {
        last = rgb(stage.frame(&s, Levels::default(), 0.025)[0]);
    }
    assert_ne!(first, last);
}

#[test]
fn a_chase_hands_the_beat_on() {
    // The light that has the beat holds it to the next one, the others
    // dark; as the next takes it, it goes out within the beat.
    let s = ShowSettings::default();
    let mut stage = playing(Pattern::Chase);
    let mut f = stage.frame(&s, beat(1.0), 0.025);
    for _ in 0..19 {
        f = stage.frame(&s, between(), 0.025);
    }
    let holder = (0..3).max_by_key(|&i| f[i].bright()).unwrap();
    assert!(f[holder].bright() > 30, "{f:?}");
    assert!((0..3).all(|i| i == holder || f[i].bright() < 15), "{f:?}");
    let f = stage.frame(&s, beat(1.0), 0.025);
    let next = (0..3).max_by_key(|&i| f[i].bright()).unwrap();
    assert_ne!(next, holder);
    let held = f[holder].bright();
    let mut f = f;
    for _ in 0..8 {
        f = stage.frame(&s, between(), 0.025);
    }
    assert!(f[holder].bright() < held / 2, "{held} -> {f:?}");
    assert!(f[next].bright() > 30, "{f:?}");
}

#[test]
fn a_swap_trades_the_colours_on_every_beat() {
    let s = ShowSettings::default();
    let mut stage = playing(Pattern::Swap);
    let mut struck = Vec::new();
    for _ in 0..2 {
        let f = stage.frame(&s, beat(1.0), 0.025);
        struck.push([rgb(f[0]), rgb(f[1]), rgb(f[2])]);
        for _ in 0..19 {
            let f = stage.frame(&s, between(), 0.025);
            // Every light burns on between the beats.
            assert!(f.iter().all(|o| o.bright() > 20), "{f:?}");
        }
    }
    let [a, b] = [struck[0], struck[1]];
    assert_ne!(a[0], a[1]);
    assert_eq!((a[0], a[1], a[2]), (b[1], b[0], b[1]));
}

#[test]
fn a_carousel_moves_the_colours_along() {
    let s = ShowSettings::default();
    let mut stage = playing(Pattern::Carousel);
    let mut struck = Vec::new();
    for _ in 0..3 {
        let f = stage.frame(&s, beat(1.0), 0.025);
        struck.push([rgb(f[0]), rgb(f[1]), rgb(f[2])]);
        for _ in 0..19 {
            stage.frame(&s, between(), 0.025);
        }
    }
    // Three colours, one a light, each one light further right on
    // the next beat.
    assert!(struck[0][0] != struck[0][1] && struck[0][1] != struck[0][2]);
    for w in struck.windows(2) {
        for r in 0..3 {
            assert_eq!(w[1][(r + 1) % 3], w[0][r]);
        }
    }
}

#[test]
fn the_look_follows_how_hard_the_music_plays() {
    // The body of the track, its chorus, a quiet verse, the chorus
    // again: the body travels, the verse gets a look where every light
    // stays lit, and the chorus comes back to its own.
    let s = ShowSettings::default();
    let mut stage = Stage::new(&Fixture::row(3));
    stage.enter(Section::Groove);
    let body = Levels {
        energy: 0.8,
        bass: 0.8,
        mid: 0.5,
        treble: 0.5,
        ..Levels::default()
    };
    let chorus = Levels {
        energy: 0.9,
        bass: 0.9,
        mid: 0.8,
        treble: 0.8,
        ..Levels::default()
    };
    let verse = Levels {
        energy: 0.6,
        bass: 0.7,
        mid: 0.2,
        treble: 0.1,
        ..Levels::default()
    };
    let four = |j: u32| (true, j % 2 == 1);
    // A track at one level all through is all body.
    drive(&mut stage, &s, 0.5, 40, body, four);
    assert_eq!(stage.tier, DRIVING);
    assert!(Pattern::for_lights(3, DRIVING).contains(&stage.look));
    drive(&mut stage, &s, 0.5, 24, chorus, four);
    assert_eq!(stage.tier, PEAK);
    let chorus_look = stage.look;
    assert!(Pattern::for_lights(3, PEAK).contains(&chorus_look));
    drive(&mut stage, &s, 0.5, 16, verse, four);
    assert_eq!(stage.tier, GENTLE);
    assert!(Pattern::for_lights(3, GENTLE).contains(&stage.look));
    drive(&mut stage, &s, 0.5, 16, chorus, four);
    assert_eq!(stage.tier, PEAK);
    assert_eq!(stage.look, chorus_look);
}

#[test]
fn the_one_of_the_bar_strikes_hardest() {
    let s = ShowSettings::default();
    let mut stage = playing(Pattern::Together);
    settle(&mut stage, 0.8);
    let frames = groove(&mut stage, &s, 8);
    // The first beat counted is the two: the fourth, the one of the
    // second bar.
    let at = |j: usize| frames[j * 20][0].bright();
    assert!(
        at(3) > at(4) + 5,
        "{} on the one, {} on the two",
        at(3),
        at(4)
    );
}

#[test]
fn a_gentle_track_plays_warm_and_never_white() {
    let s = ShowSettings::default();
    // A ballad's treble comes and goes: over half a minute it sits well
    // under its loud parts.
    let sound = Levels {
        energy: 0.7,
        bass: 0.6,
        treble: 0.5,
        ..Levels::default()
    };
    // 86 beats a minute, a kick on the one of each bar.
    let mut ballad = Stage::new(&Fixture::row(3));
    ballad.enter(Section::Groove);
    drive(&mut ballad, &s, 0.7, 48, sound, |j| {
        (j % 4 == 0, j % 4 == 2)
    });
    assert_eq!(ballad.mood, Mood::Soft);
    assert!(SOFT.contains(ballad.harmony()));
    // Even its drop stays in colour.
    ballad.enter(Section::Hold);
    let out = ballad.frame(&s, beat(1.0), 0.025);
    assert_eq!(ballad.section, Section::Drop);
    assert!(out.iter().all(|o| matches!(o, BulbOutput::Color { .. })));
    // A club track, a kick on every beat and the hats never stopping:
    // the party.
    let club_sound = Levels {
        treble: 0.85,
        ..sound
    };
    let mut club = Stage::new(&Fixture::row(3));
    club.enter(Section::Groove);
    club.mood = Mood::Soft;
    drive(&mut club, &s, 0.47, 48, club_sound, |j| (true, j % 2 == 1));
    assert_eq!(club.mood, Mood::Party);
    assert!(PARTY.contains(club.harmony()));
}

#[test]
fn a_snare_on_the_three_plays_in_half_time() {
    let s = ShowSettings::default();
    let sound = Levels {
        energy: 0.8,
        bass: 0.8,
        ..Levels::default()
    };
    // 140 beats a minute, the snare on the three alone: half time.
    let mut trap = playing(Pattern::Together);
    let frames = drive(&mut trap, &s, 0.425, 32, sound, |j| {
        (j % 4 == 0, j % 4 == 2)
    });
    assert!(trap.half);
    // Late on, the beats struck come every other one, somewhere in the
    // room.
    let per = 17;
    let lit: Vec<bool> = (24..32)
        .map(|j| frames[j * per].iter().any(|o| o.bright() > 40))
        .collect();
    assert!(
        lit.windows(2).all(|w| w[0] != w[1]),
        "every other beat should strike: {lit:?}"
    );
    // The same tempo, the snare on the two and the four: a full beat.
    let mut rock = playing(Pattern::Together);
    drive(&mut rock, &s, 0.425, 32, sound, |j| {
        (j % 2 == 0, j % 2 == 1)
    });
    assert!(!rock.half);
    // And a slow groove with its snare on the three is no half time.
    let mut slow = playing(Pattern::Together);
    drive(&mut slow, &s, 0.7, 32, sound, |j| (j % 4 == 0, j % 4 == 2));
    assert!(!slow.half);
}

#[test]
fn a_loud_snare_off_the_drop_stays_in_colour() {
    // White is spent on the drops: a loud chorus's snares crack in
    // colour, even with the white flashes on.
    let s = ShowSettings::default();
    let mut fixtures = Fixture::row(4);
    fixtures[1].height = 0.9;
    let mut stage = Stage::new(&fixtures);
    stage.enter(Section::Groove);
    let loud = Levels {
        energy: 1.0,
        bass: 1.0,
        treble: 0.9,
        ..Levels::default()
    };
    settle(&mut stage, 1.0);
    let frames = drive(&mut stage, &s, 0.5, 16, loud, |j| (true, j % 2 == 1));
    // A party, where white is allowed at all.
    assert_eq!(stage.mood, Mood::Party);
    assert!(
        frames
            .iter()
            .flatten()
            .all(|o| matches!(o, BulbOutput::Color { .. })),
        "a white flash outside a drop"
    );
}

#[test]
fn the_washes_trade_colours_with_the_bars() {
    // Two Hues beside two Yeelights: the Hues glide between the room's
    // colour and the second hit colour, a bar each, out of step.
    let mut fixtures = Fixture::row(4);
    for f in &mut fixtures[..2] {
        f.interval = Duration::from_millis(200);
    }
    let s = ShowSettings::default();
    let mut stage = Stage::new(&fixtures);
    assert_eq!(stage.lamps[0].part, Part::Wash);
    assert_eq!(stage.lamps[1].part, Part::Wash);
    stage.enter(Section::Groove);
    let frames = groove(&mut stage, &s, 12);
    let apart = frames.iter().any(|f| rgb(f[0]) != rgb(f[1]));
    assert!(apart, "the washes never parted");
    let first = rgb(frames[0][0]);
    assert!(frames.iter().any(|f| rgb(f[0]) != first));
}
