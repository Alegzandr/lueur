use super::ambilight::*;
use super::*;

fn fps(n: u64) -> Duration {
    Duration::from_secs_f64(1.0 / n as f64)
}

#[test]
fn the_frame_rate_follows_the_live_settings() {
    let mut config = AppConfig::default();
    config.capture.target_fps = 60;
    assert_eq!(LiveSettings::of(&config).frame_budget(), fps(60));
    config.capture.target_fps = 30;
    assert_eq!(LiveSettings::of(&config).frame_budget(), fps(30));
    // Out-of-range rates from a hand-edited config are kept sane.
    assert_eq!(frame_budget(0), fps(1));
    assert_eq!(frame_budget(1000), fps(120));
}

#[test]
fn sample_step_follows_capture_width() {
    assert_eq!(scaled_step(4, 1920), 4);
    assert_eq!(scaled_step(4, 3840), 8);
    // A GPU-shrunk frame is read pixel by pixel.
    assert_eq!(scaled_step(4, 480), 1);
    assert_eq!(scaled_step(1, 0), 1);
}

#[test]
fn sudden_stays_sudden() {
    assert_eq!(pace(Effect::Sudden, 300, fps(30)), (Effect::Sudden, 0));
}

#[test]
fn transition_is_cut_to_the_frame_interval() {
    // The reported bug: 300 ms fades re-issued every frame. At 30 fps
    // each one gets the 33 ms until the next.
    assert_eq!(pace(Effect::Smooth, 300, fps(30)), (Effect::Smooth, 33));
}

#[test]
fn a_fast_frame_rate_still_glides() {
    // Dropping to Sudden here is what made every family step at the
    // default 45 fps: the fade they should stretch never reached them.
    assert_eq!(pace(Effect::Smooth, 300, fps(60)), (Effect::Smooth, 30));
    assert_eq!(pace(Effect::Smooth, 80, fps(45)), (Effect::Smooth, 30));
}

#[test]
fn a_short_request_is_left_alone() {
    // Well inside a 10 fps budget (100 ms): nothing to clamp.
    assert_eq!(pace(Effect::Smooth, 80, fps(10)), (Effect::Smooth, 80));
}

#[test]
fn never_emits_a_smooth_below_the_protocol_floor() {
    for f in [15u64, 24, 30, 45, 60, 90, 120] {
        let (effect, ms) = pace(Effect::Smooth, 300, fps(f));
        assert_eq!(effect, Effect::Smooth, "{f} fps");
        assert!(
            ms >= lueur_core::yeelight::MIN_SMOOTH_DURATION_MS,
            "{f} fps produced a {ms} ms smooth"
        );
    }
}

#[test]
fn the_guard_is_handed_the_light_the_walls_will_get() {
    use lueur_core::color::Rgb;
    let orange = BulbOutput::Color {
        rgb: Rgb::new(255, 150, 100),
        bright: 80,
    };
    let mut outputs = vec![orange];
    let asked = on_walls(&mut outputs, Rgb::new(120, 255, 200));
    // The window draws what the screen asked for; the bulb, and so the
    // guard before it, get the red that corrects for a green wall.
    assert_eq!(asked, vec![orange]);
    assert_eq!(outputs, vec![orange.on_wall(Rgb::new(120, 255, 200))]);
    match outputs[0] {
        BulbOutput::Color { rgb, bright } => assert!(rgb.r == 255 && rgb.g < 100 && bright == 80),
        other => panic!("{other:?}"),
    }
}
