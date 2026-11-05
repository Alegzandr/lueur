//! The two backdrops under the window: behind Lighting, a pool of light
//! per light where it stands, in the colour it was last sent; behind the
//! other pages, the room's photograph as its lights make it now.
//!
//! GPUI has neither a shader for the page nor a blur filter. The pools are
//! soft shadows of round shapes, which GPUI blurs as Gaussian bells, laid
//! over the ground rather than added as light. The photograph is drawn
//! several times, a little apart and each fainter, which averages it into a
//! blur; on paper a wash of the ground then lifts it to a high key, as the
//! stylesheet's filter does.

use super::controls::photo;
use super::model::{Doing, Look, Room, kelvin_rgb, seen_from, tint_rgb};
use crate::state::{Lueur, View};
use crate::theme::{Theme, theme};
use gpui::{
    Animation, AnimationExt, BoxShadow, Context, Entity, Hsla, IntoElement, Render, Rgba, Task,
    Window, div, ease_in_out, linear_color_stop, linear_gradient, prelude::*, px,
};
use lueur_shell::runner::Glow;
use lueur_shell::service;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// About a frame of a 60 Hz screen, between two steps of the pools.
const FRAME: Duration = Duration::from_millis(16);

/// What a zone's pool shows on screen, eased toward what its light shows.
struct Pool {
    rgb: [f32; 3],
    level: f32,
    x: f32,
    y: f32,
}

pub struct GlowBackdrop {
    lueur: Entity<Lueur>,
    shown: HashMap<String, Pool>,
    /// When the pools started to come in, for their arrival.
    born: Instant,
    last: Instant,
    /// The backdrop's size at the last frame: the page's, not the window's.
    size: (f32, f32),
    /// Steps the pools while something moves; None once they rest.
    ticking: Option<Task<()>>,
}

impl GlowBackdrop {
    pub fn new(lueur: Entity<Lueur>, cx: &mut Context<Self>) -> Self {
        // A change to the lights wakes the pools; once still, they cost
        // nothing until the next one.
        cx.observe(&lueur, |this, lueur, cx| {
            if lueur.read(cx).view == View::Lighting {
                this.wake(cx);
            } else {
                // Shown again later, the glow comes in again.
                this.born = Instant::now();
            }
        })
        .detach();
        let mut this = GlowBackdrop {
            lueur,
            shown: HashMap::new(),
            born: Instant::now(),
            last: Instant::now(),
            size: (0.0, 0.0),
            ticking: None,
        };
        this.wake(cx);
        this
    }

