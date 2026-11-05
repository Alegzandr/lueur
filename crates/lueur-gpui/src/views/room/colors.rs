//! The plan's own colours, from `src/styles/app.css` (`--zone-*`, `--plan-*`,
//! `--pin-shadow`, `--bezel`), for both themes. They live here rather than
//! in `theme.rs` because only the Room page uses them; the values are the
//! stylesheet's, and must follow it while both interfaces exist.

use crate::theme::{Theme, oklch};
use gpui::Hsla;
use lueur_core::geometry::ScreenEdge;

#[derive(Debug, Clone, Copy)]
pub struct PlanColors {
    /// Blue, teal and orange: which part of the screen a light mirrors.
    /// They differ in lightness as well as hue, so they stay apart for
    /// colour-blind eyes, and each zone also carries a letter.
    pub zone_left: Hsla,
    pub zone_center: Hsla,
    pub zone_right: Hsla,
    /// The letter on a zone's colour.
    pub zone_ink: Hsla,
    /// The well the plan sits in, its dotted grid, the floor and the walls.
    pub bg: Hsla,
    pub grid: Hsla,
    pub floor: Hsla,
    pub wall: Hsla,
    pub pin_shadow: Hsla,
    /// The screen's frame in a light's preview.
    pub bezel: Hsla,
}

impl PlanColors {
    pub fn of(theme: &Theme) -> Self {
        if theme.dark {
            PlanColors {
                zone_left: oklch(0.64, 0.13, 252.0, 1.0),
                zone_center: oklch(0.74, 0.12, 175.0, 1.0),
                zone_right: oklch(0.87, 0.13, 80.0, 1.0),
                zone_ink: oklch(0.2, 0.0, 0.0, 1.0),
                bg: oklch(0.18, 0.0, 0.0, 1.0),
                grid: oklch(0.28, 0.0, 0.0, 1.0),
                floor: oklch(0.23, 0.0, 0.0, 1.0),
                wall: oklch(0.86, 0.0, 0.0, 1.0),
                pin_shadow: oklch(0.0, 0.0, 0.0, 0.3),
                bezel: oklch(0.2, 0.0, 0.0, 1.0),
            }
        } else {
            PlanColors {
                zone_left: oklch(0.42, 0.14, 262.0, 1.0),
                zone_center: oklch(0.51, 0.1, 175.0, 1.0),
                zone_right: oklch(0.625, 0.145, 58.0, 1.0),
                zone_ink: oklch(1.0, 0.0, 0.0, 1.0),
                bg: oklch(0.955, 0.0, 0.0, 1.0),
                grid: oklch(0.87, 0.0, 0.0, 1.0),
                floor: oklch(1.0, 0.0, 0.0, 1.0),
                wall: oklch(0.5, 0.0, 0.0, 1.0),
                pin_shadow: oklch(0.0, 0.0, 0.0, 0.16),
                bezel: oklch(0.42, 0.0, 0.0, 1.0),
            }
        }
    }

    pub fn zone(&self, edge: ScreenEdge) -> Hsla {
        match edge {
            ScreenEdge::Left => self.zone_left,
            ScreenEdge::Center => self.zone_center,
            ScreenEdge::Right => self.zone_right,
        }
    }
}
