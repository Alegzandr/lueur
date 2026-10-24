//! Light shows that need no screen: a fade through a palette, a candle, and
//! lights that dance to the music.
//!
//! Everything here is pure: given the time (and, for music, the sound levels
//! the shell measured) it says what every light shows. The shell streams the
//! result to the bulbs exactly as it streams the ambilight.

use crate::color::{BulbOutput, MAX_BRIGHT, MIN_BRIGHT, Rgb, full_chroma};
use serde::{Deserialize, Serialize};

/// The light shows. Each is a finished recipe: picking one is the only
/// decision, so there is nothing to tune and nothing to get wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShowKind {
    /// Colours change on the beat, brightness follows the loudness.
    #[default]
    Music,
    /// The whole colour wheel, slowly, a different colour on each light.
    Rainbow,
    /// Reds, oranges and pinks drifting into one another.
    Sunset,
    /// Blues and teals.
    Ocean,
    /// A warm flame on every light, flickering on its own.
    Candle,
}

pub const SHOWS: [ShowKind; 5] = [
    ShowKind::Music,
    ShowKind::Rainbow,
    ShowKind::Sunset,
    ShowKind::Ocean,
    ShowKind::Candle,
];

impl ShowKind {
    pub fn id(self) -> &'static str {
        match self {
            ShowKind::Music => "music",
            ShowKind::Rainbow => "rainbow",
            ShowKind::Sunset => "sunset",
            ShowKind::Ocean => "ocean",
            ShowKind::Candle => "candle",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        SHOWS.into_iter().find(|k| k.id() == id)
    }

    fn recipe(self) -> Recipe {
        const RAINBOW: &[Rgb] = &[
            Rgb::new(255, 0, 0),
            Rgb::new(255, 160, 0),
            Rgb::new(200, 255, 0),
            Rgb::new(0, 255, 120),
            Rgb::new(0, 140, 255),
            Rgb::new(150, 0, 255),
        ];
        const SUNSET: &[Rgb] = &[
            Rgb::new(255, 70, 30),
            Rgb::new(255, 140, 0),
            Rgb::new(255, 40, 120),
            Rgb::new(160, 40, 220),
        ];
        const OCEAN: &[Rgb] = &[
            Rgb::new(0, 90, 255),
            Rgb::new(0, 200, 210),
            Rgb::new(40, 60, 200),
            Rgb::new(0, 230, 150),
        ];
        const PARTY: &[Rgb] = &[
            Rgb::new(255, 0, 80),
            Rgb::new(0, 120, 255),
            Rgb::new(255, 150, 0),
            Rgb::new(170, 0, 255),
            Rgb::new(0, 220, 160),
        ];
        match self {
            ShowKind::Music => Recipe {
                palette: PARTY,
                period: 0.0,
                spread: true,
            },
            ShowKind::Rainbow => Recipe {
                palette: RAINBOW,
                period: 6.0,
                spread: true,
            },
            ShowKind::Sunset => Recipe {
                palette: SUNSET,
                period: 9.0,
                spread: true,
            },
            ShowKind::Ocean => Recipe {
                palette: OCEAN,
                period: 9.0,
                spread: true,
            },
            ShowKind::Candle => Recipe {
                palette: &[],
                period: 0.0,
                spread: false,
            },
        }
    }
}

/// The show the window and the tray start, and how bright it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShowSettings {
    pub kind: ShowKind,
    /// 1..=100.
    pub bright: u8,
}

impl Default for ShowSettings {
    fn default() -> Self {
        ShowSettings {
            kind: ShowKind::default(),
            bright: 80,
        }
    }
}

impl ShowSettings {
    fn bright(&self) -> f32 {
        f32::from(self.bright.clamp(MIN_BRIGHT, MAX_BRIGHT)) / 100.0
    }
}

/// How one show plays.
#[derive(Debug, Clone, Copy)]
struct Recipe {
    /// Colours it goes through, in order.
    palette: &'static [Rgb],
    /// Seconds spent on each colour, for the fades.
    period: f32,
    /// Lights take different colours of the palette at once, rather than all
    /// showing the same one.
    spread: bool,
}

impl Recipe {
    /// The palette, never empty.
    fn colors(&self) -> &'static [Rgb] {
        const FALLBACK: &[Rgb] = &[Rgb::new(255, 147, 41)];
        if self.palette.is_empty() {
            FALLBACK
        } else {
            self.palette
        }
    }
}

/// A bulb output for a colour at a 0..=1 brightness.
fn output(rgb: Rgb, level: f32) -> BulbOutput {
    let pct = (level.clamp(0.0, 1.0) * 100.0).round() as u8;
    BulbOutput::Color {
        rgb: full_chroma(rgb),
        bright: pct.clamp(MIN_BRIGHT, MAX_BRIGHT),
    }
}

/// Hue (degrees), saturation and value of a colour.
fn hsv(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb::new(to(r), to(g), to(b))
}

