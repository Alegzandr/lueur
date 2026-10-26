//! Following the track: where it is (a groove, a breakdown, a build, a
//! drop), its phrases and bars, its mood, and whether it plays in half
//! time.

use super::pattern::{DRIVING, GENTLE, PEAK, Pattern};
use super::{Levels, Stage};
use crate::color::Rgb;
use crate::show::{PARTY, SOFT, ShowSettings};

/// Where the track is, as a lighting desk would read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    /// The beat is on.
    Groove,
    /// The kick has gone: a breakdown, an intro, a quiet bridge.
    Calm,
    /// No kick, but the snares and hats crowd in and the level climbs.
    Build,
    /// The build fell silent all at once: the breath before the drop.
    Hold,
    /// The kick is back, all at once.
    Drop,
}

/// Beats in a phrase: four bars of four. The colours hold through a phrase
/// and move on as the next one lands, the way a lighting desk's do; the
/// beats play the brightness and the movement, never the colour.
pub(super) const PHRASE_BEATS: u32 = 16;
/// Phrases the show stays on one pattern while the music holds its tier:
/// sixteen bars, a whole section. The tiers bring the changes in between.
const PHRASES_PER_LOOK: u32 = 4;
/// Phrases the show stays on one harmony, unless a drop moves it on.
pub(super) const PHRASES_PER_HARMONY: u32 = 4;
/// How long a build takes to reach white, seconds.
pub(super) const BUILD_SECS: f32 = 8.0;
/// A white flash's colour temperature.
pub(super) const WHITE_KELVIN: u16 = 6500;
/// The quickest a quick bulb's hit fades, seconds. Slower bulbs take longer,
/// and every hit stretches to fill most of a slow beat.
pub(super) const QUICKEST_FADE: f32 = 0.08;
/// Seconds for a breakdown's colour to drift to its other shade and back.
pub(super) const CALM_DRIFT_SECS: f32 = 30.0;
/// Seconds for a breakdown's slow swell to roll across the room.
pub(super) const CALM_WAVE_SECS: f32 = 6.0;
/// A calm shorter than this, seconds, was the ear losing the beat for a
/// moment rather than the track changing: the groove picks up where it
/// left off, in the same colours.
const CALM_STUMBLE: f32 = 4.0;
/// Beats compared on either side of where a phrase may have turned.
pub(super) const SIDE: usize = 4;
/// How far the sound must move between those two sides (mix, bass, middle
/// and treble together, each 0..=1) for a new phrase to have begun: new
/// instruments coming in, or going.
const TURN: f32 = 0.2;
/// Beats a phrase found late must have run before the colours still move
/// on for it; nearer the last change, only the count is set right.
pub(super) const LATE_TURN: u32 = 6;
/// How hard a beat strikes off the one of its bar: the one leads the bar,
/// so the eye counts it the way the body does.
pub(super) const OFF_BEAT: f32 = 0.8;
/// Bars a passage is read against: the last two minutes or so of the
/// track. Every track is loud and bright somewhere; what tells a chorus is
/// how it stands against the rest of the same track.
const BAR_MEMORY: usize = 64;
/// Bars heard before the looks follow the track at all.
const BARS_TO_LEARN: usize = 8;
/// How far up the bars lately a bar must rank to be at the peak (and may
/// fall to stay there), and how far down to be gentle (and may rise to stay
/// there).
const PEAK_RANK: (f32, f32) = (0.7, 0.6);
const GENTLE_RANK: (f32, f32) = (0.25, 0.35);
/// And how far from the middle bar it must sound: a track at one level all
/// through has no peak and no lull, only its body.
const PEAK_LIFT: f32 = 0.02;
const GENTLE_DROP: f32 = 0.05;
/// Bars a tier plays at the least before the look moves to another: a desk
/// does not change the look on every bar.
const TIER_BARS: u32 = 4;
/// The treble, against its own loud parts, over the last half minute, under
/// which a track is gentle and over which it is a party again. A ballad's
/// cymbals and hats come and go; a club track's never stop. (Measured on
/// real tracks: a piano ballad sits near 0.6, club, rap and rock over 0.78.)
const SOFT_TREBLE: f32 = 0.65;
const PARTY_TREBLE: f32 = 0.72;
/// Seconds of music the treble is averaged over.
pub(super) const TREBLE_SECS: f32 = 30.0;
/// Kicks a beat under which a track has no kick to speak of, and over which
/// it has one again.
const NO_KICK: (f32, f32) = (0.2, 0.3);
/// How much of a snare's weight on each beat of the bar is kept from one
/// beat to the next: the last few bars count.
const SNARE_MEMORY: f32 = 0.95;
/// How close to a beat a snare must fall to count as on it, as a share of
/// a beat.
const SNARE_ON_BEAT: f32 = 0.2;
/// The half-time feel: the snare on one beat of the bar and on no other,
/// this sure (clear snares heard on it, give or take) and with the others
/// this far under it. It lets go when another catches up this far. A dense
/// mix strikes the snare's band on every beat (a wobble, an 808); there the
/// show plays the full beat, which is never wrong, only less telling.
const HALF_SURE: f32 = 2.5;
const HALF_LOPSIDED: f32 = 0.35;
const HALF_GONE: f32 = 0.6;
/// A snare this strong counts for nothing towards the half time, and one at
/// full strength for one: the faint ones are as often something else.
const SNARE_CLEAR: f32 = 0.6;
/// The slowest tempo read in half time, seconds a beat: below 130 beats a
/// minute a snare on the three is a slow groove, not a halved one, and a
/// club track's claps can pass for one.
const HALF_PERIOD: f32 = 0.46;

