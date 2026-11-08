//! The two backdrops under the window: behind Lighting, a pool of light
//! per light where it stands, in the colour it was last sent; behind the
//! other pages, the room's photograph as its lights make it now.
//!
//! The pools are worked out as the Tauri versions' shader worked them out
//! (see `glow.rs`) and stepped once per frame the screen shows: while they
//! move, the window calls `Backdrop.frame` at every frame, and once they
//! rest it stops, and nothing is worked out until the lights change again.
//! Slint has no blur filter: the photograph is blurred once, in Rust, as it
//! is first shown (`crate::images::blurred`).

use super::glow;
use super::model::{Doing, Look, Room, kelvin_rgb, photo_name, seen_from, tint_rgb};
use crate::state::{Lueur, View};
use crate::{AppWindow, Backdrop, Motion, rt};
use lueur_shell::runner::Glow;
use lueur_shell::service;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What a zone's pool shows on screen, eased toward what its light shows.
struct Pool {
    rgb: [f32; 3],
    level: f32,
    x: f32,
    y: f32,
}

/// The glow and the room's photograph, as last shown.
pub struct State {
    shown: HashMap<String, Pool>,
    /// When the pools started to come in, for their arrival.
    born: Instant,
    last: Instant,
    /// True while something moves.
    animating: bool,
    /// The page size and theme the last glow was worked out for.
    drawn: Option<((f32, f32), bool)>,
    /// Whether Lighting was shown at the last look, so the glow comes in
    /// again each time it is.
    on_lighting: bool,
    /// The look of the room shown behind the other pages, and the timer
    /// that dissolves it into the next.
    room: Option<Look>,
    dark: Option<bool>,
    dissolve: Option<slint::Timer>,
    noise_set: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            shown: HashMap::new(),
            born: Instant::now(),
            last: Instant::now(),
            animating: false,
            drawn: None,
            on_lighting: false,
            room: None,
            dark: None,
            dissolve: None,
            noise_set: false,
        }
    }
}

/// RGBA pixels as an image Slint draws.
fn rgba_image(w: u32, h: u32, pixels: &[u8]) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        pixels, w, h,
    ))
}

fn colour([r, g, b]: [u8; 3]) -> slint::Color {
    slint::Color::from_rgb_u8(r, g, b)
}

/// Bring the backdrops in line with the state: called with every change.
pub fn project(l: &Lueur, ui: &AppWindow) {
    let g = ui.global::<Backdrop>();
    let lighting = l.view == View::Lighting;
    g.set_glow_shown(lighting);
    let mut st = l.pages.lighting.backdrop.borrow_mut();
    if !st.noise_set {
        st.noise_set = true;
        g.set_noise(rgba_image(
            glow::NOISE_SIDE,
            glow::NOISE_SIDE,
            &glow::noise(),
        ));
    }
    if lighting {
        if !st.on_lighting {
            // Shown again, the glow comes in again.
            st.born = Instant::now();
            st.shown.clear();
        }
        st.on_lighting = true;
        // A change to the lights wakes the pools; once still, they cost
        // nothing until the next one.
        if !st.animating {
            st.animating = true;
            st.last = Instant::now();
        }
        g.set_animating(l.window_shown);
    } else {
        st.on_lighting = false;
        st.animating = false;
        g.set_animating(false);
        follow_room(&mut st, l, ui);
    }
}

/// One frame: step the pools and draw the glow. Called by the window while
/// `Backdrop.animating` is set.
pub fn frame() {
    let Some(ui) = rt::ui() else { return };
    rt::read(|l| {
        let g = ui.global::<Backdrop>();
        let mut st = l.pages.lighting.backdrop.borrow_mut();
        if l.view != View::Lighting || !l.window_shown {
            st.animating = false;
            g.set_animating(false);
            return;
        }
        let still = ui.global::<Motion>().get_reduce();
        let moving = step(&mut st, l, still);
        draw(&mut st, &ui, true);
        if !moving {
            st.animating = false;
            g.set_animating(false);
        }
    });
}

/// The page changed size: the glow is drawn again for it.
pub fn resized() {
    let Some(ui) = rt::ui() else { return };
    rt::read(|l| {
        if l.view == View::Lighting {
            let mut st = l.pages.lighting.backdrop.borrow_mut();
            draw(&mut st, &ui, false);
        }
    });
}

