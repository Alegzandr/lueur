use super::ambient::*;
use super::colour::*;
use super::palettes::*;
use super::*;
use crate::color::Rgb;

fn rgb(o: BulbOutput) -> Rgb {
    match o {
        BulbOutput::Color { rgb, .. } => rgb,
        other => panic!("expected a colour, got {other:?}"),
    }
}

fn settings(kind: ShowKind) -> ShowSettings {
    ShowSettings {
        kind,
        ..ShowSettings::default()
    }
}

/// Play `kind` on `n` lights for `secs`, frame by frame.
fn play(kind: ShowKind, n: usize, secs: f32) -> Vec<Vec<BulbOutput>> {
    play_at(settings(kind), n, secs)
}

fn play_at(s: ShowSettings, n: usize, secs: f32) -> Vec<Vec<BulbOutput>> {
    let kind = s.kind;
    let mut show = Show::new(kind, &Fixture::row(n));
    let dt = kind.frame_ms() as f32 / 1000.0;
    (0..(secs / dt) as usize)
        .map(|_| show.frame(&s, dt, Levels::default()))
        .collect()
}

/// The largest jump of any channel between two colours.
fn jump(a: Rgb, b: Rgb) -> u8 {
    a.r.abs_diff(b.r)
        .max(a.g.abs_diff(b.g))
        .max(a.b.abs_diff(b.b))
}

#[test]
fn blending_goes_round_the_wheel() {
    // Red to blue the short way passes through magenta, not grey.
    let mid = blend(Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), 0.5);
    assert!(mid.r > 200 && mid.b > 200 && mid.g < 30, "{mid:?}");
    assert_eq!(
        blend(Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), 0.0),
        Rgb::new(255, 0, 0)
    );
}

#[test]
fn noise_is_smooth_and_bounded() {
    let mut prev = noise(7, 0.0);
    for k in 1..4000 {
        let v = noise(7, k as f32 * 0.005);
        assert!((-1.0..=1.0).contains(&v));
        assert!((v - prev).abs() < 0.05, "noise jumps at {k}");
        prev = v;
    }
    assert_ne!(noise(1, 3.3), noise(2, 3.3), "seeds differ");
}

#[test]
fn evening_holds_back_the_bright_hues_only() {
    assert!((even(Rgb::new(0, 0, 255)) - 1.0).abs() < 1e-6);
    assert!(even(Rgb::new(255, 255, 0)) < 0.8);
    assert!(even(Rgb::new(255, 0, 0)) > 0.9);
}

#[test]
fn ambient_shows_flow_without_jumps() {
    for kind in [ShowKind::Rainbow, ShowKind::Sunset, ShowKind::Ocean] {
        let frames = play(kind, 3, 120.0);
        for w in frames.windows(2) {
            for (a, b) in w[0].iter().zip(&w[1]) {
                assert!(
                    jump(rgb(*a), rgb(*b)) <= 12,
                    "{kind:?} jumps: {a:?} -> {b:?}"
                );
                assert!(
                    a.bright().abs_diff(b.bright()) <= 2,
                    "{kind:?} brightness jumps"
                );
            }
        }
    }
}

#[test]
fn ambient_shows_move() {
    for kind in [ShowKind::Rainbow, ShowKind::Sunset, ShowKind::Ocean] {
        let frames = play(kind, 1, 10.0);
        assert_ne!(frames[0], frames[frames.len() - 1], "{kind:?} is frozen");
    }
}

#[test]
fn the_pace_runs_from_a_quarter_to_four_times() {
    let at = |speed| {
        ShowSettings {
            speed,
            ..settings(ShowKind::Rainbow)
        }
        .pace()
    };
    assert!((at(50) - 1.0).abs() < 1e-6);
    assert!((at(100) - 4.0).abs() < 1e-4);
    assert!(at(1) > 0.24 && at(1) < 0.26);
    assert_eq!(at(0), at(1));
}

#[test]
fn a_faster_show_covers_more_ground() {
    // At double pace, the rainbow is where it would be twice as late.
    let fast = ShowSettings {
        speed: 75,
        ..settings(ShowKind::Rainbow)
    };
    let quick = play_at(fast, 1, 5.0);
    let slow = play(ShowKind::Rainbow, 1, 10.0);
    let (a, _, _) = hsv(rgb(quick[quick.len() - 1][0]));
    let (b, _, _) = hsv(rgb(slow[slow.len() - 1][0]));
    assert!((a - b).abs() < 3.0, "{a}° vs {b}°");
}

#[test]
fn the_fastest_shows_still_flow() {
    for kind in [ShowKind::Rainbow, ShowKind::Sunset, ShowKind::Ocean] {
        let s = ShowSettings {
            speed: 100,
            ..settings(kind)
        };
        for w in play_at(s, 3, 60.0).windows(2) {
            for (a, b) in w[0].iter().zip(&w[1]) {
                assert!(
                    jump(rgb(*a), rgb(*b)) <= 40,
                    "{kind:?} jumps: {a:?} -> {b:?}"
                );
            }
        }
    }
}