/// The track's mood, which sets the colours the show plays in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mood {
    /// Driven by the kick, at a pace: the party's neon.
    Party,
    /// Slow, or with little or no kick: warm, paler colours, never a white
    /// flash.
    Soft,
}

impl Stage {
    pub(super) fn pattern(&self) -> Pattern {
        if self.section == Section::Drop {
            return Pattern::Together;
        }
        self.look
    }

    /// Play the look of `tier` its turn has come to, from this phrase.
    fn set_look(&mut self, tier: usize) {
        let all = Pattern::for_lights(self.punches, tier);
        self.tier = tier;
        self.look = all[self.looks[tier] % all.len()];
        self.look_age = 0;
    }

    /// A bar has just begun: read how hard the one before it played against
    /// the bars lately, and once two bars agree that the music has moved up
    /// or down a tier, and the look has played a while, change the look to
    /// that tier's.
    pub(super) fn read_bar(&mut self) {
        let Some(sound) = self.last_bar() else {
            return;
        };
        self.bars.push_back(sound);
        while self.bars.len() > BAR_MEMORY {
            self.bars.pop_front();
        }
        self.tier_bars = self.tier_bars.saturating_add(1);
        let n = self.bars.len();
        if n < BARS_TO_LEARN {
            return;
        }
        let mut sorted: Vec<f32> = self.bars.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let middle = sorted[n / 2];
        let rank = sorted.iter().filter(|&&b| b < sound).count() as f32 / n as f32;
        // Crossing into a tier takes more than staying in it.
        let peak = if self.tier == PEAK {
            PEAK_RANK.1
        } else {
            PEAK_RANK.0
        };
        let gentle = if self.tier == GENTLE {
            GENTLE_RANK.1
        } else {
            GENTLE_RANK.0
        };
        let tier = if rank >= peak && sound > middle + PEAK_LIFT {
            PEAK
        } else if rank < gentle && sound < middle - GENTLE_DROP {
            GENTLE
        } else {
            DRIVING
        };
        if tier != self.tier && self.tier_heard == Some(tier) && self.tier_bars >= TIER_BARS {
            self.tier_bars = 0;
            // A look that moved on as this phrase began was for a phrase its
            // tier never played: it is still that tier's next.
            if self.rotated && self.look_age == 0 {
                self.looks[self.tier] -= 1;
            }
            self.rotated = false;
            self.set_look(tier);
        }
        self.tier_heard = Some(tier);
    }

