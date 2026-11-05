//! The design tokens (see DESIGN.md), for the light and the dark theme.
//!
//! The values are written in OKLCH, as the stylesheet of the Tauri versions
//! wrote them, and turned into sRGB the way a browser does, so the colours
//! did not change with the move to GPUI. Amber only ever means light; what a
//! choice applies to is neutral ink (`select`).

use gpui::{App, Global, Hsla, Rgba, WindowAppearance};
use lueur_core::config::Appearance;

/// An `oklch(l c h / a)` colour, as the stylesheet writes it.
pub fn oklch(l: f32, c: f32, h: f32, a: f32) -> Hsla {
    let h = h.to_radians();
    let (ok_a, ok_b) = (c * h.cos(), c * h.sin());
    // OKLab to linear sRGB (Björn Ottosson's matrices, as CSS Color 4).
    let l_ = l + 0.396_337_78 * ok_a + 0.215_803_76 * ok_b;
    let m_ = l - 0.105_561_346 * ok_a - 0.063_854_17 * ok_b;
    let s_ = l - 0.089_484_18 * ok_a - 1.291_485_5 * ok_b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3;
    let b = -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    // Linear to gamma-encoded sRGB, clipped to the gamut as a browser does.
    let encode = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.003_130_8 {
            12.92 * x
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        }
    };
    Rgba {
        r: encode(r),
        g: encode(g),
        b: encode(b),
        a,
    }
    .into()
}

/// A neutral grey `oklch(l 0 0 / a)`.
fn grey(l: f32, a: f32) -> Hsla {
    oklch(l, 0.0, 0.0, a)
}

/// The colours of one appearance.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: Hsla,
    pub sidebar: Hsla,
    pub surface: Hsla,
    pub surface_2: Hsla,
    pub surface_hover: Hsla,
    pub sunken: Hsla,
    pub line: Hsla,
    pub line_strong: Hsla,
    pub text: Hsla,
    pub text_2: Hsla,
    pub text_3: Hsla,
    pub ink: Hsla,
    pub ink_hover: Hsla,
    pub on_ink: Hsla,
    pub glow: Hsla,
    pub glow_text: Hsla,
    pub glow_soft: Hsla,
    /// The amber of something running.
    pub active: Hsla,
    pub active_text: Hsla,
    pub active_soft: Hsla,
    /// The neutral ink of a target or a selection.
    pub select: Hsla,
    pub ok: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub material: Hsla,
    pub material_hover: Hsla,
    pub separator: Hsla,
    pub focus: Hsla,
    pub control_line: Hsla,
}

impl Theme {
    pub fn light() -> Self {
        Theme {
            dark: false,
            bg: grey(0.972, 1.0),
            sidebar: grey(0.948, 1.0),
            surface: grey(1.0, 1.0),
            surface_2: grey(0.962, 1.0),
            surface_hover: grey(0.935, 1.0),
            sunken: grey(0.955, 1.0),
            line: grey(0.9, 1.0),
            line_strong: grey(0.83, 1.0),
            text: grey(0.19, 1.0),
            text_2: grey(0.4, 1.0),
            text_3: grey(0.49, 1.0),
            ink: grey(0.27, 1.0),
            ink_hover: grey(0.21, 1.0),
            on_ink: grey(0.99, 1.0),
            glow: oklch(0.76, 0.15, 70.0, 1.0),
            glow_text: oklch(0.52, 0.12, 62.0, 1.0),
            glow_soft: oklch(0.76, 0.15, 70.0, 0.16),
            active: oklch(0.58, 0.13, 62.0, 1.0),
            active_text: oklch(0.5, 0.12, 60.0, 1.0),
            active_soft: oklch(0.76, 0.15, 70.0, 0.16),
            select: grey(0.34, 1.0),
            ok: oklch(0.58, 0.14, 150.0, 1.0),
            danger: oklch(0.55, 0.19, 27.0, 1.0),
            danger_soft: oklch(0.55, 0.19, 27.0, 0.1),
            material: grey(1.0, 0.7),
            material_hover: grey(1.0, 0.86),
            separator: grey(0.0, 0.075),
            focus: oklch(0.6, 0.13, 64.0, 1.0),
            control_line: grey(0.62, 1.0),
        }
    }

