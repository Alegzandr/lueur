//! The music show: the lights as a small stage that plays along with the
//! track, rather than bulbs blinking in unison.
//!
//! Two halves. The [`Analyzer`] listens: it splits the sound into the kick,
//! the snare and the hi-hats, reads each against how loud the track has been
//! playing (so the volume knob changes nothing), and locks onto the tempo so
//! the beats land on time and carry on through a kick it missed. The
//! [`Stage`] plays: every light gets a part from where it stands in the room
//! and how fast it answers. The quick ones near the screen punch the beat in
//! patterns that travel across the room, the slow or distant ones wash it in
//! colour, a light at the ceiling answers the snare. Like a lighting desk at a
//! concert, the colours change with the phrase and the beats play the
//! brightness and the movement, the one of each bar leading. And the show
//! follows the shape of the track: one deep colour in a breakdown, a
//! quickening chase whitening through a build, dark for the breath before the
//! drop, everything at once when it lands. It dresses each track for what it
//! is: the looks follow how hard the music plays (lit and trading colours in
//! a verse, travelling in the body of the track, bursting at its peak, and a
//! chorus comes back to its own), a ballad plays in warm, pale colours and a
//! club track in neon, white is kept for the drops, and a track in half time
//! is struck in half time.

mod analyzer;
mod dsp;
mod fixture;
mod pattern;
mod play;
mod tempo;
#[cfg(test)]
mod tests;
mod track;

pub use analyzer::{Analyzer, LEAD, Levels, MOST_LEAD};
pub use fixture::Fixture;

use crate::color::{BulbOutput, Rgb};
use crate::show::{ShowSettings, blend};
use crate::sync::{Lag, timing};
use dsp::follow;
use fixture::{Part, cast};
use pattern::{PEAK, Pattern};
use std::collections::VecDeque;
use track::{CALM_WAVE_SECS, Mood, QUICKEST_FADE, SIDE, Section, TREBLE_SECS};

/// One light on the stage.
#[derive(Debug, Clone, Copy)]
struct Lamp {
    part: Part,
    /// Left to right among the punching lights.
    rank: usize,
    /// Where it stands across the room, 0 at the left to 1 at the right.
    place: f32,
    /// The quickest its flash may fade, seconds: longer for a slow bulb, so
    /// it can still show it.
    fade: f32,
    flash: f32,
    white: f32,
    /// A hit landed on the last frame: send it sharp, not faded in.
    sharp: bool,
    /// How much it still burns in the colour of a beat it holds (0..=1),
    /// at `hold`: while `holding` it keeps the beat, once it hands it on it
    /// goes out.
    glow: f32,
    hold: f32,
    holding: bool,
    /// A wash's share of the way from the room's colour to the second hit
    /// colour: neighbouring washes trade the two, bar by bar.
    tint: f32,
    /// Every other light across the room.
    odd: bool,
}

impl Lamp {
    /// Land a hit of `amount`; `white` makes it a white flash.
    fn hit(&mut self, amount: f32, white: bool) {
        if amount <= 0.0 {
            return;
        }
        // A wash glides into everything, hits included.
        if self.part != Part::Wash && (amount > self.flash + 0.15 || white) {
            self.sharp = true;
        }
        self.flash = self.flash.max(amount.min(1.0));
        if white {
            self.white = 1.0;
        }
    }
}

/// `a` turned `t` of the way to `b` round the wheel, landing on each exactly.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        blend(a, b, t)
    }
}