    /// How the last bar sounded, the mix, the middle and the treble together
    /// (each 0..=1): a chorus brings the hats and the voices up as well as
    /// the level. None until a bar has been heard.
    pub(super) fn last_bar(&self) -> Option<f32> {
        let n = self.sounds.len();
        (n >= 4).then(|| {
            self.sounds
                .range(n - 4..)
                .map(|[e, _, m, t]| (e + m + t) / 3.0)
                .sum::<f32>()
                / 4.0
        })
    }

    pub(super) fn harmony(&self) -> &'static [Rgb; 3] {
        let all = match self.mood {
            Mood::Party => PARTY,
            Mood::Soft => SOFT,
        };
        &all[(self.phrases / PHRASES_PER_HARMONY) as usize % all.len()]
    }

    /// The mood the track plays in, now: kept until it plainly changes.
    fn heard_mood(&self) -> Mood {
        if !self.locked || self.treble.is_nan() {
            return self.mood;
        }
        let kicks = self.kicks * self.period;
        match self.mood {
            Mood::Party if self.treble < SOFT_TREBLE || kicks < NO_KICK.0 => Mood::Soft,
            Mood::Soft if self.treble > PARTY_TREBLE && kicks > NO_KICK.1 => Mood::Party,
            mood => mood,
        }
    }

    /// The colours of the phrase: the back colour the room rests in, and the
    /// two the punches strike in. Each colour of the harmony takes each role
    /// in turn, one phrase after another.
    pub(super) fn roles(&self) -> (Rgb, Rgb, Rgb) {
        let h = self.harmony();
        let p = self.phrases as usize;
        (h[(p + 1) % 3], h[p % 3], h[(p + 2) % 3])
    }

    /// Whether the white flashes are on: never in a gentle track. White is
    /// kept for the drops, where it means something; flashed on every loud
    /// snare it soon means nothing, and tires the eye. How often they may go
    /// out is not the show's to decide: the show's guard
    /// ([`crate::safety::FlashGuard::show`]) keeps them to three a second,
    /// protection on or off.
    pub(super) fn white(&self, settings: &ShowSettings) -> bool {
        settings.flashes && self.mood == Mood::Party
    }

    pub(super) fn enter(&mut self, section: Section) {
        // What the beats sounded like is only compared within a groove.
        if section != Section::Groove {
            self.sounds.clear();
            self.turns = [0.0; 2];
        }
        self.section = section;
        self.held = 0.0;
        self.roll = 0;
        self.roll_due = 0.0;
        for lamp in &mut self.lamps {
            // A beat is held only within a groove.
            lamp.holding = false;
            if section == Section::Hold {
                lamp.flash = 0.0;
                lamp.white = 0.0;
                lamp.glow = 0.0;
            }
        }
    }

    /// The beat landing now is the first of a phrase. The clock may have
    /// played it already, a little early: then it is counted here.
    pub(super) fn start_phrase(&mut self, levels: &Levels) {
        if levels.beat.is_some() {
            self.starting = true;
        } else {
            self.starting = false;
            self.beat = 0;
        }
    }

    /// The colours move on to the next phrase's. If the track's mood has
    /// changed, they move into the other mood's harmonies; if the look has
    /// played its while, the next look of its tier takes over.
    pub(super) fn turn(&mut self) {
        self.phrases += 1;
        self.since_turn = 0;
        let mood = self.heard_mood();
        if mood != self.mood {
            self.mood = mood;
            self.phrases = (self.phrases / PHRASES_PER_HARMONY + 1) * PHRASES_PER_HARMONY;
        }
        self.look_age += 1;
        self.rotated = self.look_age >= PHRASES_PER_LOOK;
        if self.rotated {
            self.looks[self.tier] += 1;
            self.set_look(self.tier);
        }
    }

    /// Count a beat: the first of a phrase after a fresh start, otherwise
    /// the next. A phrase full, the colours move on, and a drop gives way to
    /// the patterns.
    pub(super) fn count_beat(&mut self) {
        self.since_turn += 1;
        if self.starting {
            self.starting = false;
            self.beat = 0;
            return;
        }
        self.beat += 1;
        if self.beat >= PHRASE_BEATS {
            self.beat = 0;
            self.turn();
            if self.section == Section::Drop {
                self.enter(Section::Groove);
            }
        }
        // A phrase found late can set the count back; the drop still gives
        // way after a phrase's worth of beats.
        if self.section == Section::Drop && self.since_turn >= PHRASE_BEATS {
            self.enter(Section::Groove);
        }
    }

    /// Note what the beat that just ended sounded like. If the sound moved
    /// most a few beats back, a phrase began there: the beats since, then,
    /// counting the one starting now.
    ///
    /// Four bars of four are counted from wherever the clock found the
    /// beat, which is rarely where the track's phrases start. A phrase
    /// starts where instruments come in or go (the hats enter, the bass
    /// drops out), so the sound of the four beats before is set against
    /// the four after, and where it moved most is the turn.
    pub(super) fn mark(&mut self) -> Option<u32> {
        if self.gathered > 0 {
            let n = self.gathered as f32;
            self.sounds.push_back(self.gather.map(|v| v / n));
        }
        self.gather = [0.0; 4];
        self.gathered = 0;
        while self.sounds.len() > 2 * SIDE {
            self.sounds.pop_front();
        }
        if self.sounds.len() < 2 * SIDE {
            return None;
        }
        let mean = |from: usize| {
            let mut m = [0.0f32; 4];
            for s in self.sounds.range(from..from + SIDE) {
                for (m, v) in m.iter_mut().zip(s) {
                    *m += v / SIDE as f32;
                }
            }
            m
        };
        let (before, after) = (mean(0), mean(SIDE));
        let moved: f32 = before.iter().zip(&after).map(|(a, b)| (a - b).abs()).sum();
        let [older, last] = self.turns;
        self.turns = [last, moved];
        // The last comparison, a beat ago, was the peak: it set the beats
        // before its middle against those from it on, so the phrase began
        // `SIDE` beats before that one, and this beat is the one after.
        (last > TURN && last > older && last >= moved).then_some(SIDE as u32 + 1)
    }

    /// Read where the track is from what was heard this frame. `struck` is
    /// whether something played the beat: a kick, or the clock in a track
    /// without one.
    pub(super) fn follow_track(&mut self, settings: &ShowSettings, levels: &Levels, struck: bool) {
        // A bar without the beat, not a beat or two the ear missed: going
        // calm and back at every kick the bass hid looks like a show that
        // cannot make up its mind.
        let gone = self.since_pulse > (4.0 * self.period).max(2.0);
        let back = struck;
        match self.section {
            Section::Groove | Section::Drop if gone => self.enter(Section::Calm),
            Section::Calm if back => {
                // After a long break the kick's return is a drop of its own.
                // After a longer gap than a stumble it starts a phrase: the
                // colours move on as it lands. A stumble just picks the
                // groove up where it left it.
                if self.held > 6.0 && levels.kick {
                    self.drop(settings, levels);
                } else if self.held < CALM_STUMBLE {
                    self.enter(Section::Groove);
                } else {
                    self.enter(Section::Groove);
                    self.turn();
                    self.start_phrase(levels);
                }
            }
            Section::Calm => {
                let crowding = self.hits > 3.0 && self.hits > 1.3 * self.hits_long;
                let climbing = self.short > self.recent + 0.06;
                if self.held > 1.0 && (crowding || climbing) {
                    self.enter(Section::Build);
                }
            }
            Section::Build | Section::Hold if levels.kick => self.drop(settings, levels),
            // A track without a kick does not drop: its beat just carries on.
            Section::Build | Section::Hold if back => {
                self.enter(Section::Groove);
                self.turn();
                self.start_phrase(levels);
            }
            Section::Build => {
                if self.short > 0.4 && levels.energy < 0.5 * self.short {
                    self.enter(Section::Hold);
                } else if self.short < self.long - 0.2 || self.held > 30.0 {
                    // It fell back without dropping.
                    self.enter(Section::Calm);
                }
            }
            Section::Hold if self.held > 2.5 => self.enter(Section::Calm),
            _ => {}
        }
    }

    /// The drop: everyone at once, white if allowed, then straight into a
    /// new harmony for a phrase of everyone together.
    pub(super) fn drop(&mut self, settings: &ShowSettings, levels: &Levels) {
        self.enter(Section::Drop);
        self.phrases = (self.phrases / PHRASES_PER_HARMONY + 1) * PHRASES_PER_HARMONY;
        self.since_turn = 0;
        self.start_phrase(levels);
        // What follows a drop is the track at its peak.
        self.rotated = false;
        self.set_look(PEAK);
        self.tier_heard = None;
        let white = self.white(settings);
        for lamp in &mut self.lamps {
            lamp.hit(1.0, white);
        }
    }

    /// How loud the music plays, as far as a breakdown cares: nothing in a
    /// quiet one, 1 when it plays near the top of its range.
    pub(super) fn loud(&self) -> f32 {
        ((self.swell - 0.4) / 0.4).clamp(0.0, 1.0)
    }

    /// Seconds between two beats struck: twice the clock's in half time.
    pub(super) fn felt_period(&self) -> f32 {
        if self.half {
            2.0 * self.period
        } else {
            self.period
        }
    }

    /// How hard the beat just counted strikes, against the strongest: the
    /// one of the bar leads. None for a beat half time leaves out.
    pub(super) fn stress(&self) -> Option<f32> {
        if self.half {
            // The snare lands on the three; the one is across the bar.
            let at = self.beats as usize % 4;
            let one = (self.half_on + 2) % 4;
            if at % 2 != one % 2 {
                return None;
            }
            return Some(if at == one { 1.0 } else { OFF_BEAT });
        }
        Some(if self.beat.is_multiple_of(4) {
            1.0
        } else {
            OFF_BEAT
        })
    }

    /// The beat the patterns move on: every other one in half time.
    pub(super) fn step(&self) -> u32 {
        if self.half { self.beat / 2 } else { self.beat }
    }

    /// Note which beat of the bar a snare just heard fell on, if it fell on
    /// one: the last beat counted or the next, as they reach the ear (each
    /// goes out [`Stage::lead`] early).
    pub(super) fn place_snare(&mut self, strength: f32) {
        let p = self.period;
        let off = self.since_beat - self.lead;
        let (at, by) = if off < 0.5 * p {
            (self.beats, off.abs())
        } else {
            (self.beats.wrapping_add(1), p - off)
        };
        if by < SNARE_ON_BEAT * p {
            let clear = ((strength - SNARE_CLEAR) / (1.0 - SNARE_CLEAR)).clamp(0.0, 1.0);
            self.snares[at as usize % 4] += clear;
        }
    }

    /// A beat was counted: the snares heard on each beat of the bar fade a
    /// little, and the show reads whether the track plays in half time. In
    /// a rock or a club groove the snare lands on the two and the four, two
    /// beats apart; in half time (trap, dubstep, a heavy chorus) it lands on
    /// the three alone, four apart, and the body feels a beat on every other
    /// one the clock counts. The same reading puts right a clock that counts
    /// a slow track twice over.
    pub(super) fn feel_half(&mut self) {
        for s in &mut self.snares {
            *s *= SNARE_MEMORY;
        }
        let (on, top) = self
            .snares
            .iter()
            .copied()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        // The loudest of the other beats of the bar.
        let others = (1..4)
            .map(|d| self.snares[(on + d) % 4])
            .fold(0.0, f32::max);
        let fast = self.locked && self.period <= HALF_PERIOD;
        if !self.half {
            if fast && top >= HALF_SURE && others < HALF_LOPSIDED * top {
                self.half = true;
                self.half_on = on;
            }
        } else if !fast || top < 0.5 * HALF_SURE || others > HALF_GONE * top {
            self.half = false;
        } else {
            self.half_on = on;
        }
    }
}