/// Blend two colours by `t` along the hue circle, the short way round.
/// Mixing in RGB would pass through grey between opposite colours; going
/// round the wheel keeps every step as vivid as the ends.
pub fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let (ha, sa, va) = hsv(a);
    let (hb, sb, vb) = hsv(b);
    // A grey end has no hue of its own: borrow the other's.
    let (ha, hb) = match (sa < 0.05, sb < 0.05) {
        (true, false) => (hb, hb),
        (false, true) => (ha, ha),
        _ => (ha, hb),
    };
    let mut dh = hb - ha;
    if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    from_hsv(ha + dh * t, sa + (sb - sa) * t, va + (vb - va) * t)
}

/// Where light `i` of `n` sits in the palette, as a fraction of one colour.
fn offset(recipe: &Recipe, i: usize, n: usize) -> f32 {
    if recipe.spread && n > 1 {
        // Spread the lights over the whole palette, so the room holds every
        // colour at once and they chase each other round it.
        i as f32 * recipe.colors().len() as f32 / n as f32
    } else {
        0.0
    }
}

/// The colour at `pos` along the palette (1.0 = one colour further on),
/// eased so each colour holds a moment before moving to the next.
fn palette_at(colors: &[Rgb], pos: f32) -> Rgb {
    let len = colors.len();
    let pos = pos.rem_euclid(len as f32);
    let i = pos.floor() as usize % len;
    let f = pos - pos.floor();
    let eased = f * f * (3.0 - 2.0 * f);
    blend(colors[i], colors[(i + 1) % len], eased)
}

/// A palette fade: every light glides through the show's colours.
pub fn fade(settings: &ShowSettings, t: f32, n: usize) -> Vec<BulbOutput> {
    let recipe = settings.kind.recipe();
    let colors = recipe.colors();
    let base = t / recipe.period.max(0.5);
    (0..n)
        .map(|i| {
            output(
                palette_at(colors, base + offset(&recipe, i, n)),
                settings.bright(),
            )
        })
        .collect()
}

/// A small, fast, deterministic random source: enough for a flame.
#[derive(Debug, Clone, Copy)]
struct XorShift(u32);

impl XorShift {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// A candle per light: a warm flame that wavers on its own.
#[derive(Debug, Clone)]
pub struct Candle {
    flames: Vec<Flame>,
    rng: XorShift,
}

#[derive(Debug, Clone, Copy)]
struct Flame {
    level: f32,
    target: f32,
    warmth: f32,
}

impl Candle {
    pub fn new(n: usize) -> Self {
        Candle {
            flames: vec![
                Flame {
                    level: 0.8,
                    target: 0.8,
                    warmth: 0.5,
                };
                n
            ],
            rng: XorShift(0x9e37_79b9),
        }
    }

    /// Advance by `dt` seconds.
    pub fn frame(&mut self, settings: &ShowSettings, dt: f32) -> Vec<BulbOutput> {
        // How often a flame picks a new height, per second.
        let restless = 6.0;
        let ember = Rgb::new(255, 110, 20);
        let flame = Rgb::new(255, 170, 70);
        let bright = settings.bright();
        let rng = &mut self.rng;
        self.flames
            .iter_mut()
            .map(|f| {
                if rng.next() < restless * dt {
                    // Mostly steady, now and then a gust that dips it hard.
                    f.target = if rng.next() < 0.08 {
                        0.35 + 0.2 * rng.next()
                    } else {
                        0.7 + 0.3 * rng.next()
                    };
                    f.warmth = rng.next();
                }
                let k = (dt * 12.0).min(1.0);
                f.level += (f.target - f.level) * k;
                let rgb = blend(ember, flame, f.warmth * f.level);
                output(rgb, f.level * bright)
            })
            .collect()
    }
}

/// Levels measured from one slice of sound.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Levels {
    /// Loudness relative to what has been playing lately, 0..=1.
    pub energy: f32,
    /// A kick or a strong onset just landed.
    pub beat: bool,
}

/// Turns raw samples into [`Levels`]: how loud it is now compared with the
/// last few seconds, and where the beats fall.
///
/// The bass carries the beat, so onsets are read from a low-passed signal;
/// loudness is self-normalising so the show looks the same with the volume
/// at 10% or at 100%.
#[derive(Debug, Clone)]
pub struct Analyzer {
    rate: f32,
    low: f32,
    env: f32,
    bass_avg: f32,
    peak: f32,
    since_beat: f32,
    heard: f32,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        Analyzer {
            rate: sample_rate.max(1) as f32,
            low: 0.0,
            env: 0.0,
            bass_avg: 0.0,
            peak: 1e-4,
            since_beat: 1.0,
            heard: 0.0,
        }
    }

    /// Read the mono samples captured since the last call, covering `dt`
    /// seconds.
    pub fn feed(&mut self, samples: &[f32], dt: f32) -> Levels {
        // One-pole low-pass at ~150 Hz isolates the kick and the bass line.
        let a = 1.0 - (-2.0 * std::f32::consts::PI * 150.0 / self.rate).exp();
        let mut sum = 0.0f32;
        let mut bass = 0.0f32;
        for &s in samples {
            let s = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.low += (s - self.low) * a;
            sum += s * s;
            bass += self.low * self.low;
        }
        let n = samples.len().max(1) as f32;
        let rms = (sum / n).sqrt();
        let bass = (bass / n).sqrt();

        // Fast attack, slower release: lights jump with the music and settle.
        let k = if rms > self.env {
            0.6
        } else {
            (dt * 4.0).min(1.0)
        };
        self.env += (rms - self.env) * k;
        // The loudest of the last few seconds sets the scale.
        self.peak = (self.peak * (1.0 - (dt * 0.15).min(1.0)))
            .max(self.env)
            .max(1e-4);

        // A beat: bass well above its running average, not too soon after
        // the last one. The first half second only learns the level.
        self.since_beat += dt;
        self.heard += dt;
        let beat = self.heard > 0.5
            && bass > self.bass_avg * 1.4
            && bass > self.peak * 0.08
            && self.since_beat > 0.18;
        if beat {
            self.since_beat = 0.0;
        }
        self.bass_avg += (bass - self.bass_avg) * (dt * 2.5).min(1.0);

        Levels {
            energy: (self.env / self.peak).clamp(0.0, 1.0),
            beat,
        }
    }
}

