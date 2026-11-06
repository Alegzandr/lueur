//! The design tokens (see DESIGN.md), for the light and the dark theme.
//!
//! The values are written in OKLCH, as the stylesheet of the Tauri versions
//! wrote them, and turned into sRGB the way a browser does, so the colours
//! did not change with the moves to GPUI and to Slint. Amber only ever
//! means light; what a choice applies to is neutral ink (`select`). The
//! `.slint` files read them from the `Theme` global, which [`apply`] fills.

use lueur_core::config::Appearance;
use slint::Color;

/// An `oklch(l c h / a)` colour, as the stylesheet writes it.
pub fn oklch(l: f32, c: f32, h: f32, a: f32) -> Color {
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
    let byte = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::from_argb_u8(
        byte(a),
        byte(encode(r)),
        byte(encode(g)),
        byte(encode(b)),
    )
}

/// A neutral grey `oklch(l 0 0 / a)`.
fn grey(l: f32, a: f32) -> Color {
    oklch(l, 0.0, 0.0, a)
}

/// The colours of one appearance.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color,
    pub sidebar: Color,
    pub surface: Color,
    pub surface_2: Color,
    pub surface_hover: Color,
    pub sunken: Color,
    pub line: Color,
    pub line_strong: Color,
    pub text: Color,
    pub text_2: Color,
    pub text_3: Color,
    pub ink: Color,
    pub ink_hover: Color,
    pub on_ink: Color,
    pub glow: Color,
    pub glow_text: Color,
    pub glow_soft: Color,
    /// The amber of something running.
    pub active: Color,
    pub active_text: Color,
    pub active_soft: Color,
    /// The neutral ink of a target or a selection.
    pub select: Color,
    pub ok: Color,
    pub danger: Color,
    pub danger_soft: Color,
    pub material: Color,
    pub material_hover: Color,
    pub separator: Color,
    pub focus: Color,
    pub control_line: Color,
    /// The soft shadow under raised controls.
    pub shadow: Color,
    /// The longer shadow of what floats (tooltips, menus, toasts).
    pub shadow_float: Color,
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
            // Faint on paper: no pure black there (DESIGN.md).
            shadow: grey(0.0, 0.08),
            shadow_float: grey(0.0, 0.12),
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
            shadow: grey(0.0, 0.35),
            shadow_float: grey(0.0, 0.5),
        }
    }

    /// The theme the settings force, or the system's (`system_dark`).
    pub fn resolve(appearance: Appearance, system_dark: bool) -> Self {
        let dark = match appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => system_dark,
        };
        if dark { Theme::dark() } else { Theme::light() }
    }

    /// Hand the colours to a `Theme` global.
    pub fn apply(&self, g: &crate::Theme<'_>) {
        g.set_dark(self.dark);
        g.set_bg(self.bg);
        g.set_sidebar(self.sidebar);
        g.set_surface(self.surface);
        g.set_surface_2(self.surface_2);
        g.set_surface_hover(self.surface_hover);
        g.set_sunken(self.sunken);
        g.set_line(self.line);
        g.set_line_strong(self.line_strong);
        g.set_text(self.text);
        g.set_text_2(self.text_2);
        g.set_text_3(self.text_3);
        g.set_ink(self.ink);
        g.set_ink_hover(self.ink_hover);
        g.set_on_ink(self.on_ink);
        g.set_glow(self.glow);
        g.set_glow_text(self.glow_text);
        g.set_glow_soft(self.glow_soft);
        g.set_active(self.active);
        g.set_active_text(self.active_text);
        g.set_active_soft(self.active_soft);
        g.set_select(self.select);
        g.set_ok(self.ok);
        g.set_danger(self.danger);
        g.set_danger_soft(self.danger_soft);
        g.set_material(self.material);
        g.set_material_hover(self.material_hover);
        g.set_separator(self.separator);
        g.set_focus(self.focus);
        g.set_control_line(self.control_line);
        g.set_shadow(self.shadow);
        g.set_shadow_float(self.shadow_float);
    }
}

/// The window's frame (its title bar) in the appearance the settings
/// force, or the system's.
pub fn frame(ui: &crate::AppWindow, appearance: Appearance) {
    use slint::ComponentHandle;
    use slint::winit_030::{WinitWindowAccessor, winit};
    let theme = match appearance {
        Appearance::System => None,
        Appearance::Light => Some(winit::window::Theme::Light),
        Appearance::Dark => Some(winit::window::Theme::Dark),
    };
    ui.window().with_winit_window(|w| w.set_theme(theme));
}

/// Whether the system shows its windows dark, as Slint reads it.
pub fn system_dark() -> bool {
    use slint::ComponentHandle;
    crate::rt::ui().is_some_and(|ui| {
        ui.global::<crate::Palette>().get_color_scheme() == slint::language::ColorScheme::Dark
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(c: Color) -> [u8; 3] {
        [c.red(), c.green(), c.blue()]
    }

    /// The same sRGB a browser computes for these stylesheet values.
    #[test]
    fn oklch_matches_the_browser() {
        assert_eq!(rgb(grey(1.0, 1.0)), [255, 255, 255]);
        assert_eq!(rgb(grey(0.0, 1.0)), [0, 0, 0]);
        // oklch(0.9 0 0) is #dedede, the light hairline.
        assert_eq!(rgb(grey(0.9, 1.0)), [0xde, 0xde, 0xde]);
        // oklch(0.31 0 0) is #303030, its dark counterpart.
        assert_eq!(rgb(grey(0.31, 1.0)), [0x30, 0x30, 0x30]);
        // An amber: red over green over blue, as the eye expects.
        let [r, g, b] = rgb(oklch(0.76, 0.15, 70.0, 1.0));
        assert!(r > g && g > b, "{r} {g} {b}");
    }

    #[test]
    fn a_forced_appearance_wins_over_the_system() {
        assert!(Theme::resolve(Appearance::Dark, false).dark);
        assert!(!Theme::resolve(Appearance::Light, true).dark);
        assert!(Theme::resolve(Appearance::System, true).dark);
    }
}
