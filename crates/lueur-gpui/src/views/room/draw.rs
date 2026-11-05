//! The plan: the room drawn from
//! above (walls, screen, seat and lights), each dragged into place.
//!
//! It is painted on a `canvas` in the stage's pixels: positions go through
//! the frame that fits the room to the stage, while strokes, handles and
//! text keep a constant size in pixels, as the SVG did with its `k`. The
//! presses and drags are caught on the canvas and hit-tested in plan units
//! (see [`plan::hit`]). Over the canvas, each light also has an invisible
//! focusable pin, so the keyboard and screen readers reach every light by
//! name and its zone, and the arrows move it.

use super::colors::PlanColors;
use super::plan::{self, Frame, Other, Target, ViewBox};
use super::{Drag, RoomPage};
use crate::app::FONT;
use crate::i18n::{fill as fill_in, t};
use crate::theme::{Theme, size as sz};
use crate::ui;
use gpui::{
    AnyElement, App, Bounds, Context, CursorStyle, DispatchPhase, FontWeight, Hitbox,
    HitboxBehavior, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, Point, Role, SharedString, TextAlign, TextRun, WeakEntity, Window, canvas,
    div, fill, font, point, prelude::*, px, size,
};
use lueur_core::geometry::{LightMapping, Room, ScreenEdge, Vec2, map_light};

/// Everything the canvas paints, taken from the page when it renders.
struct Scene {
    room: Room,
    maps: Vec<LightMapping>,
    selected: Option<String>,
    other: Option<Other>,
    hover: Option<Target>,
    dragging: bool,
    vb: ViewBox,
    colors: PlanColors,
    theme: Theme,
    letters: [SharedString; 3],
    you: SharedString,
    screen: SharedString,
}

fn edge_index(e: ScreenEdge) -> usize {
    match e {
        ScreenEdge::Left => 0,
        ScreenEdge::Center => 1,
        ScreenEdge::Right => 2,
    }
}

