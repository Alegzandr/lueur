//! Playing the stage: what each section strikes, and what each light shows.

use super::fixture::Part;
use super::tempo::FASTEST;
use super::track::{BUILD_SECS, CALM_DRIFT_SECS, LATE_TURN, Mood, OFF_BEAT, Section, WHITE_KELVIN};
use super::{Levels, Stage, mix};
use crate::color::{BulbOutput, Rgb};
use crate::show::{ShowSettings, even, output};
use std::f32::consts::TAU;

impl Stage {
    /// The beat is on: punch it in the current pattern, and answer the snare.
    pub(super) fn groove(&mut self, settings: &ShowSettings, levels: &Levels) {
        if let Some(s) = levels.beat {
            let found = self.mark();
            self.count_beat();
            if let Some(into) = found
                && self.beat != into
            {
                // The phrase began a few beats ago, not where the count
                // has it. The colours move on now if they have not lately.
                if self.since_turn > LATE_TURN {
                    self.turn();
                    self.since_turn = into;
                }
                self.beat = into;
            }
            self.beats = self.beats.wrapping_add(1);
            self.since_beat = 0.0;
            self.feel_half();
            if self.beat.is_multiple_of(4) {
                self.read_bar();
            }
            if let Some(stress) = self.stress() {
                let s = s * stress;
                let pattern = self.pattern();
                let (k, b) = (self.punches, self.step());
                // A phrase opening swells the room as well as the stage.
                let opening = self.beat == 0;
                for lamp in &mut self.lamps {
                    match lamp.part {
                        Part::Punch => {
                            lamp.hit(s * pattern.beat(lamp.rank, k, b), false);
                            // The beat moves on: who holds it now, and who
                            // hands it on.
                            let hold = pattern.hold(lamp.rank, k, b);
                            lamp.holding = hold > 0.0;
                            if lamp.holding {
                                lamp.glow = 1.0;
                                lamp.hold = s * hold;
                            }
                        }
                        // The wash only feels the strongest beats, softly.
                        Part::Wash => {
                            let swell = if opening { 0.4 * s } else { 0.0 };
                            lamp.hit(swell.max(0.25 * (s - 0.6).max(0.0) / 0.4), false);
                        }
                        Part::Accent => {}
                    }
                }
            }
        }
        if let Some(s) = levels.snare {
            self.place_snare(s);
            // In a drop the snare cracks white; elsewhere white is spent.
            let has_accent = self.lamps.iter().any(|l| l.part == Part::Accent);
            let white = self.section == Section::Drop && self.white(settings);
            if has_accent {
                self.accent(s, white);
            } else {
                let (pattern, k, b) = (self.pattern(), self.punches, self.step());
                for lamp in &mut self.lamps {
                    if lamp.part == Part::Punch {
                        let share = pattern.snare(lamp.rank, k, b);
                        lamp.hit(s * share, white && share > 0.0);
                    }
                }
            }
        }
    }

    /// A build: a chase walks across the room, a light a beat, then two,
    /// then four as it climbs, harder and whiter all the way. The snares
    /// crack on the accent.
    pub(super) fn build(&mut self, settings: &ShowSettings, levels: &Levels, dt: f32) {
        let climb = (self.held / BUILD_SECS).min(1.0);
        let per_beat = if climb < 1.0 / 3.0 {
            1.0
        } else if climb < 2.0 / 3.0 {
            2.0
        } else {
            4.0
        };
        self.roll_due -= dt * per_beat / self.period.max(FASTEST);
        if self.roll_due <= 0.0 {
            self.roll_due += 1.0;
            let k = self.punches.max(1);
            let at = self.roll % k;
            self.roll += 1;
            let white = climb > 0.8 && self.white(settings);
            for lamp in &mut self.lamps {
                if lamp.part == Part::Punch && lamp.rank == at {
                    lamp.hit(0.3 + 0.7 * climb, white);
                }
            }
        }
        if let Some(s) = levels.snare {
            self.accent((0.3 + 0.7 * climb) * s, false);
        }
    }