/// How far apart two colours are round the wheel, in degrees.
fn gap(a: Rgb, b: Rgb) -> f32 {
    let ((a, _, _), (b, _, _)) = (hsv(a), hsv(b));
    (a - b).abs().min(360.0 - (a - b).abs())
}

/// Chartreuse and lime: the hues that look sickly on a bulb.
fn sickly(c: Rgb) -> bool {
    (65.0..=125.0).contains(&hsv(c).0)
}

/// One whole loop through the combinations, in seconds.
const LOOP: f32 = COMBO_SECS * COMBOS.len() as f32;

#[test]
fn the_rainbow_lays_a_combination_across_the_room() {
    // Left to right, each light its own colour, close enough to its
    // neighbour to blend, never a sickly colour, all the way round.
    for frame in play(ShowKind::Rainbow, 4, LOOP) {
        for w in frame.windows(2) {
            let g = gap(rgb(w[0]), rgb(w[1]));
            assert!((3.0..=60.0).contains(&g), "lights {g}° apart");
        }
        for o in &frame {
            assert!(!sickly(rgb(*o)), "a sickly colour: {:?}", rgb(*o));
        }
        assert!(gap(rgb(frame[0]), rgb(frame[3])) >= 20.0);
    }
}

#[test]
fn the_combinations_go_well_together() {
    for (i, combo) in COMBOS.iter().enumerate() {
        let next = &COMBOS[(i + 1) % COMBOS.len()];
        for k in 0..3 {
            assert!(!sickly(combo[k]), "combination {i}: {:?}", combo[k]);
            // Each colour has only a short way to go to the next.
            let g = gap(combo[k], next[k]);
            assert!(g <= 110.0, "combination {i}, colour {k}: {g}°");
        }
        for w in combo.windows(2) {
            let g = gap(w[0], w[1]);
            assert!((10.0..=80.0).contains(&g), "combination {i}: {g}°");
        }
    }
}

#[test]
fn a_glide_steers_clear_of_the_yellow_greens() {
    // Emerald to gold the short way is all chartreuse; the long way is not.
    let (emerald, gold) = (Rgb::new(0, 255, 110), Rgb::new(255, 160, 0));
    for k in 0..=20 {
        let c = clean_blend(emerald, gold, k as f32 / 20.0);
        assert!(!sickly(c), "{c:?}");
    }
    // Elsewhere it goes the short way, as a blend does.
    let mid = clean_blend(Rgb::new(255, 0, 0), Rgb::new(0, 0, 255), 0.5);
    assert!(mid.r > 200 && mid.b > 200 && mid.g < 30, "{mid:?}");
}

#[test]
fn the_rainbow_moves_through_every_combination() {
    // Over one loop, warm and cool alike, then back where it began.
    let frames = play(ShowKind::Rainbow, 1, LOOP);
    let hues: Vec<f32> = frames.iter().map(|f| hsv(rgb(f[0])).0).collect();
    let warm = |h: &f32| !(60.0..=330.0).contains(h);
    assert!(hues.iter().any(warm), "never warm");
    assert!(
        hues.iter().any(|h| (150.0..=230.0).contains(h)),
        "never cool"
    );
    let end = rgb(frames[frames.len() - 1][0]);
    assert!(gap(rgb(frames[0][0]), end) < 3.0, "the loop has a seam");
}

#[test]
fn the_rainbow_never_holds_still() {
    // Even while a combination lingers, the colours drift.
    let frames = play(ShowKind::Rainbow, 2, COMBO_SECS);
    for w in frames.chunks(25).collect::<Vec<_>>().windows(2) {
        assert_ne!(w[0][0], w[1][0], "the show stalled");
    }
}

#[test]
fn places_follow_the_room_left_to_right() {
    let mut room = Fixture::row(3);
    room.swap(0, 2);
    let p = places(&room);
    assert_eq!(p, vec![1.0, 0.5, 0.0]);
    assert_eq!(places(&Fixture::row(1)), vec![0.5]);
}

#[test]
fn the_sunset_stays_warm() {
    for frame in play(ShowKind::Sunset, 4, 180.0) {
        for o in frame {
            let (h, _, _) = hsv(rgb(o));
            assert!(!(60.0..=280.0).contains(&h), "a cold sunset: {h}°");
        }
    }
}

#[test]
fn the_ocean_stays_blue_and_swells() {
    let frames = play(ShowKind::Ocean, 2, 30.0);
    let mut lo = 100;
    let mut hi = 0;
    for frame in &frames {
        for o in frame {
            let (h, _, _) = hsv(rgb(*o));
            assert!((150.0..=245.0).contains(&h), "the sea turned {h}°");
            lo = lo.min(o.bright());
            hi = hi.max(o.bright());
        }
    }
    assert!(hi - lo > 20, "no swell: {lo}..{hi}");
    // The swell rolls: the two lights are not in step.
    assert!(
        frames
            .iter()
            .any(|f| f[0].bright().abs_diff(f[1].bright()) > 8)
    );
}