/// The music show.
///
/// It plays like a lighting desk at a concert. Its colours come from one of
/// the harmonies of the track's mood at a time, in two roles: the room rests
/// in a back colour and the punches strike in the other two, fading back into
/// it. The roles turn with every phrase, the harmony moves on every few
/// phrases, at every drop and when the mood changes, and the beats play the
/// brightness, from near dark, and where it lands in the room. The lights play off one another: one holds
/// the beat and hands it to the next, two halves turn over, the colours
/// trade places or travel along the room, and the washes trade theirs bar
/// by bar.
#[derive(Debug, Clone)]
pub struct Stage {
    lamps: Vec<Lamp>,
    /// Punching lights.
    punches: usize,
    section: Section,
    /// Seconds in this section.
    held: f32,
    /// Energy over the last second or so, and over the last half minute.
    short: f32,
    long: f32,
    /// Energy over the last few seconds: a build climbs above it.
    recent: f32,
    /// Snare and hat hits a second, lately and over a longer while.
    hits: f32,
    hits_long: f32,
    /// How much the hi-hats are going: the accent shimmers with them.
    shimmer: f32,
    /// The loudness and bass the lights burn with, smoothed.
    swell: f32,
    bass: f32,
    /// Seconds since anything played the beat: a kick, or in a track
    /// without one, the clock.
    since_pulse: f32,
    /// Seconds a beat: the clock's, or a guess.
    period: f32,
    /// Beats into the current phrase, and phrases so far.
    beat: u32,
    phrases: u32,
    /// The next beat counted is the first of a phrase.
    starting: bool,
    /// Beats since the colours last moved on.
    since_turn: u32,
    /// What the last beats sounded like (mix, bass, middle, treble), the
    /// latest last, and the sound gathered since the last one.
    sounds: VecDeque<[f32; 4]>,
    gather: [f32; 4],
    gathered: u32,
    /// How far the sound moved at the last two beats compared, the latest
    /// last.
    turns: [f32; 2],
    /// How far a breakdown's swell has rolled, in waves: it rolls faster
    /// the louder the music.
    wave: f32,
    /// Steps a build's chase has taken, and how far it is to the next.
    roll: usize,
    roll_due: f32,
    sharp: Vec<bool>,
    /// Where each light stands and how fast it answers.
    fixtures: Vec<Fixture>,
    /// How early the beats go out, and how long each light waits on them
    /// (see [`timing`]).
    lead: f32,
    waits: Vec<f32>,
    lag: Lag,
    /// Seconds until the clock's next beat goes out, as last heard.
    due: Option<f32>,
    /// The pattern playing, how hard the passage it was chosen for plays,
    /// and phrases it has played.
    look: Pattern,
    tier: usize,
    look_age: u32,
    /// Which look of each tier plays when the music comes back to it: the
    /// chorus finds its own look again.
    looks: [usize; 3],
    /// The look moved on to the next of its tier as this phrase began.
    rotated: bool,
    /// How hard the last bar read, waiting for the next to agree before the
    /// look follows it.
    tier_heard: Option<usize>,
    /// How the bars lately sounded, against which a bar reads, and bars
    /// since the tier last changed.
    bars: VecDeque<f32>,
    tier_bars: u32,
    mood: Mood,
    /// The treble over the last half minute (NaN until the music plays),
    /// and seconds of music it has heard, up to that.
    treble: f32,
    treble_for: f32,
    /// Whether the clock is sure of the tempo.
    locked: bool,
    /// Kicks a second, lately.
    kicks: f32,
    /// Beats counted in all, and seconds since the last.
    beats: u32,
    since_beat: f32,
    /// How many snares fell on each beat of the bar lately, by `beats`.
    snares: [f32; 4],
    /// The track plays in half time (or the clock counts it twice as fast
    /// as it feels): only every other beat is struck, those whose place in
    /// the bar matches `half_on`, the beat the snare lands on.
    half: bool,
    half_on: usize,
}

impl Stage {
    pub fn new(fixtures: &[Fixture]) -> Self {
        let parts = cast(fixtures);
        let by_x = |a: &usize, b: &usize| fixtures[*a].x.total_cmp(&fixtures[*b].x);
        let mut order: Vec<usize> = (0..fixtures.len())
            .filter(|&i| parts[i] == Part::Punch)
            .collect();
        order.sort_by(by_x);
        let mut room: Vec<usize> = (0..fixtures.len()).collect();
        room.sort_by(by_x);
        let n = fixtures.len();
        let lamps = fixtures
            .iter()
            .zip(&parts)
            .enumerate()
            .map(|(i, (f, &part))| Lamp {
                part,
                rank: order.iter().position(|&o| o == i).unwrap_or(0),
                place: if n > 1 {
                    room.iter().position(|&o| o == i).unwrap_or(0) as f32 / (n - 1) as f32
                } else {
                    0.5
                },
                fade: (2.5 * f.interval.as_secs_f32()).max(QUICKEST_FADE),
                flash: 0.0,
                white: 0.0,
                sharp: false,
                glow: 0.0,
                hold: 0.0,
                holding: false,
                tint: 0.0,
                odd: room.iter().position(|&o| o == i).unwrap_or(0) % 2 == 1,
            })
            .collect();
        Stage {
            lamps,
            punches: order.len(),
            section: Section::Calm,
            held: 0.0,
            short: 0.0,
            long: 0.0,
            recent: 0.0,
            hits: 0.0,
            hits_long: 0.0,
            shimmer: 0.0,
            swell: 0.0,
            bass: 0.0,
            since_pulse: 10.0,
            period: 0.5,
            beat: 0,
            phrases: 0,
            starting: false,
            since_turn: 0,
            sounds: VecDeque::new(),
            gather: [0.0; 4],
            gathered: 0,
            turns: [0.0; 2],
            wave: 0.0,
            roll: 0,
            roll_due: 0.0,
            sharp: vec![false; n],
            fixtures: fixtures.to_vec(),
            lead: LEAD,
            waits: vec![0.0; n],
            lag: Lag::default(),
            due: None,
            look: Pattern::for_lights(order.len(), PEAK)[0],
            tier: PEAK,
            look_age: 0,
            looks: [0; 3],
            rotated: false,
            tier_heard: None,
            bars: VecDeque::new(),
            tier_bars: 0,
            treble: f32::NAN,
            treble_for: 0.0,
            mood: Mood::Party,
            locked: false,
            kicks: 0.0,
            beats: 0,
            since_beat: 0.0,
            snares: [0.0; 4],
            half: false,
            half_on: 0,
        }
    }