    /// No beat to play. The snare still answers on the accent, and in loud
    /// music (a wall of guitars, a pad swelling up before the kick comes
    /// in) every chord and stab strikes the next light along the room, as
    /// hard as the music is loud: loud music never leaves the room still.
    pub(super) fn calm(&mut self, levels: &Levels) {
        if let Some(s) = levels.snare {
            self.accent(0.5 * s, false);
        }
        let loud = self.loud();
        let Some(s) = levels.snare.or(levels.pulse) else {
            return;
        };
        if loud <= 0.0 {
            return;
        }
        let k = self.punches.max(1);
        let at = self.roll % k;
        self.roll += 1;
        for lamp in &mut self.lamps {
            if lamp.part == Part::Punch && lamp.rank == at {
                lamp.hit(0.7 * loud * s, false);
            }
        }
    }

    pub(super) fn accent(&mut self, amount: f32, white: bool) {
        for lamp in &mut self.lamps {
            if lamp.part == Part::Accent {
                lamp.hit(amount, white);
            }
        }
    }

    /// What each light shows now.
    pub(super) fn light(&self, settings: &ShowSettings) -> Vec<BulbOutput> {
        let (back, hit, alt) = self.roles();
        let colours = [hit, alt, back];
        let pattern = self.pattern();
        let bright = settings.bright();
        let swell = self.swell.clamp(0.0, 1.0).powf(2.5);
        let climb = if self.section == Section::Build {
            (self.held / BUILD_SECS).min(1.0)
        } else {
            0.0
        };
        // A breakdown rests in one deep colour drifting to its other shade,
        // a slow swell rolling across the room; both ease in.
        let calm = self.section == Section::Calm;
        let rest = if calm {
            mix(
                back,
                alt,
                0.5 - 0.5 * (TAU * self.held / CALM_DRIFT_SECS).cos(),
            )
        } else {
            back
        };
        self.lamps
            .iter()
            .map(|lamp| {
                // What it glows at between hits, and the colour it strikes in.
                let (base, strike) = match lamp.part {
                    // From near dark, so every beat lands; a gentle track
                    // keeps a little more glow between its beats.
                    Part::Punch => (
                        match self.mood {
                            Mood::Party => 0.04 + 0.16 * swell,
                            Mood::Soft => 0.1 + 0.2 * swell,
                        },
                        colours[pattern.colour(lamp.rank, self.punches, self.step())],
                    ),
                    Part::Wash => (
                        0.15 + 0.55 * self.bass.clamp(0.0, 1.0).powf(1.5) + 0.15 * swell,
                        mix(rest, alt, lamp.tint),
                    ),
                    Part::Accent => (0.05 + 0.2 * self.shimmer + 0.15 * swell, alt),
                };
                let base = match self.section {
                    // Lights out for the breath before the drop.
                    Section::Hold => 0.0,
                    Section::Calm => {
                        // The louder the music, the deeper the swell, so a
                        // loud passage without a beat still moves.
                        let ease = (self.held / 3.0).min(1.0);
                        let wave = (TAU * (self.wave - 0.5 * lamp.place)).sin();
                        let depth = 0.25 + 0.5 * swell;
                        (0.12 + 0.3 * swell) * (1.0 - depth * ease * (1.0 - wave) / 2.0)
                    }
                    // A build brightens as it climbs.
                    Section::Build => base + (0.5 - base).max(0.0) * climb,
                    Section::Groove | Section::Drop => base,
                };
                // A light holding the beat burns on in its colour between
                // the beats, and the beats punch on top.
                let lit = lamp.flash.max(lamp.glow * lamp.hold);
                let level = base + (1.0 - base) * lit;
                if lamp.white > 0.35 || (climb > 0.85 && lamp.part != Part::Wash) {
                    BulbOutput::White {
                        kelvin: WHITE_KELVIN,
                        bright: ((level * bright * 100.0).round() as u8).max(1),
                    }
                } else {
                    // A hit strikes in its colour and fades back into the
                    // room's; a build drains it towards white as it climbs.
                    let rgb = if calm {
                        rest
                    } else if lamp.part == Part::Wash {
                        strike
                    } else {
                        // A beat off the one strikes less hard, in the
                        // same colour.
                        let struck = lamp.flash.max(lamp.glow) / OFF_BEAT;
                        mix(rest, strike, struck)
                    };
                    let rgb = pale(rgb, 0.6 * climb);
                    output(rgb, level * bright * even(rgb))
                }
            })
            .collect()
    }
}

/// `rgb` mixed `t` of the way to white.
pub(super) fn pale(rgb: Rgb, t: f32) -> Rgb {
    let mix = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * t).round() as u8;
    Rgb::new(mix(rgb.r), mix(rgb.g), mix(rgb.b))
}
