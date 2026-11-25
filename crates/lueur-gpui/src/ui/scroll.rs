//! Smooth wheel scrolling for a box that scrolls up and down.
//!
//! GPUI moves a scrolling box by the wheel's whole step at once, which jumps
//! by several lines. Here each wheel step sets a target, and the box glides
//! there over a few frames, slowing as it arrives. Steps that come in while
//! it glides push the same target further, so a fast spin of the wheel stays
//! one continuous movement. Only the scrolled box is redrawn while it moves,
//! and nothing runs once it has arrived.
//!
//! A touchpad that reports exact pixels already moves smoothly and is
//! followed as is, and so is every scroll when the system asks for reduced
//! motion.

use gpui::{App, Div, ScrollEvent, ScrollHandle, Stateful, Window, point, prelude::*, px};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// How quickly the box closes on its target: after this long it has covered
/// about two thirds of the way, and it settles within about four times this.
const TAU_SECS: f32 = 0.06;

/// The box is where it should be once it is less than this far away.
const SETTLED_PX: f32 = 0.5;

/// The scroll state of one box: keep it in the view that draws the box, so it
/// lasts from one frame to the next.
#[derive(Clone, Default)]
pub struct SmoothScroll {
    handle: ScrollHandle,
    glide: Rc<RefCell<Glide>>,
}

#[derive(Default)]
struct Glide {
    /// Where the box is heading, as GPUI's offset: zero at the top, more
    /// negative further down.
    target: f32,
    /// The offset written on the last frame, to notice when something else
    /// (a keyboard focus brought into view) moved the box meanwhile.
    last: f32,
    /// When the last frame was drawn.
    at: Option<Instant>,
    running: bool,
}

impl SmoothScroll {
    fn wheel(&self, event: &ScrollEvent, window: &mut Window, cx: &mut App) {
        let delta = event.delta.pixel_delta(window.line_height()).y;
        let delta = f32::from(delta);
        if delta == 0.0 {
            return;
        }
        let max = f32::from(self.handle.max_offset().y);
        let offset = self.handle.offset();
        let now = f32::from(offset.y);
        let mut glide = self.glide.borrow_mut();
        let from = if glide.running { glide.target } else { now };
        let target = (from + delta).clamp(-max, 0.0);
        if target == from && target == now {
            // Already at the edge: let a box around this one scroll instead.
            return;
        }
        cx.stop_propagation();

        if event.delta.precise() || cx.reduce_motion() {
            glide.running = false;
            self.handle.set_offset(point(offset.x, px(target)));
            window.refresh();
            return;
        }
        glide.target = target;
        if !glide.running {
            glide.running = true;
            glide.last = now;
            glide.at = Some(Instant::now());
            drop(glide);
            self.next_frame(window);
        }
    }

    fn next_frame(&self, window: &mut Window) {
        let this = self.clone();
        window.on_next_frame(move |window, _| this.step(window));
    }

    fn step(&self, window: &mut Window) {
        let mut glide = self.glide.borrow_mut();
        if !glide.running {
            return;
        }
        let offset = self.handle.offset();
        let now = f32::from(offset.y);
        if (now - glide.last).abs() > SETTLED_PX {
            // Something else moved the box: it wins.
            glide.running = false;
            return;
        }
        let max = f32::from(self.handle.max_offset().y);
        glide.target = glide.target.clamp(-max, 0.0);
        let at = Instant::now();
        let dt = glide.at.map_or(0.016, |t| (at - t).as_secs_f32());
        glide.at = Some(at);

        let mut y = now + (glide.target - now) * (1.0 - (-dt / TAU_SECS).exp());
        if (glide.target - y).abs() < SETTLED_PX {
            y = glide.target;
            glide.running = false;
        }
        glide.last = y;
        let running = glide.running;
        drop(glide);
        self.handle.set_offset(point(offset.x, px(y)));
        window.refresh();
        if running {
            self.next_frame(window);
        }
    }
}

/// Lets a box scroll up and down, gliding with the wheel.
pub trait SmoothScrollY {
    fn smooth_scroll_y(self, scroll: &SmoothScroll) -> Self;
}

impl SmoothScrollY for Stateful<Div> {
    fn smooth_scroll_y(self, scroll: &SmoothScroll) -> Self {
        let scroll = scroll.clone();
        // Overflow hidden, not scroll: GPUI's own wheel handling stays out of
        // the way, and the tracked handle still offsets the content.
        self.overflow_y_hidden()
            .track_scroll(&scroll.handle)
            .on_scroll(move |event, window, cx| scroll.wheel(event, window, cx))
    }
}