impl RoomPage {
    /// The stage: the plan's well, its canvas, the lights' pins and the
    /// help line under them.
    pub(super) fn plan(
        &mut self,
        now: &super::Now,
        theme: &Theme,
        colors: &PlanColors,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let room = now.config.room.clone();
        let (stage_w, stage_h) = self.stage;
        let vb = self
            .held
            .unwrap_or_else(|| plan::fit(&room, stage_w, stage_h));
        self.drawn = vb;
        // The frame as the pins see it, from the stage's top-left corner.
        let frame = Frame::new(vb, (0.0, 0.0), stage_w, stage_h);
        let maps: Vec<LightMapping> = room
            .lights
            .iter()
            .map(|light| map_light(&room.screen, light))
            .collect();
        let letters = plan::EDGES.map(|e| t(cx, plan::letter_key(e)));

        let pins: Vec<AnyElement> = if stage_w > 0.0 {
            room.lights
                .iter()
                .zip(&maps)
                .enumerate()
                .map(|(i, (light, m))| {
                    let (x, y) = frame.to_px(light.pos);
                    let selected = now.selected.as_deref() == Some(light.id.as_str());
                    let zone = fill_in(
                        &t(cx, "zone_of"),
                        &[
                            ("letter", &letters[edge_index(m.edge)]),
                            ("zone", &t(cx, plan::kind_key(m.edge))),
                        ],
                    );
                    let label = if light.enabled {
                        format!("{}, {zone}", light.name)
                    } else {
                        format!("{}, {}", light.name, t(cx, "left_out"))
                    };
                    let id = light.id.clone();
                    let key_id = light.id.clone();
                    let page = cx.entity().downgrade();
                    let key_page = page.clone();
                    ui::pressable(("room-pin", i), theme)
                        .role(Role::Button)
                        .aria_label(SharedString::from(label))
                        .aria_selected(selected)
                        .absolute()
                        .left(px(x - 15.0))
                        .top(px(y - 15.0))
                        .size(px(30.0))
                        .rounded_full()
                        .cursor(CursorStyle::OpenHand)
                        .on_click(move |e, _, cx| {
                            // A press on the plan already selected it; the
                            // keyboard's Enter or Space does it here.
                            if e.is_keyboard() {
                                let id = id.clone();
                                let _ = page.update(cx, |p, cx| p.select_light(id, cx));
                            }
                        })
                        .on_key_down(move |e: &KeyDownEvent, _, cx| {
                            // The arrows move the light the focus is on: it
                            // becomes the selection first, then the page
                            // moves it.
                            if matches!(e.keystroke.key.as_str(), "left" | "right" | "up" | "down")
                            {
                                let id = key_id.clone();
                                let _ = key_page.update(cx, |p, cx| {
                                    if p.selected_light(cx).as_deref() != Some(id.as_str()) {
                                        p.select_light(id, cx);
                                    }
                                });
                            }
                        })
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };

        let scene = Scene {
            room,
            maps,
            selected: now.selected.clone(),
            other: self.other,
            hover: self.hover.clone(),
            dragging: self.drag.is_some(),
            vb,
            colors: *colors,
            theme: *theme,
            letters,
            you: t(cx, "plan_you"),
            screen: t(cx, "plan_screen"),
        };
        let page = cx.entity().downgrade();
        let stage_page = page.clone();
        let surface = theme.surface;

        div()
            .id("room-plan")
            .track_focus(&self.plan_focus)
            .role(Role::Group)
            .aria_label(t(cx, "plan_label"))
            .relative()
            .flex_1()
            .min_h(px(240.0))
            .rounded(px(sz::R_LG))
            .bg(colors.bg)
            .border_1()
            .border_color(theme.separator)
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        // The stage's size, for the frame the pins use on
                        // the next render; a change draws them again.
                        let w = f32::from(bounds.size.width);
                        let h = f32::from(bounds.size.height);
                        let _ = stage_page.update(cx, |p, cx| {
                            if (p.stage.0 - w).abs() > 0.5 || (p.stage.1 - h).abs() > 0.5 {
                                p.stage = (w, h);
                                cx.notify();
                            }
                        });
                        window.insert_hitbox(bounds, HitboxBehavior::Normal)
                    },
                    move |bounds, hitbox, window, cx| {
                        paint_scene(&scene, bounds, &hitbox, window, cx);
                        listen(page, scene.vb, bounds, hitbox, window);
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(pins)
            .child(
                div()
                    .absolute()
                    .bottom(px(sz::S3))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px(px(10.0))
                            .py(px(3.0))
                            .rounded_full()
                            .bg(surface.opacity(0.85))
                            .border_1()
                            .border_color(theme.separator)
                            .text_size(px(sz::T_XS))
                            .text_color(theme.text_3)
                            .child(t(cx, "plan_help")),
                    ),
            )
            .into_any_element()
    }

    /// A press on the plan: grab what is under it, or let go of the
    /// selection on the empty floor. A double-click on a wall drops a
    /// corner exactly there.
    fn press(&mut self, p: Vec2, k: f32, clicks: usize, cx: &mut Context<Self>) {
        let room = self.lueur.read(cx).config.room.clone();
        let screen_selected = self.other == Some(Other::Screen);
        let off = |q: Vec2| p - q;
        match plan::hit(&room, p, k, screen_selected) {
            Some(Target::Light(id)) => {
                let Some(light) = room.lights.iter().find(|l| l.id == id) else {
                    return;
                };
                let off = off(light.pos);
                self.select_light(id.clone(), cx);
                self.begin(Drag::Light { id, off });
            }
            Some(Target::Corner(index)) => {
                self.select_other(Other::Corner(index), cx);
                self.begin(Drag::Corner {
                    index,
                    off: off(room.walls[index]),
                });
            }
            Some(Target::Width) => self.begin(Drag::Width),
            Some(Target::Screen) => {
                self.select_other(Other::Screen, cx);
                self.begin(Drag::Screen {
                    off: off(room.screen.center),
                });
            }
            Some(Target::Seat) => self.begin(Drag::Seat),
            Some(Target::Wall) if clicks >= 2 => self.insert_corner_at(p, cx),
            _ => self.clear_selection(false, cx),
        }
        cx.notify();
    }

    fn begin(&mut self, drag: Drag) {
        // The view is held still during a drag, or the plan would slide
        // under the pointer.
        self.held = Some(self.drawn);
        self.drag = Some(drag);
    }

    fn drag_to(&mut self, p: Vec2, shift: bool, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        self.lueur.update(cx, |l, cx| {
            l.edit(cx, |c| {
                let s = &mut c.room.screen;
                match &drag {
                    Drag::Light { id, off } => {
                        if let Some(light) = c.room.lights.iter_mut().find(|x| x.id == *id) {
                            light.pos = plan::clamp_p(p - *off);
                        }
                    }
                    Drag::Screen { off } => s.center = plan::clamp_p(p - *off),
                    Drag::Corner { index, off } => {
                        if let Some(w) = c.room.walls.get_mut(*index) {
                            *w = plan::clamp_p(p - *off);
                        }
                    }
                    Drag::Seat => s.facing_deg = plan::facing_toward(s.center, p, shift),
                    Drag::Width => s.width = plan::width_toward(s.center, p),
                }
            })
        });
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        // Moving a light, the screen or the seat is arranging the room.
        if matches!(drag, Drag::Light { .. } | Drag::Seat | Drag::Screen { .. }) {
            self.mark_arranged(cx);
        }
        self.held = None;
        let snap = super::history::Snap::of(&self.lueur.read(cx).config);
        self.observe_plan(&snap, cx);
        cx.notify();
    }

    fn hover_at(&mut self, p: Vec2, k: f32, cx: &mut Context<Self>) {
        let screen_selected = self.other == Some(Other::Screen);
        let hover = plan::hit(&self.lueur.read(cx).config.room, p, k, screen_selected);
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    /// Drop a corner on the wall nearest `p`, and select it.
    fn insert_corner_at(&mut self, p: Vec2, cx: &mut Context<Self>) {
        let walls = &self.lueur.read(cx).config.room.walls;
        let Some((i, q, _)) = plan::nearest_wall(walls, p) else {
            return;
        };
        self.lueur.update(cx, |l, cx| {
            l.edit(cx, |c| c.room.walls.insert(i + 1, plan::clamp_p(q)))
        });
        self.select_other(Other::Corner(i + 1), cx);
    }
}

/// The canvas's presses, drags and pointer moves, for the frame painted.
fn listen(
    page: WeakEntity<RoomPage>,
    vb: ViewBox,
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    window: &mut Window,
) {
    let frame = Frame::new(
        vb,
        (f32::from(bounds.origin.x), f32::from(bounds.origin.y)),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    );
    let at = move |p: Point<Pixels>| frame.to_plan(f32::from(p.x), f32::from(p.y));
    {
        let page = page.clone();
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || e.button != MouseButton::Left
                || !hitbox.is_hovered(window)
            {
                return;
            }
            let p = at(e.position);
            let clicks = e.click_count;
            let grabbed = page
                .update(cx, |page, cx| {
                    page.press(p, frame.k, clicks, cx);
                    page.drag.is_some()
                })
                .unwrap_or(false);
            if grabbed {
                // The drag follows the pointer outside the stage too.
                window.capture_pointer(hitbox.id);
            }
        });
    }
    {
        let page = page.clone();
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let p = at(e.position);
            let hovered = hitbox.is_hovered(window);
            let _ = page.update(cx, |page, cx| {
                if page.drag.is_some() {
                    page.drag_to(p, e.modifiers.shift, cx);
                } else if hovered {
                    page.hover_at(p, frame.k, cx);
                } else if page.hover.is_some() {
                    page.hover = None;
                    cx.notify();
                }
            });
        });
    }
    window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        let _ = page.update(cx, |page, cx| page.release(cx));
    });
}