    pub fn dark() -> Self {
        Theme {
            dark: true,
            bg: grey(0.205, 1.0),
            sidebar: grey(0.18, 1.0),
            surface: grey(0.24, 1.0),
            surface_2: grey(0.26, 1.0),
            surface_hover: grey(0.29, 1.0),
            sunken: grey(0.18, 1.0),
            line: grey(0.31, 1.0),
            line_strong: grey(0.4, 1.0),
            text: grey(0.95, 1.0),
            text_2: grey(0.77, 1.0),
            text_3: grey(0.66, 1.0),
            ink: grey(0.93, 1.0),
            ink_hover: grey(1.0, 1.0),
            on_ink: grey(0.2, 1.0),
            glow: oklch(0.8, 0.14, 72.0, 1.0),
            glow_text: oklch(0.83, 0.13, 75.0, 1.0),
            glow_soft: oklch(0.8, 0.14, 72.0, 0.16),
            active: oklch(0.8, 0.14, 72.0, 1.0),
            active_text: oklch(0.83, 0.13, 75.0, 1.0),
            active_soft: oklch(0.8, 0.14, 72.0, 0.14),
            select: grey(0.9, 1.0),
            ok: oklch(0.76, 0.13, 150.0, 1.0),
            danger: oklch(0.72, 0.16, 27.0, 1.0),
            danger_soft: oklch(0.72, 0.16, 27.0, 0.15),
            material: grey(0.24, 0.62),
            material_hover: grey(0.29, 0.7),
            separator: grey(1.0, 0.07),
            focus: oklch(0.8, 0.14, 72.0, 1.0),
            control_line: grey(0.54, 1.0),
        }
    }

    /// The theme the settings force, or the system's.
    pub fn resolve(appearance: Appearance, system: WindowAppearance) -> Self {
        let dark = match appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => matches!(
                system,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        if dark { Theme::dark() } else { Theme::light() }
    }
}

impl Global for Theme {}

/// The theme in use.
pub fn theme(cx: &App) -> &Theme {
    cx.global::<Theme>()
}

/// Sizes and spaces of the stylesheet (`--s-*`, `--r-*`, `--t-*`), in pixels
/// at a 16px root.
pub mod size {
    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
    pub const S5: f32 = 20.0;
    pub const S6: f32 = 28.0;
    pub const R_SM: f32 = 5.0;
    pub const R_MD: f32 = 8.0;
    pub const R_LG: f32 = 14.0;
    pub const T_XS: f32 = 11.0;
    pub const T_SM: f32 = 12.0;
    pub const T_MD: f32 = 13.0;
    pub const T_LG: f32 = 15.0;
    pub const T_XL: f32 = 28.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: Hsla) -> [u8; 3] {
        let c = c.to_rgb();
        [c.r, c.g, c.b].map(|x| (x * 255.0).round() as u8)
    }

    /// The same sRGB a browser computes for these stylesheet values.
    #[test]
    fn oklch_matches_the_browser() {
        assert_eq!(rgb(grey(1.0, 1.0)), [255, 255, 255]);
        assert_eq!(rgb(grey(0.0, 1.0)), [0, 0, 0]);
        // oklch(0.9 0 0) is #dedede, the hairline titlebar.rs paints.
        assert_eq!(rgb(grey(0.9, 1.0)), [0xde, 0xde, 0xde]);
        // oklch(0.31 0 0) is #303030, its dark counterpart.
        assert_eq!(rgb(grey(0.31, 1.0)), [0x30, 0x30, 0x30]);
        // An amber: red over green over blue, as the eye expects.
        let [r, g, b] = rgb(oklch(0.76, 0.15, 70.0, 1.0));
        assert!(r > g && g > b, "{r} {g} {b}");
    }

    #[test]
    fn a_forced_appearance_wins_over_the_system() {
        assert!(Theme::resolve(Appearance::Dark, WindowAppearance::Light).dark);
        assert!(!Theme::resolve(Appearance::Light, WindowAppearance::Dark).dark);
        assert!(Theme::resolve(Appearance::System, WindowAppearance::VibrantDark).dark);
    }
}