    fn wake(&mut self, cx: &mut Context<Self>) {
        if self.ticking.is_some() {
            return;
        }
        self.last = Instant::now();
        self.ticking = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let go_on = this
                    .update(cx, |this, cx| {
                        let moving = this.step(cx);
                        cx.notify();
                        if !moving {
                            this.ticking = None;
                        }
                        moving
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        }));
    }

    /// Ease every pool toward what its light shows. True while something
    /// still moves or an effect plays.
    fn step(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let still = cx.reduce_motion();
        let l = self.lueur.read(cx);
        if l.view != View::Lighting || cx.windows().is_empty() {
            return false;
        }
        let room = Room::of(l);
        let running = l.status.running;
        // The frames the effects last sent, read as they go out.
        let glows: HashMap<String, Vec<Glow>> = if running {
            service::light_glows()
        } else {
            HashMap::new()
        };
        let mut moving = self.arrival() < 1.0;
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
                let pool = self.shown.entry(key.clone()).or_insert(Pool {
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
        self.shown.retain(|key, pool| {
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
    fn arrival(&self) -> f32 {
        let t = (self.born.elapsed().as_secs_f32() / 0.6).min(1.0);
        1.0 - (1.0 - t).powi(3)
    }
}

/// More saturated, as coloured light on a wall looks.
fn saturate(rgb: [f32; 3], by: f32) -> [f32; 3] {
    let grey = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
    rgb.map(|c| (grey + (c - grey) * by).clamp(0.0, 255.0))
}

fn colour(rgb: [f32; 3], a: f32) -> Hsla {
    Rgba {
        r: rgb[0] / 255.0,
        g: rgb[1] / 255.0,
        b: rgb[2] / 255.0,
        a,
    }
    .into()
}

impl Render for GlowBackdrop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        // Stronger and purer on paper, softer at night (`--backdrop`).
        let (strength, saturation) = if theme.dark { (0.7, 1.15) } else { (0.6, 1.5) };
        let (w, h) = if self.size.0 > 0.0 {
            self.size
        } else {
            let size = window.viewport_size();
            (f32::from(size.width), f32::from(size.height))
        };
        let this = cx.entity().downgrade();
        let known = self.size;
        let measure = gpui::canvas(
            move |bounds, _, cx| {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if (size.0 - known.0).abs() > 0.5 || (size.1 - known.1).abs() > 0.5 {
                    let _ = this.update(cx, |b, cx| {
                        b.size = size;
                        cx.notify();
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let arrival = self.arrival();
        let pools = self.shown.values().map(|p| {
            // The pool's reach; the eye reads a dim bulb as brighter than
            // its share of full power.
            let reach = w.max(h) * (0.32 + 0.12 * p.level);
            let alpha = (0.75 * p.level.powf(0.6) * strength * arrival).min(1.0);
            // A round shape a third of the reach, blurred to a bell that
            // fades out by the reach; its blur thins its middle, which the
            // colour's strength makes up for.
            let core = reach * 0.7;
            div()
                .absolute()
                .left(px(p.x * w - core / 2.0))
                .top(px(p.y * h - core / 2.0))
                .size(px(core))
                .rounded_full()
                .shadow(vec![
                    BoxShadow::new(
                        px(0.0),
                        px(0.0),
                        colour(saturate(p.rgb, saturation), (alpha * 2.0).min(1.0)),
                    )
                    .blur_radius(px(reach * 0.55)),
                ])
        });
        div()
            .absolute()
            .inset_0()
            .overflow_hidden()
            .child(measure)
            .children(pools)
    }
}

/// One room photograph shown, and since when, so a new one dissolves in.
struct Layer {
    id: u64,
    look: Look,
}

pub struct RoomBackdrop {
    lueur: Entity<Lueur>,
    layers: Vec<Layer>,
    next: u64,
    _settle: Option<Task<()>>,
}

impl RoomBackdrop {
    pub fn new(lueur: Entity<Lueur>, cx: &mut Context<Self>) -> Self {
        cx.observe(&lueur, |this, _, cx| this.follow(cx)).detach();
        let mut this = RoomBackdrop {
            lueur,
            layers: Vec::new(),
            next: 0,
            _settle: None,
        };
        this.follow(cx);
        this
    }

    /// The look most of the lights share, the dark room when there are
    /// none.
    fn look(&self, cx: &Context<Self>) -> Look {
        let l = self.lueur.read(cx);
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

    /// A new look fades in over the one before, which goes once covered.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let look = self.look(cx);
        if self.layers.last().is_some_and(|l| l.look.key == look.key) {
            return;
        }
        self.next += 1;
        let layer = Layer {
            id: self.next,
            look,
        };
        if cx.reduce_motion() || self.layers.is_empty() {
            self.layers = vec![layer];
        } else {
            let last = self.layers.pop();
            self.layers = last.into_iter().chain([layer]).collect();
            let id = self.next;
            self._settle = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(720))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.layers.retain(|l| l.id >= id);
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }
}

/// One photograph, out of focus by `blur` pixels: drawn at the centre and
/// on three rings around it, each copy fainter than the ones under it, so
/// the copies average out.
fn blurred(path: &'static str, blur: f32) -> gpui::Div {
    let mut offsets = vec![(0.0, 0.0)];
    for ring in [0.34, 0.67, 1.0] {
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4 + ring * 1.2;
            offsets.push((a.cos() * blur * ring, a.sin() * blur * ring));
        }
    }
    let zoom = Rc::new(Cell::new(1.06 + blur / 400.0));
    div()
        .absolute()
        .inset_0()
        .children(offsets.into_iter().enumerate().map(|(k, offset)| {
            div()
                .absolute()
                .inset_0()
                .opacity(1.0 / (k as f32 + 1.0))
                .child(
                    photo(path, zoom.clone(), 0.62, offset, 0.0)
                        .absolute()
                        .size_full(),
                )
        }))
}

impl Render for RoomBackdrop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme: Theme = *theme(cx);
        let height = (f32::from(window.viewport_size().height) * 0.62).min(560.0);
        // At night the room in the dark, a little out of focus; on paper,
        // blurred until no furniture is left and lifted to a high key.
        let (opacity, blur) = if theme.dark {
            (0.62, 6.0)
        } else {
            (0.85, 40.0)
        };
        let n = self.layers.len();
        let layers = self.layers.iter().enumerate().map(|(i, layer)| {
            let picture = blurred(layer.look.picture, blur)
                .when_some(layer.look.tint, |d, c| {
                    d.child(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(colour(c.map(f32::from), 0.45)),
                    )
                })
                .when(!theme.dark, |d| {
                    d.child(div().absolute().inset_0().bg(theme.bg.opacity(0.55)))
                });
            if i + 1 == n && n > 1 {
                picture
                    .with_animation(
                        ("room-layer", layer.id),
                        Animation::new(Duration::from_millis(700)).with_easing(ease_in_out),
                        |d, t| d.opacity(t),
                    )
                    .into_any_element()
            } else {
                picture.into_any_element()
            }
        });
        let bg = theme.bg;
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(height))
            .overflow_hidden()
            .child(div().absolute().inset_0().opacity(opacity).children(layers))
            // On paper the dark screen in the middle would only grey the
            // ground: a soft patch of the ground covers it.
            .when(!theme.dark, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(height * 0.05))
                        .left(gpui::relative(0.38))
                        .w(gpui::relative(0.24))
                        .h(px(height * 0.35))
                        .rounded_full()
                        .shadow(vec![
                            BoxShadow::new(px(0.0), px(0.0), bg.opacity(0.75))
                                .blur_radius(px(height * 0.3)),
                        ]),
                )
            })
            // Gone by the time the cards gather: the page's own ground below.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(gpui::relative(0.45))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(bg.opacity(0.0), 0.0),
                        linear_color_stop(bg.opacity(0.45), 1.0),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .top(gpui::relative(0.45))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(bg.opacity(0.45), 0.0),
                        linear_color_stop(bg, 1.0),
                    )),
            )
    }
}