// --- Painting ---

fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

/// A filled circle.
fn disc(window: &mut Window, (x, y): (f32, f32), r: f32, color: Hsla) {
    window.paint_quad(
        fill(
            Bounds::new(at(x - r, y - r), size(px(2.0 * r), px(2.0 * r))),
            color,
        )
        .corner_radii(px(r)),
    );
}

/// A circle of radius `r` with a stroke of `w` centred on its edge, as SVG
/// draws one.
fn ring(window: &mut Window, (x, y): (f32, f32), r: f32, inside: Hsla, w: f32, stroke: Hsla) {
    let o = r + w / 2.0;
    window.paint_quad(
        fill(
            Bounds::new(at(x - o, y - o), size(px(2.0 * o), px(2.0 * o))),
            inside,
        )
        .corner_radii(px(o))
        .border_widths(px(w))
        .border_color(stroke),
    );
}

fn line(
    window: &mut Window,
    points: &[(f32, f32)],
    closed: bool,
    width: f32,
    dash: Option<&[f32]>,
    color: Hsla,
) {
    let mut b = PathBuilder::stroke(px(width));
    if let Some(dash) = dash {
        let dash: Vec<Pixels> = dash.iter().map(|d| px(*d)).collect();
        b = b.dash_array(&dash);
    }
    let pts: Vec<Point<Pixels>> = points.iter().map(|(x, y)| at(*x, *y)).collect();
    b.add_polygon(&pts, closed);
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

fn polygon(window: &mut Window, points: &[(f32, f32)], color: Hsla) {
    if points.len() < 3 {
        return;
    }
    let mut b = PathBuilder::fill();
    let pts: Vec<Point<Pixels>> = points.iter().map(|(x, y)| at(*x, *y)).collect();
    b.add_polygon(&pts, true);
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

/// Text centred on `(x, y)`, with a halo of the floor's colour around it
/// so it reads over lines and pools, as the SVG's `paint-order: stroke`.
#[allow(clippy::too_many_arguments)]
fn text(
    window: &mut Window,
    cx: &mut App,
    s: &SharedString,
    (x, y): (f32, f32),
    font_size: f32,
    weight: FontWeight,
    color: Hsla,
    halo: Option<Hsla>,
) {
    if s.is_empty() {
        return;
    }
    let mut f = font(FONT);
    f.weight = weight;
    let shape = |color: Hsla, window: &mut Window| {
        let run = TextRun {
            len: s.len(),
            font: f.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(s.clone(), px(font_size), &[run], None)
    };
    let main = shape(color, window);
    let lh = font_size * 1.25;
    let ox = x - f32::from(main.width) / 2.0;
    let oy = y - lh / 2.0;
    if let Some(halo) = halo {
        let back = shape(halo, window);
        for (dx, dy) in [
            (-1.5, 0.0),
            (1.5, 0.0),
            (0.0, -1.5),
            (0.0, 1.5),
            (-1.1, -1.1),
            (1.1, -1.1),
            (-1.1, 1.1),
            (1.1, 1.1),
        ] {
            let _ = back.paint(
                at(ox + dx, oy + dy),
                px(lh),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
    let _ = main.paint(at(ox, oy), px(lh), TextAlign::Left, None, window, cx);
}

/// The pool a light casts on the floor, in its zone's colour: discs from
/// the edge in, each a little more opaque, standing for the SVG's radial
/// gradient (0.55 at the centre, 0.16 halfway, nothing at the edge).
fn pool(window: &mut Window, c: (f32, f32), radius: f32, color: Hsla, opacity: f32) {
    const STEPS: usize = 14;
    let target = |t: f32| {
        let a = if t <= 0.55 {
            0.55 + (0.16 - 0.55) * t / 0.55
        } else {
            0.16 * (1.0 - (t - 0.55) / 0.45)
        };
        a.max(0.0) * opacity
    };
    let mut so_far = 0.0_f32;
    for j in (1..=STEPS).rev() {
        let t = j as f32 / STEPS as f32;
        let want = target(t - 0.5 / STEPS as f32);
        // Over what the outer discs already laid down.
        let alpha = if so_far < 1.0 {
            ((want - so_far) / (1.0 - so_far)).max(0.0)
        } else {
            0.0
        };
        disc(window, c, radius * t, color.opacity(alpha));
        so_far = want;
    }
}

fn paint_scene(
    s: &Scene,
    bounds: Bounds<Pixels>,
    hitbox: &Hitbox,
    window: &mut Window,
    cx: &mut App,
) {
    let frame = Frame::new(
        s.vb,
        (f32::from(bounds.origin.x), f32::from(bounds.origin.y)),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    );
    let k = frame.k;
    let c = &s.colors;
    let th = &s.theme;
    let room = &s.room;
    let scr = &room.screen;
    let to = |p: Vec2| frame.to_px(p);

    let cursor = if s.dragging {
        CursorStyle::ClosedHand
    } else {
        match s.hover {
            Some(Target::Width) => CursorStyle::ResizeLeftRight,
            Some(Target::Wall) => CursorStyle::Crosshair,
            Some(_) => CursorStyle::OpenHand,
            None => CursorStyle::Arrow,
        }
    };
    window.set_cursor_style(cursor, hitbox);

    window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
        // The dotted grid, every 25 units, anchored to the plan.
        let step = 25.0;
        let x0 = ((s.vb.x - 12.5) / step).floor() * step + 12.5;
        let y0 = ((s.vb.y - 12.5) / step).floor() * step + 12.5;
        let mut y = y0;
        while y <= s.vb.y + s.vb.h {
            let mut x = x0;
            while x <= s.vb.x + s.vb.w {
                disc(window, to(Vec2::new(x, y)), 1.1, c.grid);
                x += step;
            }
            y += step;
        }

        // The floor with a faint shadow under it, then the light falling
        // on it, then the walls on top.
        let walls: Vec<(f32, f32)> = room.walls.iter().map(|p| to(*p)).collect();
        let shadow: Vec<(f32, f32)> = walls.iter().map(|(x, y)| (*x, y + 3.0)).collect();
        polygon(window, &shadow, gpui::black().opacity(0.06));
        polygon(window, &walls, c.floor);
        for (light, m) in room.lights.iter().zip(&s.maps) {
            if light.enabled {
                let r = (70.0 + 70.0 * m.spread) / k;
                pool(window, to(light.pos), r, c.zone(m.edge), 0.5 + 0.5 * m.gain);
            }
        }
        let (left, right) = scr.endpoints();
        let seat = plan::seat(scr);
        polygon(
            window,
            &[to(seat), to(left), to(right)],
            th.text.opacity(0.04),
        );
        if walls.len() >= 2 {
            line(window, &walls, true, 5.0, None, c.wall);
        }

        // Which part of the screen each light looks at.
        for (light, m) in room.lights.iter().zip(&s.maps) {
            let a = plan::anchor_on_screen(scr, light.pos);
            let opacity = if light.enabled {
                0.25 + 0.45 * m.gain
            } else {
                0.12
            };
            line(
                window,
                &[to(light.pos), to(a)],
                false,
                1.5,
                Some(&[2.0, 5.0]),
                c.zone(m.edge).opacity(opacity),
            );
        }

        // The seat: dragged around, it turns the screen.
        line(
            window,
            &[to(scr.center), to(seat)],
            false,
            1.25,
            Some(&[4.0, 5.0]),
            th.text_3,
        );
        let seat_px = to(seat);
        let seat_ink = if s.hover == Some(Target::Seat) {
            th.text
        } else {
            th.text_2
        };
        ring(window, seat_px, 7.0, c.floor, 2.0, seat_ink);
        text(
            window,
            cx,
            &s.you,
            (seat_px.0, seat_px.1 + 18.0),
            12.0,
            FontWeight::MEDIUM,
            th.text_3,
            Some(c.floor),
        );

        // The screen, amber while selected: the plan's selection is the one
        // place amber marks something other than light (see DESIGN.md).
        let selected_screen = s.other == Some(Other::Screen);
        let body = if selected_screen { th.active } else { c.wall };
        let (lp, rp) = (to(left), to(right));
        line(window, &[lp, rp], false, 7.0, None, body);
        disc(window, lp, 3.5, body);
        disc(window, rp, 3.5, body);
        if selected_screen {
            for p in [lp, rp] {
                ring(window, p, 6.0, th.surface, 2.0, th.active);
            }
        }
        let f = scr.facing();
        let sc = to(scr.center);
        text(
            window,
            cx,
            &s.screen,
            (sc.0 + f.x * 20.0, sc.1 + f.y * 20.0),
            12.0,
            FontWeight::MEDIUM,
            th.text_3,
            Some(c.floor),
        );

        // Corners.
        for (i, w) in room.walls.iter().enumerate() {
            let (x, y) = to(*w);
            let selected = s.other == Some(Other::Corner(i));
            let hovered = s.hover == Some(Target::Corner(i));
            let (inside, edge) = if selected {
                (th.active, th.active)
            } else if hovered {
                (th.surface_hover, c.wall)
            } else {
                (c.floor, c.wall)
            };
            window.paint_quad(
                fill(
                    Bounds::new(at(x - 5.75, y - 5.75), size(px(11.5), px(11.5))),
                    inside,
                )
                .corner_radii(px(2.5))
                .border_widths(px(1.5))
                .border_color(edge),
            );
        }

        // The lights on top: a ring when selected (fainter under the
        // pointer), the pin in its zone's colour with the zone's letter on
        // it, and the name.
        for (light, m) in room.lights.iter().zip(&s.maps) {
            let p = to(light.pos);
            let selected = s.selected.as_deref() == Some(light.id.as_str());
            let hovered = s.hover == Some(Target::Light(light.id.clone()));
            if selected || hovered {
                let a = if selected { 1.0 } else { 0.35 };
                ring(
                    window,
                    p,
                    15.0,
                    gpui::transparent_black(),
                    2.0,
                    th.active.opacity(a),
                );
            }
            disc(window, (p.0, p.1 + 1.0), 12.0, c.pin_shadow);
            let (pin, ink, name_ink) = if light.enabled {
                (c.zone(m.edge), c.zone_ink, th.text)
            } else {
                (th.line_strong, th.text, th.text_3)
            };
            ring(window, p, 9.5, pin, 2.5, c.floor);
            text(
                window,
                cx,
                &s.letters[edge_index(m.edge)],
                p,
                10.5,
                FontWeight::BOLD,
                ink,
                None,
            );
            let dy = if plan::label_below(scr, light.pos, k) {
                20.0
            } else {
                -20.0
            };
            text(
                window,
                cx,
                &SharedString::from(light.name.clone()),
                (p.0, p.1 + dy),
                12.5,
                FontWeight::MEDIUM,
                name_ink,
                Some(c.floor),
            );
        }
    });
}
