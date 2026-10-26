//! The colours the shows play in.

use crate::color::Rgb;

/// The colour show's combinations: three colours each, laid across the
/// lights left to right, that sit well together on a bulb and still mix into
/// a colour on the walls (neighbours stay within a quarter turn of the
/// wheel). The show glides from each to the next, looping: every colour
/// moves only a short way round the wheel to its place in the next one, and
/// the last leads back into the first, so the loop has no seam. The
/// yellow-greens are left out, and no glide passes through them: chartreuse
/// and lime turn sickly on a bulb.
pub(super) const COMBOS: &[[Rgb; 3]] = &[
    // Embers: gold, orange, red.
    [
        Rgb::new(255, 160, 0),
        Rgb::new(255, 70, 0),
        Rgb::new(255, 0, 40),
    ],
    // Dusk: orange, coral, raspberry.
    [
        Rgb::new(255, 100, 0),
        Rgb::new(255, 20, 50),
        Rgb::new(235, 0, 150),
    ],
    // Flamingo: coral, hot pink, violet.
    [
        Rgb::new(255, 30, 60),
        Rgb::new(255, 0, 140),
        Rgb::new(170, 0, 255),
    ],
    // Neon: hot pink, violet, azure.
    [
        Rgb::new(255, 0, 110),
        Rgb::new(130, 0, 255),
        Rgb::new(0, 150, 255),
    ],
    // Twilight: magenta, indigo, blue.
    [
        Rgb::new(220, 0, 230),
        Rgb::new(90, 20, 255),
        Rgb::new(0, 110, 255),
    ],
    // Lagoon: violet, blue, cyan.
    [
        Rgb::new(120, 0, 255),
        Rgb::new(20, 60, 255),
        Rgb::new(0, 200, 240),
    ],
    // Ocean: blue, azure, turquoise.
    [
        Rgb::new(30, 50, 255),
        Rgb::new(0, 140, 255),
        Rgb::new(0, 230, 190),
    ],
    // Aurora: azure, turquoise, emerald.
    [
        Rgb::new(0, 170, 255),
        Rgb::new(0, 230, 170),
        Rgb::new(0, 255, 110),
    ],
    // Glacier: indigo, azure, cyan.
    [
        Rgb::new(90, 40, 255),
        Rgb::new(0, 150, 255),
        Rgb::new(0, 220, 220),
    ],
    // Orchid: purple, indigo, azure.
    [
        Rgb::new(200, 0, 255),
        Rgb::new(90, 40, 255),
        Rgb::new(0, 130, 255),
    ],
    // Berry: raspberry, magenta, violet.
    [
        Rgb::new(255, 0, 90),
        Rgb::new(200, 0, 220),
        Rgb::new(100, 20, 255),
    ],
    // Coral: orange, red, hot pink.
    [
        Rgb::new(255, 70, 0),
        Rgb::new(255, 10, 40),
        Rgb::new(255, 0, 150),
    ],
];

/// From a low amber sun to the last rose and plum of the sky, and back.
pub(super) const SUNSET: &[Rgb] = &[
    Rgb::new(255, 140, 10),
    Rgb::new(255, 85, 10),
    Rgb::new(255, 45, 35),
    Rgb::new(255, 30, 105),
    Rgb::new(190, 30, 180),
    Rgb::new(255, 40, 70),
];

/// The sea from its depths to a breaking crest.
pub(super) const OCEAN: &[Rgb] = &[
    Rgb::new(20, 35, 255),
    Rgb::new(0, 105, 255),
    Rgb::new(0, 185, 235),
    Rgb::new(0, 235, 175),
];

/// The music's harmonies: bolder than the rainbow's, three colours each that
/// still sit well side by side. Every way round a harmony steers clear of the
/// yellow-greens, so even its drift between beats stays clean.
pub(crate) const PARTY: &[[Rgb; 3]] = &[
    // Neon: hot pink, violet, cyan.
    [
        Rgb::new(255, 0, 110),
        Rgb::new(130, 0, 255),
        Rgb::new(0, 190, 255),
    ],
    // Fire: red, orange, magenta.
    [
        Rgb::new(255, 10, 0),
        Rgb::new(255, 110, 0),
        Rgb::new(255, 0, 160),
    ],
    // Synthwave: magenta, blue, orange.
    [
        Rgb::new(255, 0, 200),
        Rgb::new(30, 40, 255),
        Rgb::new(255, 90, 0),
    ],
    // Deep sea: blue, turquoise, violet.
    [
        Rgb::new(0, 60, 255),
        Rgb::new(0, 230, 190),
        Rgb::new(150, 0, 255),
    ],
    // Aurora: emerald, azure, violet.
    [
        Rgb::new(0, 255, 120),
        Rgb::new(0, 170, 255),
        Rgb::new(120, 0, 255),
    ],
];

/// The music's harmonies for a slow or gentle track (a ballad, a band
/// without drums): warm and a little pale, the ambers and powdered roses a
/// lighting desk keeps for a quiet song. Neon is for a track that earns it.
/// Like the party's, every way round them steers clear of the yellow-greens.
pub(crate) const SOFT: &[[Rgb; 3]] = &[
    // Amber: amber, rose, gold.
    [
        Rgb::new(255, 120, 20),
        Rgb::new(255, 70, 100),
        Rgb::new(255, 165, 60),
    ],
    // Powder: powdered rose, peach, lavender.
    [
        Rgb::new(255, 110, 150),
        Rgb::new(255, 150, 100),
        Rgb::new(185, 130, 255),
    ],
    // Dusk: coral, plum, amber.
    [
        Rgb::new(255, 90, 60),
        Rgb::new(200, 60, 180),
        Rgb::new(255, 140, 40),
    ],
    // Moonlight: lavender, soft blue, rose.
    [
        Rgb::new(160, 140, 255),
        Rgb::new(110, 160, 255),
        Rgb::new(255, 130, 185),
    ],
];

/// A candle at its dimmest (reddest) and brightest (yellowest).
pub(super) const EMBER: Rgb = Rgb::new(255, 78, 4);
pub(super) const FLAME: Rgb = Rgb::new(255, 152, 42);