    /// The section the show reads the track to be in, one letter, for
    /// benches.
    #[doc(hidden)]
    pub fn debug_section(&self) -> &'static str {
        match self.section {
            Section::Groove => "G",
            Section::Calm => "c",
            Section::Build => "B",
            Section::Hold => "h",
            Section::Drop => "D",
        }
    }

    /// How the show dresses the track, for benches: the tier of its look
    /// (g, d, p), its mood (N for neon, S for soft), H in half time, and the
    /// look.
    #[doc(hidden)]
    pub fn debug_look(&self) -> String {
        format!(
            "{}{}{} {:?} kicks/beat {:.2} treble {:.2} bar {:.2} bpm {:.0} snares {:.1?}",
            ["g", "d", "p"][self.tier],
            if self.mood == Mood::Party { "N" } else { "S" },
            if self.half { "H" } else { "-" },
            self.pattern(),
            self.kicks * self.period,
            self.treble,
            self.last_bar().unwrap_or(f32::NAN),
            60.0 / self.period,
            self.snares,
        )
    }

    /// The lights that should be sent sharp this frame rather than faded in.
    pub fn sharp(&self) -> &[bool] {
        &self.sharp
    }

    /// How early the clock's beats should go out (see [`Analyzer::set_lead`]):
    /// as long as the slowest light takes to show one, less the delay the
    /// sound reaches the ear with.
    pub fn lead(&self) -> f32 {
        self.lead
    }

    /// Seconds until light `i` is due something sharp: the clock's next
    /// beat, once it has waited its turn, or a hit it holds back. A light
    /// that can only take a command now and then should keep its next one
    /// for it.
    pub fn due(&self, i: usize) -> Option<f32> {
        let wait = self.waits.get(i).copied().unwrap_or(0.0);
        let beat = self.due.map(|d| d + wait);
        let held = self.lag.next_sharp(i, wait);
        match (beat, held) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Seconds until anything sharp is due on any light: when to compute
    /// the next frame, at the latest, for it to go out on time.
    pub fn next_due(&self) -> Option<f32> {
        (0..self.lamps.len())
            .filter_map(|i| self.due(i))
            .min_by(f32::total_cmp)
    }

    /// Advance by `dt` seconds with what the music is doing.
    pub fn frame(&mut self, settings: &ShowSettings, levels: Levels, dt: f32) -> Vec<BulbOutput> {
        (self.lead, self.waits) = timing(&self.fixtures, settings.delay());
        self.due = levels.due;
        // A hit fades out just before the next beat struck comes, like a
        // pumping sidechain, but never quicker than the bulb can show.
        let felt = self.felt_period();
        let pump = felt / 3.0;
        // A light holding the beat lets go if the beats stop coming, and
        // one that handed it on goes out within the beat, as the next one
        // comes up.
        let kept = self.since_pulse < 2.0 * felt;
        for lamp in &mut self.lamps {
            let fade = lamp.fade.max(pump);
            lamp.flash *= (-dt / fade).exp();
            lamp.white *= (-dt / (0.6 * fade)).exp();
            if !(lamp.holding && kept) {
                lamp.glow *= (-dt / lamp.fade.max(0.25 * felt)).exp();
            }
            lamp.sharp = false;
        }
        self.held += dt;
        self.since_beat += dt;
        self.locked = levels.tempo.is_some();
        // The treble while the music plays: an even mean of what has been
        // heard at first, so the track is not judged on its intro, then of
        // the last half minute.
        if levels.energy > 0.2 {
            self.treble_for = (self.treble_for + dt).min(TREBLE_SECS);
            self.treble = if self.treble.is_nan() {
                levels.treble
            } else {
                self.treble + (levels.treble - self.treble) * follow(dt, self.treble_for)
            };
        }
        let kick = if levels.kick { 1.0 / dt.max(1e-3) } else { 0.0 };
        self.kicks += (kick - self.kicks) * follow(dt, 6.0);
        // A sure clock in music still at its level plays the beat as if it
        // were a kick: through the kicks a loud bass hides, and all along a
        // track that has none (a band without drums). A breakdown drops the
        // level, or lets the clock go.
        let carried = levels.tempo.is_some() && self.short >= self.long - 0.1;
        let pulse = levels.kick || (carried && levels.beat.is_some());
        self.since_pulse = if pulse { 0.0 } else { self.since_pulse + dt };
        if let Some(p) = levels.tempo {
            self.period = p;
        }
        for (g, v) in
            self.gather
                .iter_mut()
                .zip([levels.energy, levels.bass, levels.mid, levels.treble])
        {
            *g += v;
        }
        self.gathered += 1;
        let e = levels.energy;
        self.short += (e - self.short) * follow(dt, 1.0);
        self.recent += (e - self.recent) * follow(dt, 4.0);
        self.long += (e - self.long) * follow(dt, 30.0);
        let struck = f32::from(u8::from(levels.snare.is_some()) + u8::from(levels.hat.is_some()));
        let rate = struck / dt.max(1e-3);
        self.hits += (rate - self.hits) * follow(dt, 1.5);
        self.hits_long += (rate - self.hits_long) * follow(dt, 8.0);
        let hat = if levels.hat.is_some() { 1.0 } else { 0.0 };
        self.shimmer = self.shimmer * (-dt / 0.3).exp() + 0.5 * hat * (1.0 - self.shimmer);
        let tau = if e > self.swell { 0.04 } else { 0.25 };
        self.swell += (e - self.swell) * follow(dt, tau);
        self.wave = (self.wave + dt * (1.0 + self.loud()) / CALM_WAVE_SECS) % 1.0;
        let tau = if levels.bass > self.bass { 0.06 } else { 0.4 };
        self.bass += (levels.bass - self.bass) * follow(dt, tau);

        // A cymbal crashing in with the kick as the whole band jumps up a
        // level is a phrase starting there and then, wherever the count is.
        let crash = self.section == Section::Groove
            && levels.kick
            && levels.hat.is_some_and(|h| h >= 0.8)
            && e > self.recent + 0.2
            && self.beat != 0
            && self.since_turn >= SIDE as u32;
        if crash {
            self.turn();
            self.start_phrase(&levels);
        }
        self.follow_track(settings, &levels, pulse);
        match self.section {
            Section::Groove | Section::Drop => self.groove(settings, &levels),
            Section::Build => self.build(settings, &levels, dt),
            Section::Calm => self.calm(&levels),
            Section::Hold => {}
        }
        if let Some(h) = levels.hat {
            for lamp in &mut self.lamps {
                if lamp.part == Part::Accent {
                    lamp.hit(0.25 * h, false);
                }
            }
        }
        // In a groove the washes trade the room's colour and the second hit
        // colour, a bar each, gliding over a beat: the room moves with the
        // stage instead of resting under it.
        let grooving = matches!(self.section, Section::Groove | Section::Drop);
        let bar = self.beat / 4;
        for lamp in &mut self.lamps {
            if lamp.part == Part::Wash {
                let target = if grooving && (bar % 2 == 1) != lamp.odd {
                    1.0
                } else {
                    0.0
                };
                lamp.tint += (target - lamp.tint) * follow(dt, lamp.fade.max(felt));
            }
        }

        let outputs = self.light(settings);
        for (sharp, lamp) in self.sharp.iter_mut().zip(&self.lamps) {
            *sharp = lamp.sharp;
        }
        self.lag.run(dt, outputs, &mut self.sharp, &self.waits)
    }
}