/// Lights that dance: a beat moves them on to the next palette colour with a
/// flash, loudness sets how bright they burn in between.
#[derive(Debug, Clone)]
pub struct Dance {
    /// Palette position each light is gliding towards.
    step: f32,
    shown: f32,
    flash: f32,
    level: f32,
}

impl Default for Dance {
    fn default() -> Self {
        Dance {
            step: 0.0,
            shown: 0.0,
            flash: 0.0,
            level: 0.0,
        }
    }
}

impl Dance {
    pub fn frame(
        &mut self,
        settings: &ShowSettings,
        levels: Levels,
        dt: f32,
        n: usize,
    ) -> Vec<BulbOutput> {
        self.flash *= (-dt * 6.0).exp();
        if levels.beat {
            self.step += 1.0;
            self.flash = 1.0;
        }
        // Colour glides to the new step quickly.
        let glide = (dt * 14.0).min(1.0);
        self.shown += (self.step - self.shown) * glide;
        self.level += (levels.energy - self.level) * (dt * 10.0).min(1.0);

        // Silence keeps a low glow rather than going dark.
        let level = (0.12 + 0.88 * self.level.max(self.flash)) * settings.bright();
        let recipe = ShowKind::Music.recipe();
        let colors = recipe.colors();
        (0..n)
            .map(|i| {
                let pos = self.shown + offset(&recipe, i, n).round();
                output(palette_at(colors, pos), level)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(o: BulbOutput) -> Rgb {
        match o {
            BulbOutput::Color { rgb, .. } => rgb,
            other => panic!("expected a colour, got {other:?}"),
        }
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
    fn fades_spread_colours_over_the_lights() {
        let s = ShowSettings {
            kind: ShowKind::Rainbow,
            bright: 80,
        };
        let out = fade(&s, 0.0, 3);
        assert_ne!(out[0], out[1]);
        assert_ne!(out[1], out[2]);
        assert!(out.iter().all(|o| o.bright() == 80));
    }

    #[test]
    fn fades_move_over_time() {
        let s = ShowSettings {
            kind: ShowKind::Ocean,
            bright: 80,
        };
        assert_ne!(fade(&s, 0.0, 1), fade(&s, 4.0, 1));
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
    fn a_candle_stays_warm() {
        let mut c = Candle::new(2);
        let s = ShowSettings {
            kind: ShowKind::Candle,
            bright: 80,
        };
        for _ in 0..200 {
            for o in c.frame(&s, 0.04) {
                let c = rgb(o);
                assert!(c.r == 255 && c.b < 120, "{c:?}");
            }
        }
    }

    fn tone(rate: u32, freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    #[test]
    fn a_kick_after_quiet_is_a_beat() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let quiet = tone(rate, 60.0, 0.02, 0.04);
        for _ in 0..50 {
            assert!(!a.feed(&quiet, 0.04).beat);
        }
        let kick = tone(rate, 60.0, 0.8, 0.04);
        let l = a.feed(&kick, 0.04);
        assert!(l.beat);
        assert!(l.energy > 0.9);
    }

    #[test]
    fn silence_is_calm() {
        let mut a = Analyzer::new(44_100);
        let l = a.feed(&vec![0.0; 1764], 0.04);
        assert!(!l.beat);
        assert_eq!(l.energy, 0.0);
        let mut d = Dance::default();
        let out = d.frame(&ShowSettings::default(), l, 0.04, 1);
        assert!(out[0].bright() < 20);
    }

    #[test]
    fn a_beat_flashes_and_moves_the_colour_on() {
        let s = ShowSettings::default();
        let mut d = Dance::default();
        let calm = Levels {
            energy: 0.5,
            beat: false,
        };
        let before = rgb(d.frame(&s, calm, 0.04, 1)[0]);
        let flash = d.frame(
            &s,
            Levels {
                energy: 1.0,
                beat: true,
            },
            0.04,
            1,
        )[0];
        assert_eq!(flash.bright(), s.bright);
        let mut after = before;
        for _ in 0..30 {
            after = rgb(d.frame(&s, calm, 0.04, 1)[0]);
        }
        assert_ne!(before, after);
    }
}