#[test]
fn show_ids_round_trip() {
    for k in SHOWS {
        assert_eq!(ShowKind::from_id(k.id()), Some(k));
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, format!("\"{}\"", k.id()));
    }
    assert_eq!(ShowKind::from_id("disco"), None);
}

#[test]
fn a_candle_stays_warm_and_alive() {
    let frames = play(ShowKind::Candle, 2, 60.0);
    let mut levels = Vec::new();
    for frame in &frames {
        for o in frame {
            let c = rgb(*o);
            assert!(c.r == 255 && c.g < 170 && c.b < 60, "{c:?}");
            levels.push(o.bright());
        }
    }
    let lo = *levels.iter().min().unwrap();
    let hi = *levels.iter().max().unwrap();
    assert!(hi - lo >= 6, "the flame never moves: {lo}..{hi}");
    assert!(hi <= 80, "brighter than asked: {hi}");
    // The two flames flicker on their own.
    assert!(frames.iter().any(|f| f[0] != f[1]));
}

#[test]
fn a_candle_dims_redder() {
    let low = blend(EMBER, FLAME, 0.0);
    let high = blend(EMBER, FLAME, 1.0);
    assert!(low.g < high.g);
}

/// The loop as the bulb plays it, frame by frame: each leg a straight glide
/// from the colour before, round and round for `secs`.
fn played_by_the_bulb(steps: &[Step], secs: f32) -> Vec<BulbOutput> {
    let dt = 0.04;
    let lap: f32 = steps.iter().map(|s| s.ms as f32 / 1000.0).sum();
    (0..(secs / dt) as usize)
        .map(|f| {
            let mut t = (f as f32 * dt) % lap;
            let mut i = 0;
            while t >= steps[i].ms as f32 / 1000.0 {
                t -= steps[i].ms as f32 / 1000.0;
                i += 1;
            }
            let from = steps[(i + steps.len() - 1) % steps.len()].output;
            let to = steps[i].output;
            let u = t * 1000.0 / steps[i].ms as f32;
            let lerp =
                |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * u).round() as u8;
            let (a, b) = (rgb(from), rgb(to));
            BulbOutput::Color {
                rgb: Rgb::new(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b)),
                bright: lerp(from.bright(), to.bright()),
            }
        })
        .collect()
}

#[test]
fn every_show_but_the_music_has_a_loop_of_its_own() {
    for kind in SHOWS {
        let own = kind.own_loop(&settings(kind), 0.5);
        assert_eq!(own.is_some(), kind != ShowKind::Music, "{kind:?}");
        if let Some(own) = own {
            assert_eq!(own.kind, kind);
            assert!(own.steps.len() >= 4, "{kind:?}");
            assert!(own.bright() <= settings(kind).bright, "{kind:?}");
        }
    }
}

#[test]
fn a_light_further_right_starts_its_loop_elsewhere() {
    for kind in [
        ShowKind::Rainbow,
        ShowKind::Sunset,
        ShowKind::Ocean,
        ShowKind::Candle,
    ] {
        let s = settings(kind);
        let left = kind.own_loop(&s, 0.0).unwrap().steps;
        let right = kind.own_loop(&s, 1.0).unwrap().steps;
        assert_ne!(left[0], right[0], "{kind:?}");
    }
}

#[test]
fn the_rainbow_a_bulb_plays_by_itself_goes_round_the_wheel() {
    let s = settings(ShowKind::Rainbow);
    let left = ShowKind::Rainbow.own_loop(&s, 0.0).unwrap().steps;
    let right = ShowKind::Rainbow.own_loop(&s, 1.0).unwrap().steps;
    // A minute round the wheel at the natural pace.
    let lap: u32 = left.iter().map(|s| s.ms).sum();
    assert!((59_000..=61_000).contains(&lap), "{lap}");
    let hue = |s: &Step| hsv(rgb(s.output)).0;
    assert!(hue(&left[0]) < 1.0);
    // The light at the right starts a third of the way round.
    assert!((hue(&right[0]) - 120.0).abs() < 2.0, "{}", hue(&right[0]));
    // Never brighter than asked.
    assert!(left.iter().all(|step| step.output.bright() <= s.bright));
}

#[test]
fn a_loop_a_bulb_plays_by_itself_never_flashes() {
    // The bulb plays it without Lueur, so the guard cannot hold it back:
    // even at the fastest pace and full brightness, wherever the light
    // stands, it must have nothing to hold back.
    for kind in SHOWS {
        for place in [0.0, 0.5, 1.0] {
            let s = ShowSettings {
                speed: 100,
                bright: 100,
                ..settings(kind)
            };
            let Some(own) = kind.own_loop(&s, place) else {
                continue;
            };
            let frames = played_by_the_bulb(&own.steps, 60.0);
            let mut guard = crate::safety::FlashGuard::show(1);
            for (k, frame) in frames.iter().enumerate() {
                let mut out = [*frame];
                guard.apply(&mut out, 0.04, true);
                assert_eq!(
                    out[0], *frame,
                    "{kind:?} at {place}: held back at frame {k}"
                );
            }
        }
    }
}