/// Ease every pool toward what its light shows. True while something still
/// moves or an effect plays.
fn step(st: &mut State, l: &Lueur, still: bool) -> bool {
    let now = Instant::now();
    let dt = now.duration_since(st.last).as_secs_f32().min(0.1);
    st.last = now;
    let room = Room::of(l);
    let running = l.status.running;
    // The frames the effects last sent, read as they go out.
    let glows: HashMap<String, Vec<Glow>> = if running {
        service::light_glows()
    } else {
        HashMap::new()
    };
    let mut moving = arrival(st) < 1.0;
    let mut seen = Vec::new();
    let screen = l.config.room.screen;
    for light in l.config.room.lights.iter().filter(|x| room.bound(&x.id)) {
        let (u, v) = seen_from(&screen, light);
        let (sx, sy) = (
            (0.5 + u * 0.32).clamp(0.06, 0.94),
            (0.5 + v * 0.22).clamp(0.18, 0.85),
        );
        let driven = room.driven(&light.id);
        let zones: Vec<([u8; 3], f32)> = if driven {
            glows
                .get(&light.id)
                .map(|g| {
                    g.iter()
                        .map(|g| {
                            let rgb = g
                                .rgb
                                .unwrap_or_else(|| kelvin_rgb(g.kelvin.unwrap_or(2700)));
                            (rgb, f32::from(g.bright) / 100.0)
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else if room.on(&light.id) {
            let s = room.state(&light.id);
            vec![(tint_rgb(s.tint), f32::from(s.bright) / 100.0)]
        } else {
            Vec::new()
        };
        let n = zones.len() as f32;
        for (j, (rgb, level)) in zones.into_iter().enumerate() {
            let key = format!("{}/{j}", light.id);
            // A light with several zones spreads them side by side.
            let x = sx + (j as f32 - (n - 1.0) / 2.0) * 0.05;
            let target = rgb.map(f32::from);
            let pool = st.shown.entry(key.clone()).or_insert(Pool {
                rgb: target,
                level: 0.0,
                x,
                y: sy,
            });
            // A frame swells in quickly and ebbs a little slower: the beat
            // still shows, but a pool this large never flashes on screen.
            // Set by hand, it fades gently. Reduced motion only drifts.
            let tau = if still {
                1.5
            } else if driven {
                if level > pool.level { 0.06 } else { 0.15 }
            } else {
                0.35
            };
            let k = 1.0 - (-dt / tau).exp();
            for (c, t) in pool.rgb.iter_mut().zip(target) {
                *c += (t - *c) * k;
            }
            pool.level += (level - pool.level) * k;
            if (level - pool.level).abs() > 0.002
                || pool
                    .rgb
                    .iter()
                    .zip(target)
                    .any(|(c, t)| (c - t).abs() > 0.5)
            {
                moving = true;
            }
            pool.x = x;
            pool.y = sy;
            seen.push(key);
        }
    }
    // Lights turned off or stopped fade out rather than vanish.
    st.shown.retain(|key, pool| {
        if seen.contains(key) {
            return true;
        }
        pool.level *= (-dt / 0.35).exp();
        moving = true;
        pool.level >= 0.01
    });
    moving || running
}

/// The glow comes in gently when the page opens.
fn arrival(st: &State) -> f32 {
    let t = (st.born.elapsed().as_secs_f32() / 0.6).min(1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Work the glow out for the page's size, when something moved (`moved`)
/// or the size or theme changed since the last one.
fn draw(st: &mut State, ui: &AppWindow, moved: bool) {
    let g = ui.global::<Backdrop>();
    let (w, h) = (g.get_page_width(), g.get_page_height());
    if w < 1.0 || h < 1.0 {
        return;
    }
    let dark = ui.global::<crate::Theme>().get_dark();
    if !moved && st.drawn == Some(((w, h), dark)) {
        return;
    }
    st.drawn = Some(((w, h), dark));
    // Stronger and purer on paper, softer at night (`--backdrop`).
    let (strength, saturation) = if dark { (0.7, 1.15) } else { (0.6, 1.5) };
    let pools: Vec<glow::Pool> = st
        .shown
        .values()
        .map(|p| glow::Pool {
            x: p.x,
            y: p.y,
            rgb: p.rgb,
            level: p.level,
        })
        .collect();
    let (tw, th) = glow::image_size(w, h);
    let pixels = glow::render(&pools, (w, h), (tw, th), strength * arrival(st), saturation);
    g.set_glow(rgba_image(tw, th, &pixels));
    // Where light reaches, the only place the dither goes: the rest of the
    // page stays perfectly plain.
    let lit: Vec<(f32, f32, f32)> = pools
        .iter()
        .filter(|p| p.level > 0.0)
        .map(|p| glow::reach_square(p, (w, h)))
        .collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (l, t, s) in &lit {
        x0 = x0.min(*l);
        y0 = y0.min(*t);
        x1 = x1.max(l + s);
        y1 = y1.max(t + s);
    }
    if lit.is_empty() {
        g.set_noise_width(0.0);
    } else {
        let (x0, y0) = (x0.max(0.0), y0.max(0.0));
        let (x1, y1) = (x1.min(w), y1.min(h));
        g.set_noise_x(x0.floor());
        g.set_noise_y(y0.floor());
        g.set_noise_width((x1 - x0).max(0.0).ceil());
        g.set_noise_height((y1 - y0).max(0.0).ceil());
    }
}

/// The look most of the lights share, the dark room when there are none.
fn room_look(l: &Lueur) -> Look {
    let room = Room::of(l);
    let looks: Vec<Look> = l
        .config
        .bindings
        .iter()
        .map(|b| Look::of(&room.doing_of(&b.light_id)))
        .collect();
    let mut best: Option<(&Look, usize)> = None;
    for look in &looks {
        let n = looks.iter().filter(|o| o.key == look.key).count();
        if best.is_none_or(|(_, m)| n > m) {
            best = Some((look, n));
        }
    }
    best.map_or_else(|| Look::of(&Doing::Off), |(l, _)| l.clone())
}

/// The photograph for a look, out of focus as the theme has it: at night
/// the room in the dark, a little out of focus; on paper, blurred until no
/// furniture is left.
fn picture(look: &Look, dark: bool) -> Image {
    crate::images::blurred(photo_name(look.picture), if dark { 6 } else { 40 })
}

/// A new look fades in over the one before, over 700 ms, at once under
/// reduced motion.
fn follow_room(st: &mut State, l: &Lueur, ui: &AppWindow) {
    let g = ui.global::<Backdrop>();
    let dark = ui.global::<crate::Theme>().get_dark();
    g.set_room_opacity(if dark { 0.62 } else { 0.85 });
    let look = room_look(l);
    let same = st.room.as_ref().is_some_and(|r| r.key == look.key);
    if same && st.dark == Some(dark) {
        return;
    }
    let reduce = ui.global::<Motion>().get_reduce();
    let first = st.room.is_none() || st.dark != Some(dark);
    if let Some(before) = st.room.as_ref().filter(|_| !first && !reduce) {
        g.set_room_before(picture(before, dark));
        g.set_before_tinted(before.tint.is_some());
        g.set_before_tint(colour(before.tint.unwrap_or_default()));
        g.set_room_mix(0.0);
        let started = Instant::now();
        let weak = ui.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || {
                let t = (started.elapsed().as_secs_f32() / 0.7).min(1.0);
                // Ease in and out.
                let eased = t * t * (3.0 - 2.0 * t);
                if let Some(ui) = weak.upgrade() {
                    ui.global::<Backdrop>().set_room_mix(eased);
                }
                if t >= 1.0 {
                    // Dropped once over, after this call.
                    slint::Timer::single_shot(Duration::ZERO, || {
                        rt::read(|l| l.pages.lighting.backdrop.borrow_mut().dissolve = None);
                    });
                }
            },
        );
        st.dissolve = Some(timer);
    } else {
        st.dissolve = None;
        g.set_room_mix(1.0);
    }
    g.set_room(picture(&look, dark));
    g.set_room_tinted(look.tint.is_some());
    g.set_room_tint(colour(look.tint.unwrap_or_default()));
    st.room = Some(look);
    st.dark = Some(dark);
}
