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
//! brightness and the movement. And the show follows the shape of the track:
//! one deep colour in a breakdown, a quickening chase whitening through a
//! build, dark for the breath before the drop, everything at once when it
//! lands.

use crate::color::{BulbOutput, Rgb};
use crate::geometry::{Light, Screen};
use crate::show::{PARTY, ShowSettings, blend, even, output};
use crate::sync::{Lag, timing};
use std::collections::VecDeque;
use std::f32::consts::TAU;
use std::time::Duration;

/// Length of one analysis step: short enough to place a beat within a few
/// milliseconds whatever the frame rate, long enough to measure the bass.
const HOP_SECS: f32 = 0.01;

/// Below this a signal is silence, whatever the gain (dBFS).
const SILENCE_DB: f32 = -75.0;

/// How far under the loud parts of a track a level reads as nothing, in dB.
const RANGE_DB: f32 = 30.0;

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

/// The share of the way an exponential follower with time constant `tau`
/// moves in `dt`.
fn follow(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau).exp()
}

/// Which side of a frequency a [`Biquad`] keeps.
#[derive(Debug, Clone, Copy)]
enum Pass {
    Low,
    High,
    Band,
}

/// A second-order filter (the audio cookbook's): enough to split the sound
/// into bands without an FFT. Run in double precision, which a low cut-off at
/// a high sample rate needs to stay stable.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(pass: Pass, rate: f32, freq: f32, q: f32) -> Self {
        // Kept clear of the Nyquist frequency, where the formulas fold over.
        let w = f64::from(TAU * freq.min(0.45 * rate) / rate);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * f64::from(q));
        let (b0, b1, b2) = match pass {
            Pass::Low => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            Pass::High => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
            Pass::Band => (alpha, 0.0, -alpha),
        };
        let a0 = 1.0 + alpha;
        Biquad {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let x = f64::from(x);
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }
}

/// How loud the track is playing, learnt as it plays, so that every level is
/// read against it: a show at 10% volume looks like the same show at 100%.
///
/// It follows the loud parts: it rises at once with the music and lets go
/// slowly, so a quiet verse, or a breakdown half a minute long, still reads as
/// quieter than the chorus. When the level falls a long way and stays there
/// while the groove plainly carries on (the kick still coming), that is the
/// volume being turned down, in Windows or in the player, rather than a quiet
/// passage, and it lets go far faster. A breakdown drops the kick, so it is
/// never taken for one. In silence it holds, so a track that comes back comes
/// back at the level it left.
#[derive(Debug, Clone, Copy)]
struct Gain {
    /// The level of the loud parts, dB (NaN until something is heard).
    reference: f32,
    /// How long the level has been far under it with the music still going.
    low_for: f32,
    /// The volume went down: follow it fast until the loud parts are found
    /// again.
    sinking: bool,
}

impl Gain {
    fn new() -> Self {
        Gain {
            reference: f32::NAN,
            low_for: 0.0,
            sinking: false,
        }
    }

    /// Learn from `level`, in dB; `steady` is whether the groove carries on,
    /// the kick still coming.
    fn learn(&mut self, level: f32, dt: f32, steady: bool) {
        if level < SILENCE_DB {
            self.low_for = 0.0;
            return;
        }
        if self.reference.is_nan() {
            self.reference = level;
            return;
        }
        if level > self.reference - 3.0 {
            self.sinking = false;
        }
        if level > self.reference {
            self.reference += (level - self.reference) * follow(dt, 0.3);
            self.low_for = 0.0;
            return;
        }
        self.low_for = if steady && self.reference - level > 9.0 {
            self.low_for + dt
        } else {
            0.0
        };
        self.sinking |= self.low_for > 2.0;
        let tau = if self.sinking { 1.0 } else { 30.0 };
        self.reference += (level - self.reference) * follow(dt, tau);
    }

    /// `level` (dB) against the loud parts: 1 at their level, 0 [`RANGE_DB`]
    /// under it.
    fn read(&self, level: f32) -> f32 {
        if self.reference.is_nan() || level < SILENCE_DB {
            return 0.0;
        }
        (1.0 + (level - self.reference) / RANGE_DB).clamp(0.0, 1.0)
    }
}

/// How an [`Onset`] listens to its band.
#[derive(Debug, Clone, Copy)]
struct Ear {
    /// How fast the band's recent level follows it, seconds: a hit is a rise
    /// above it.
    follow: f32,
    /// The least rise that can be a hit, dB.
    floor: f32,
    /// The least time between two hits, seconds.
    rest: f32,
}

const KICK: Ear = Ear {
    follow: 0.25,
    floor: 4.0,
    rest: 0.22,
};
const SNARE: Ear = Ear {
    follow: 0.15,
    floor: 5.0,
    rest: 0.12,
};
const HAT: Ear = Ear {
    follow: 0.08,
    floor: 5.0,
    rest: 0.06,
};

/// Finds where a band is struck: the band jumping above its own recent level
/// by more than it usually does. In decibels, so a hit stands out the same at
/// any volume, and against a bar that adapts to the track, so a busy line
/// does not fire on every note and a sparse one is not missed.
#[derive(Debug, Clone, Copy)]
struct Onset {
    ear: Ear,
    /// The band's recent level, dB (NaN until the first hop).
    slow: f32,
    /// How far it usually rises above it: mean and mean deviation.
    rise_mean: f32,
    rise_dev: f32,
    since: f32,
}

impl Onset {
    fn new(ear: Ear) -> Self {
        Onset {
            ear,
            slow: f32::NAN,
            rise_mean: 0.0,
            rise_dev: 1.0,
            since: 1.0,
        }
    }

    /// One hop at `level` dB. How hard the band was struck, if it was;
    /// `live` says whether it carries enough of the music to count.
    fn step(&mut self, level: f32, dt: f32, live: bool) -> Option<f32> {
        if self.slow.is_nan() {
            // The first hop sets the level: music already playing is no hit.
            self.slow = level;
        }
        let rise = (level - self.slow).max(0.0);
        self.slow += (level - self.slow) * follow(dt, self.ear.follow);
        self.since += dt;
        let bar = (self.rise_mean + 1.8 * self.rise_dev).max(self.ear.floor);
        let hit = live && rise > bar && self.since > self.ear.rest;
        self.rise_mean += (rise - self.rise_mean) * follow(dt, 1.5);
        self.rise_dev += ((rise - self.rise_mean).abs() - self.rise_dev) * follow(dt, 1.5);
        if !hit {
            return None;
        }
        self.since = 0.0;
        Some((0.55 + (rise - bar) / 12.0).clamp(0.55, 1.0))
    }
}

/// One band of the sound: its filters, how loud it is, and its hits.
#[derive(Debug, Clone)]
struct Band {
    filters: Vec<Biquad>,
    /// Sum of squares over the hop being gathered.
    sum: f32,
    /// Fast attack, slower release: the lights jump with the music and settle.
    env: f32,
    gain: Gain,
    onset: Option<Onset>,
}

impl Band {
    fn new(filters: Vec<Biquad>, ear: Option<Ear>) -> Self {
        Band {
            filters,
            sum: 0.0,
            env: 0.0,
            gain: Gain::new(),
            onset: ear.map(Onset::new),
        }
    }

    fn push(&mut self, s: f32) {
        let s = self.filters.iter_mut().fold(s, |s, f| f.run(s));
        self.sum += s * s;
    }

    /// Close a hop of `n` samples. Its level, in dB. `steady` is whether the
    /// groove carries on (see [`Gain::learn`]).
    fn close(&mut self, n: usize, dt: f32, steady: bool) -> f32 {
        let rms = (self.sum / n as f32).sqrt();
        self.sum = 0.0;
        let tau = if rms > self.env { 0.01 } else { 0.25 };
        self.env += (rms - self.env) * follow(dt, tau);
        self.gain.learn(db(self.env), dt, steady);
        db(rms)
    }

    /// Whether it was struck in the hop at `level` dB.
    fn hit(&mut self, level: f32, dt: f32, warm: bool) -> Option<f32> {
        // Hiss in a pause is no hit: the band must carry some of the music.
        let live = warm && self.gain.read(level) > 0.1;
        self.onset.as_mut()?.step(level, dt, live)
    }

    /// How loud it plays now, against how loud the track plays it.
    fn level(&self) -> f32 {
        self.gain.read(db(self.env))
    }
}

/// Tempos the clock looks for, seconds a beat: 180 down to 80 beats a minute.
const FASTEST: f32 = 60.0 / 180.0;
const SLOWEST: f32 = 0.75;
/// How far a kick may fall from its beat and still be on it, as a share of a
/// beat.
const ON_BEAT: f32 = 0.18;
/// How sure the clock must be before it plays the beats itself.
const SURE: i32 = 4;
const SUREST: i32 = 8;
/// Beats in a row with nothing struck on them before the clock lets go: a
/// kick on every other beat (rock's one and three) keeps it, a breakdown
/// with nothing but a pad does not.
const MISSES: u32 = 3;
/// How early a beat goes out until the lights say how long they take, so it
/// reaches the bulb as the kick reaches the ear: the trip over the network
/// and the LEDs' own rise.
pub const LEAD: f32 = 0.05;
/// The earliest a beat may go out. Past this the clock would be guessing
/// more than a short way ahead of what it heard.
pub const MOST_LEAD: f32 = 0.25;
/// How far back the hits the tempo is read from go, seconds.
const MEMORY: f32 = 6.0;
/// The most hits the tempo is read from: a snare roll must not make
/// reading it slow.
const MEMORY_HITS: usize = 48;
/// The least time between two readings of the tempo while the clock is
/// unsure, seconds.
const REREAD: f32 = 0.25;
/// Seconds of silence after which the clock forgets the beat altogether
/// (counted once the music has faded out, some seven tenths in).
const QUIET_RESET: f32 = 0.75;
/// How often a sure clock checks that the hits still fall on its beats,
/// seconds.
const RECHECK: f32 = 1.0;

/// What a hit is worth to the clock, by what struck it. The kick sets the
/// beat; a snare, or a strum or a chord heard in the whole mix, keeps a
/// track without a kick in time, and the kick's own clock in phase through
/// a fill. Hi-hats do not count: they come so thick that they would keep
/// any clock going.
const KICK_WEIGHT: f32 = 1.0;
const SNARE_WEIGHT: f32 = 0.6;
const PULSE_WEIGHT: f32 = 0.5;
/// The analyser's clock moves back by this many seconds whenever it reaches
/// twice that, so an `f32` keeps it to the sample after hours of music.
const CLOCK_SPAN: f32 = 600.0;
/// The furthest past the last sample heard the clock reads the music to
/// have got: about the longest gap between two packets of sound.
const AHEAD: f32 = 0.04;
/// A gap between two packets this much longer than usual is a stall, not
/// the two clocks drifting apart.
const STALL: f32 = 0.2;

/// The beat's clock: the tempo, read from where the hits fall, and when the
/// next beat is due.
///
/// Once the hits agree on a tempo, the show plays the beats from this clock
/// rather than from each kick as it is heard. A beat can then go out a little
/// early, so it lands on the bulbs with the kick instead of after it, and a
/// kick the ear hears but the bass did not show still gets its beat. Every
/// hit on the beat pulls the clock back into phase, the kick hardest; when
/// they stop coming (a breakdown, the next track) it lets go, and every kick
/// plays as it is heard until a tempo is found again. A track with no kick
/// at all (a band without drums, a ballad) is read from its snares and its
/// strums alone, and its beats come only from the clock.
#[derive(Debug, Clone, Default)]
struct Tempo {
    /// When the recent hits fell, seconds, and what each is worth.
    hits: VecDeque<(f32, f32)>,
    /// Seconds a beat; 0 while none is known.
    period: f32,
    /// When the next beat is due.
    next: f32,
    sure: i32,
    /// Beats in a row that came with nothing struck on them.
    missed: u32,
    /// The next beat has already been played.
    played: bool,
    /// How hard the kicks have been landing.
    strength: f32,
    /// When the tempo was last read.
    read_at: f32,
}

impl Tempo {
    /// Move every time it holds `by` seconds earlier, with the clock.
    fn rewind(&mut self, by: f32) {
        for (k, _) in &mut self.hits {
            *k -= by;
        }
        self.next -= by;
        self.read_at -= by;
    }

    fn locked(&self) -> bool {
        self.period > 0.0 && self.sure >= SURE
    }

    fn remember(&mut self, t: f32, weight: f32) {
        self.hits.push_back((t, weight));
        while self.hits.len() > MEMORY_HITS
            || self.hits.front().is_some_and(|&(k, _)| t - k > MEMORY)
        {
            self.hits.pop_front();
        }
    }

    /// A kick heard at `t`. Whether to play it as a beat.
    fn kick(&mut self, t: f32, strength: f32) -> bool {
        self.remember(t, KICK_WEIGHT);
        self.strength += (strength - self.strength) * 0.3;
        if self.period > 0.0 && (t - self.next).abs() < ON_BEAT * self.period {
            // On the beat: pull the clock into phase and move on to the next.
            let play = !self.locked() || !self.played;
            self.land(t, KICK_WEIGHT);
            return play;
        }
        if !self.locked() {
            self.find(t);
            return true;
        }
        // Off the beat. A kick between two beats (a syncopation) still plays;
        // one just beside a beat the clock already played is the same beat.
        self.sure -= 1;
        let from_beat = (t - (self.next - self.period))
            .abs()
            .min((self.next - t).abs());
        from_beat > 0.3 * self.period
    }

    /// Something other than a kick struck at `t`, worth `weight`: it is
    /// never played as heard, it only keeps the clock.
    ///
    /// Its window is narrower than a kick's, and one off the beat costs the
    /// clock nothing: snares and strums fall between the beats all the time.
    fn hit(&mut self, t: f32, weight: f32) {
        self.remember(t, weight);
        if self.period > 0.0 && (t - self.next).abs() < 0.6 * ON_BEAT * self.period {
            self.land(t, weight);
        } else if !self.locked() && t - self.read_at > REREAD {
            self.find(t);
        }
    }

    /// A hit worth `weight` landed on the beat due, at `t`: pull the clock
    /// towards it and move on to the next.
    fn land(&mut self, t: f32, weight: f32) {
        let e = t - self.next;
        self.sure = (self.sure + 1).min(SUREST);
        self.missed = 0;
        self.period = (self.period + 0.05 * weight * e).clamp(FASTEST, SLOWEST);
        self.next += 0.5 * weight * e + self.period;
        self.played = false;
    }

    /// Read the tempo from the recent hits, and set the clock going on the
    /// beats they fall on, the first one after `t`.
    fn find(&mut self, t: f32) {
        self.read_at = t;
        let Some((p, anchor)) = self.grid() else {
            return;
        };
        self.period = p;
        // The first beat of that grid after `t`; one just before it is the
        // beat `t` struck.
        self.next = anchor + (((t - anchor) / p + ON_BEAT).floor() + 1.0) * p;
        self.sure = (self.on_grid(anchor, p).0 as i32 - 2).clamp(0, SUREST);
        self.missed = 0;
        self.played = false;
    }

    /// The tempo the recent hits keep, and one of the hits its beats fall
    /// on; none while too few have been heard.
    ///
    /// Every pair of hits votes for the tempos it is a whole number of beats
    /// of, as much as both are worth; a pair one beat apart counts most, so
    /// half the tempo (which fits every pair too) does not win. A slight
    /// lean towards 120 beats a minute settles the rest. Then the beats are
    /// laid where the most is struck on them: on the kicks, in a track that
    /// has them, rather than on the offbeat strums.
    fn grid(&self) -> Option<(f32, f32)> {
        if self.hits.len() < 4 {
            return None;
        }
        let hits: Vec<(f32, f32)> = self.hits.iter().copied().collect();
        let fit = |d: f32, p: f32| {
            let k = (d / p).round();
            if !(1.0..=4.0).contains(&k) {
                return 0.0;
            }
            let off = (d - k * p) / 0.02;
            (-off * off).exp() / k
        };
        const STEPS: usize = 100;
        let mut best = (0.0, 0.0);
        for s in 0..=STEPS {
            let p = FASTEST + (SLOWEST - FASTEST) * s as f32 / STEPS as f32;
            let lean = (p / 0.5).log2() / 0.7;
            let mut score = 0.0;
            for (i, &(a, wa)) in hits.iter().enumerate() {
                for &(b, wb) in &hits[i + 1..] {
                    score += wa * wb * fit(b - a, p);
                }
            }
            score *= (-0.5 * lean * lean).exp();
            if score > best.1 {
                best = (p, score);
            }
        }
        let p = best.0;
        if p == 0.0 {
            return None;
        }
        hits.iter()
            .map(|&(k, _)| (k, self.on_grid(k, p).1))
            .max_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)))
            .map(|(k, _)| (p, k))
    }

    /// The recent hits a grid of beats `p` apart through `at` passes by:
    /// how many, and what they are worth.
    fn on_grid(&self, at: f32, p: f32) -> (usize, f32) {
        self.hits
            .iter()
            .filter(|&&(k, _)| {
                let x = (at - k) / p;
                (x - x.round()).abs() * p < 0.03
            })
            .fold((0, 0.0), |(n, w), &(_, wk)| (n + 1, w + wk))
    }

    /// While the clock is sure, check now and then, at `t`, that the hits
    /// still fall on its beats. When the music changes under it (the next
    /// track, a tempo change, a mix from one into the other) the hits soon
    /// fall on another grid far better than on its own, and the clock is
    /// set going on that one instead of playing the old track's beats over
    /// the new.
    fn recheck(&mut self, t: f32) {
        if !self.locked() || t - self.read_at < RECHECK {
            return;
        }
        self.read_at = t;
        let Some((p, anchor)) = self.grid() else {
            return;
        };
        // Twice the tempo, or half, is the same music counted another way:
        // the offbeats always pass by a grid twice as fast.
        let ratio = p / self.period;
        let same_music = [0.5, 2.0].iter().any(|r| (ratio / r - 1.0).abs() < 0.06);
        let theirs = self.on_grid(anchor, p).1;
        let ours = self.on_grid(self.next, self.period).1;
        if !same_music && theirs > 1.5 * ours + KICK_WEIGHT {
            self.find(t);
        }
    }

    /// The clock at `now`. Whether a beat is to be played from it, `lead`
    /// seconds ahead of when it is due.
    fn tick(&mut self, now: f32, live: bool, lead: f32) -> bool {
        if self.period == 0.0 {
            return false;
        }
        while now > self.next + ON_BEAT * self.period {
            // A beat went by with nothing struck on it.
            self.missed += 1;
            self.next += self.period;
            self.played = false;
        }
        if self.missed >= MISSES {
            *self = Tempo {
                hits: std::mem::take(&mut self.hits),
                strength: self.strength,
                read_at: self.read_at,
                ..Tempo::default()
            };
            return false;
        }
        if live && self.locked() && !self.played && self.next - now <= lead {
            self.played = true;
            return true;
        }
        false
    }

    /// Seconds from `now` until the clock plays its next beat, `lead`
    /// seconds ahead of it; none while it is unsure.
    fn due(&self, now: f32, lead: f32) -> Option<f32> {
        if !self.locked() {
            return None;
        }
        let next = if self.played {
            self.next + self.period
        } else {
            self.next
        };
        Some((next - lead - now).max(0.0))
    }
}

/// What the music is doing, measured over the sound captured since the
/// last frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Levels {
    /// How loud the music is against how loud the track plays, 0..=1. The
    /// volume knob does not move it; a quiet passage does.
    pub energy: f32,
    /// The same, for the bass alone.
    pub bass: f32,
    /// Play a beat now, this hard (0..=1): a kick just heard, or the tempo's
    /// next beat, a touch early.
    pub beat: Option<f32>,
    /// A kick was really heard (a beat from the clock alone is not one).
    pub kick: bool,
    /// The snare (or a clap) was struck, this hard.
    pub snare: Option<f32>,
    /// A hi-hat or a cymbal was struck, this hard.
    pub hat: Option<f32>,
    /// Something else started at once across the mix (a chord, a stab, a
    /// strum), this hard.
    pub pulse: Option<f32>,
    /// Seconds a beat, once the tempo is sure.
    pub tempo: Option<f32>,
    /// How loud the middle (voices, snares, guitars) and the treble play,
    /// each against its own level in the track, 0..=1.
    pub mid: f32,
    pub treble: f32,
    /// Seconds until the clock plays its next beat, once the tempo is sure:
    /// when to look again so it goes out on time.
    pub due: Option<f32>,
}

/// The broadband ear: a strum, a chord, a stab, anything that starts at
/// once across the mix.
const PULSE: Ear = Ear {
    follow: 0.15,
    floor: 4.0,
    rest: 0.1,
};

/// Turns raw samples into [`Levels`].
///
/// The sound is split into three bands, each with its own instrument: under
/// 150 Hz the kick and the bass line, from 1 to 4 kHz the snare and claps, over
/// 7 kHz the hi-hats and cymbals. Each is read against its own level in the
/// track (see [`Gain`]), so the show looks the same with the volume at 10% or
/// at 100%, and from one master to the next.
#[derive(Debug, Clone)]
pub struct Analyzer {
    rate: f32,
    hop: usize,
    count: usize,
    mix: Band,
    low: Band,
    mid: Band,
    high: Band,
    /// Seconds of sound heard, give or take whole [`CLOCK_SPAN`]s.
    time: f32,
    /// The caller's clock, seconds, moved back with `time`.
    wall: f32,
    /// How far the sound heard runs behind the caller's clock, at its
    /// least (NaN until known): the sound arrives in packets, so the clock
    /// is read between them as this far ahead of the last one.
    behind: f32,
    /// Seconds since a kick was last heard.
    since_kick: f32,
    /// Seconds the music has not been playing.
    quiet_for: f32,
    tempo: Tempo,
    /// How early the clock's beats go out (see [`Analyzer::set_lead`]).
    lead: f32,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(1) as f32;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        Analyzer {
            rate,
            hop: ((rate * HOP_SECS) as usize).max(1),
            count: 0,
            mix: Band::new(Vec::new(), Some(PULSE)),
            // Two in a row, so the vocals and the snare's body stay out.
            low: Band::new(
                vec![
                    Biquad::new(Pass::Low, rate, 150.0, q),
                    Biquad::new(Pass::Low, rate, 150.0, q),
                ],
                Some(KICK),
            ),
            // Cut above too, or the hi-hats ring in it as snares.
            mid: Band::new(
                vec![
                    Biquad::new(Pass::Band, rate, 2200.0, 0.7),
                    Biquad::new(Pass::Low, rate, 4500.0, q),
                ],
                Some(SNARE),
            ),
            high: Band::new(
                vec![
                    Biquad::new(Pass::High, rate, 7000.0, q),
                    Biquad::new(Pass::High, rate, 7000.0, q),
                ],
                Some(HAT),
            ),
            time: 0.0,
            wall: 0.0,
            behind: f32::NAN,
            since_kick: 10.0,
            quiet_for: 0.0,
            tempo: Tempo::default(),
            lead: LEAD,
        }
    }

    /// How early the clock's beats go out, seconds: as long as the slowest
    /// light takes to show one, less however late the sound reaches the ear
    /// (see [`Stage::lead`]).
    pub fn set_lead(&mut self, lead: f32) {
        self.lead = if lead.is_finite() {
            lead.clamp(0.0, MOST_LEAD)
        } else {
            LEAD
        };
    }

    /// Read the mono samples captured since the last call, as if they came
    /// in exactly as fast as they play.
    pub fn feed(&mut self, samples: &[f32]) -> Levels {
        self.feed_after(samples, samples.len() as f32 / self.rate)
    }

    /// Read the mono samples captured since the last call, `elapsed`
    /// seconds of the caller's clock ago.
    ///
    /// The sound comes in packets, a few hundredths of a second apart. The
    /// caller's clock says how far the music has played since the last one,
    /// so a beat due between two packets goes out on time rather than with
    /// the next.
    pub fn feed_after(&mut self, samples: &[f32], elapsed: f32) -> Levels {
        if elapsed.is_finite() {
            self.wall += elapsed.clamp(0.0, 1.0);
        }
        let mut levels = Levels::default();
        for &s in samples {
            let s = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.mix.push(s);
            self.low.push(s);
            self.mid.push(s);
            self.high.push(s);
            self.count += 1;
            if self.count == self.hop {
                self.step(&mut levels);
            }
        }
        // Where the music has got to by the caller's clock: a little past
        // the last sample, never further than a packet's worth.
        let gap = self.wall - self.time;
        if self.behind.is_nan() || gap < self.behind || gap > self.behind + STALL {
            // Earlier than ever, or a stall (nothing plays, and some systems
            // then send nothing): the sound sets the clock again.
            self.behind = gap;
        } else {
            // The two clocks drift apart, slowly.
            self.behind += (gap - self.behind) * follow(elapsed.max(0.0), 10.0);
        }
        let now = self.time + (gap - self.behind).clamp(0.0, AHEAD);
        if now > self.time && self.tempo.tick(now, self.live(), self.lead) {
            levels.beat = Some(levels.beat.unwrap_or(0.0).max(self.tempo.strength));
        }
        levels.energy = self.mix.level();
        levels.bass = self.low.level();
        levels.mid = self.mid.level();
        levels.treble = self.high.level();
        levels.tempo = self.tempo.locked().then_some(self.tempo.period);
        // Into silence no beat plays, and nothing is worth waking for.
        levels.due = self
            .live()
            .then(|| self.tempo.due(now, self.lead))
            .flatten();
        levels
    }

    /// Whether the music plays: no beat plays into silence, however sure
    /// the clock is. The envelope, not the last hop: just before a kick is
    /// the quietest moment.
    fn live(&self) -> bool {
        self.mix.level() > 0.2
    }

    /// Close one hop, adding what it heard to `levels`.
    fn step(&mut self, levels: &mut Levels) {
        let n = self.count;
        self.count = 0;
        let dt = n as f32 / self.rate;
        self.time += dt;
        if self.time > 2.0 * CLOCK_SPAN {
            self.time -= CLOCK_SPAN;
            self.wall -= CLOCK_SPAN;
            self.tempo.rewind(CLOCK_SPAN);
        }
        let steady = self.since_kick < 1.5;
        let all = self.mix.close(n, dt, steady);
        let low = self.low.close(n, dt, steady);
        let mid = self.mid.close(n, dt, steady);
        let high = self.high.close(n, dt, steady);

        // The first half second only learns the levels.
        let warm = self.time > 0.5;
        let kick = self.low.hit(low, dt, warm);
        let hat = self.high.hit(high, dt, warm);
        // A kick's click reaches the snare's band too, and so does the
        // bottom of a hi-hat: a hit that sounds mostly in the treble is a
        // hat. (A snare's wires reach the treble as well, but its body
        // keeps the middle the louder.) Neither may count as a snare, nor
        // keep the next real one waiting out its rest.
        let hat_led = hat.is_some() && high > mid + 6.0;
        let snare = self.mid.hit(mid, dt, warm && kick.is_none() && !hat_led);
        self.since_kick = if kick.is_some() {
            0.0
        } else {
            self.since_kick + dt
        };

        // Anything else that starts at once across the mix, for a track
        // the drums do not carry.
        let pulse = self
            .mix
            .hit(all, dt, warm && kick.is_none() && snare.is_none());

        let mut beat = None;
        if let Some(s) = kick {
            levels.kick = true;
            if self.tempo.kick(self.time, s) {
                beat = Some(s);
            }
        } else if snare.is_some() {
            self.tempo.hit(self.time, SNARE_WEIGHT);
        } else if pulse.is_some() {
            self.tempo.hit(self.time, PULSE_WEIGHT);
        }
        // Silence is the end of a track, or a pause: what came before tells
        // nothing of the beat to come.
        self.quiet_for = if self.live() {
            0.0
        } else {
            self.quiet_for + dt
        };
        if self.quiet_for > QUIET_RESET && !self.tempo.hits.is_empty() {
            self.tempo = Tempo::default();
        }
        self.tempo.recheck(self.time);
        if self.tempo.tick(self.time, self.live(), self.lead) {
            beat = Some(beat.unwrap_or(0.0).max(self.tempo.strength));
        }
        let most = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        levels.beat = most(levels.beat, beat);
        levels.snare = most(levels.snare, snare);
        levels.hat = most(levels.hat, hat);
        levels.pulse = most(levels.pulse, pulse);
    }
}

/// Commands this close together or closer make a bulb quick enough to punch
/// the beat; slower ones wash the room instead.
const QUICK: Duration = Duration::from_millis(60);

/// Further in front of the screen than this, in screen half-widths, a light
/// is lighting the room rather than the stage: it washes.
const FAR: f32 = 2.5;

/// A light as the stage sees it: where it stands, and how fast it answers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fixture {
    /// Across the room as the viewer sees it, in screen half-widths: -1 is
    /// the screen's left end, 1 its right.
    pub x: f32,
    /// How far in front of the screen, in screen half-widths; negative is
    /// behind it.
    pub depth: f32,
    /// 0 on the floor, 1 at the ceiling.
    pub height: f32,
    /// The shortest gap the bulb takes between two commands.
    pub interval: Duration,
    /// How long a command takes to show: the trip to the bulb and its LEDs'
    /// own rise.
    pub latency: Duration,
}

/// How far either side of where it stands a light with several zones
/// spreads them, in screen half-widths: a strip or a few panels, a stretch
/// of wall rather than the whole room.
const ZONE_SPAN: f32 = 0.5;

impl Fixture {
    /// Where `light` stands, seen from the seat facing `screen`.
    pub fn place(screen: &Screen, light: &Light, interval: Duration, latency: Duration) -> Self {
        let half = (screen.width * 0.5).max(f32::EPSILON);
        let rel = light.pos - screen.center;
        Fixture {
            x: rel.dot(screen.right_axis()) / half,
            depth: rel.dot(screen.facing()) / half,
            height: light.height.clamp(0.0, 1.0),
            interval,
            latency,
        }
    }

    /// The `n` zones of a light that colours several parts of itself (a
    /// strip's segments, a layout's panels), left to right, spread across
    /// the wall around where it stands: on the stage each is a light of its
    /// own, so a chase runs along the strip.
    pub fn zones(self, n: usize) -> Vec<Fixture> {
        if n <= 1 {
            return vec![self];
        }
        (0..n)
            .map(|i| Fixture {
                x: self.x + ZONE_SPAN * (2.0 * i as f32 / (n - 1) as f32 - 1.0),
                ..self
            })
            .collect()
    }

    /// `n` quick lights in a row behind the screen, left to right: what a
    /// room with no plan looks like.
    pub fn row(n: usize) -> Vec<Fixture> {
        (0..n)
            .map(|i| Fixture {
                x: if n > 1 {
                    -1.0 + 2.0 * i as f32 / (n - 1) as f32
                } else {
                    0.0
                },
                depth: 0.0,
                height: 0.5,
                interval: Duration::ZERO,
                latency: Duration::from_secs_f32(LEAD),
            })
            .collect()
    }

    /// Whether the bulb answers fast enough to strike the beat.
    pub fn quick(&self) -> bool {
        self.interval <= QUICK
    }
}

/// What a light plays on the stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Punches the beat, in patterns that move across the room.
    Punch,
    /// Bathes the room in the harmony's colour, swelling with the bass.
    Wash,
    /// Answers the snare and shimmers with the hi-hats.
    Accent,
}

/// Give every light its part.
///
/// The quick lights by the screen are the stage: they punch. Slow ones (a
/// Hue bridge cannot keep up with a beat) and those far out in the room
/// wash. With three punching lights or more, one at the ceiling takes the
/// snare instead, and with four or more the highest does: enough are left
/// to make the patterns travel. There is always someone on the beat.
fn cast(fixtures: &[Fixture]) -> Vec<Part> {
    let mut parts: Vec<Part> = fixtures
        .iter()
        .map(|f| {
            if f.quick() && f.depth <= FAR {
                Part::Punch
            } else {
                Part::Wash
            }
        })
        .collect();
    if !parts.contains(&Part::Punch) {
        // The quickest takes the beat, the nearest of those.
        let best = (0..fixtures.len()).min_by(|&a, &b| {
            let (fa, fb) = (&fixtures[a], &fixtures[b]);
            fa.interval
                .cmp(&fb.interval)
                .then(fa.depth.total_cmp(&fb.depth))
        });
        if let Some(i) = best {
            parts[i] = Part::Punch;
        }
    }
    let punches: Vec<usize> = (0..fixtures.len())
        .filter(|&i| parts[i] == Part::Punch)
        .collect();
    // The highest, the one nearest the middle if they are level.
    let highest = punches.iter().copied().max_by(|&a, &b| {
        let (fa, fb) = (&fixtures[a], &fixtures[b]);
        fa.height
            .total_cmp(&fb.height)
            .then(fb.x.abs().total_cmp(&fa.x.abs()))
    });
    if let Some(i) = highest {
        let up_high = punches.len() >= 3 && fixtures[i].height >= 0.8;
        if up_high || punches.len() >= 4 {
            parts[i] = Part::Accent;
        }
    }
    parts
}

/// How the punching lights share out the beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pattern {
    /// All of them, every beat.
    Together,
    /// One after another across the room, left to right.
    Chase,
    /// From the middle out to the ends, ring by ring.
    CentreOut,
    /// Across and back again.
    Bounce,
    /// Every other light, every other beat.
    Alternate,
    /// From the ends in to the middle, ring by ring.
    OutsideIn,
    /// The left half on the kick, the right half on the snare.
    Split,
}

/// How far light `rank` of `k` (left to right) stands from the middle, in
/// rings: 0 for the middle one (or two). And how many rings there are.
fn ring(rank: usize, k: usize) -> (usize, usize) {
    ((2 * rank).abs_diff(k.saturating_sub(1)) / 2, k.div_ceil(2))
}

impl Pattern {
    /// The patterns `k` punching lights can play, in the order the show
    /// moves through them. The ones that need a middle and two ends wait for
    /// three lights.
    fn for_lights(k: usize) -> &'static [Pattern] {
        match k {
            0 | 1 => &[Pattern::Together],
            2 => &[Pattern::Together, Pattern::Alternate, Pattern::Split],
            _ => &[
                Pattern::Together,
                Pattern::Chase,
                Pattern::CentreOut,
                Pattern::Alternate,
                Pattern::Bounce,
                Pattern::OutsideIn,
                Pattern::Split,
            ],
        }
    }

    /// How much of beat `b` the light `rank` of `k` (left to right) takes.
    fn beat(self, rank: usize, k: usize, b: u32) -> f32 {
        let b = b as usize;
        let (r, rings) = ring(rank, k);
        let on = match self {
            Pattern::Together => true,
            Pattern::Chase => rank == b % k,
            Pattern::CentreOut => r == b % rings,
            Pattern::OutsideIn => r == rings - 1 - b % rings,
            Pattern::Bounce => {
                let lap = (2 * k).saturating_sub(2).max(1);
                let at = b % lap;
                rank == if at < k { at } else { lap - at }
            }
            Pattern::Alternate => rank % 2 == b % 2,
            Pattern::Split => rank < k.div_ceil(2),
        };
        if on { 1.0 } else { 0.0 }
    }

    /// How much of a snare the light `rank` of `k` takes, after beat `b`.
    fn snare(self, rank: usize, k: usize, b: u32) -> f32 {
        match self {
            Pattern::Together => 0.4,
            Pattern::Split => {
                if rank < k.div_ceil(2) {
                    0.0
                } else {
                    1.0
                }
            }
            // The lights the last beat left dark answer it.
            _ => 0.5 * (1.0 - self.beat(rank, k, b)),
        }
    }

    /// Whether light `rank` of `k` strikes in the second of the two hit
    /// colours rather than the first: the two sides of a pattern that has
    /// sides answer each other in colour too.
    fn second(self, rank: usize, k: usize) -> bool {
        match self {
            Pattern::Alternate => rank % 2 == 1,
            Pattern::Split => rank >= k.div_ceil(2),
            Pattern::CentreOut | Pattern::OutsideIn => ring(rank, k).0 % 2 == 1,
            _ => false,
        }
    }
}

/// Where the track is, as a lighting desk would read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
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
const PHRASE_BEATS: u32 = 16;
/// Phrases the show stays on one pattern: eight bars.
const PHRASES_PER_LOOK: u32 = 2;
/// Phrases the show stays on one harmony, unless a drop moves it on.
const PHRASES_PER_HARMONY: u32 = 4;
/// How long a build takes to reach white, seconds.
const BUILD_SECS: f32 = 8.0;
/// A white flash's colour temperature.
const WHITE_KELVIN: u16 = 6500;
/// The quickest a quick bulb's hit fades, seconds. Slower bulbs take longer,
/// and every hit stretches to fill most of a slow beat.
const QUICKEST_FADE: f32 = 0.08;
/// Seconds for a breakdown's colour to drift to its other shade and back.
const CALM_DRIFT_SECS: f32 = 30.0;
/// Seconds for a breakdown's slow swell to roll across the room.
const CALM_WAVE_SECS: f32 = 6.0;
/// A calm shorter than this, seconds, was the ear losing the beat for a
/// moment rather than the track changing: the groove picks up where it
/// left off, in the same colours.
const CALM_STUMBLE: f32 = 4.0;
/// Beats compared on either side of where a phrase may have turned.
const SIDE: usize = 4;
/// How far the sound must move between those two sides (mix, bass, middle
/// and treble together, each 0..=1) for a new phrase to have begun: new
/// instruments coming in, or going.
const TURN: f32 = 0.2;
/// Beats a phrase found late must have run before the colours still move
/// on for it; nearer the last change, only the count is set right.
const LATE_TURN: u32 = 6;

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
/// the party harmonies at a time, in two roles: the room rests in a back
/// colour and the punches strike in the other two, fading back into it. The
/// roles turn with every phrase, the harmony moves on every few phrases and
/// at every drop, and the beats play only the brightness, from near dark, and
/// where it lands in the room.
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

    fn pattern(&self) -> Pattern {
        if self.section == Section::Drop {
            return Pattern::Together;
        }
        let all = Pattern::for_lights(self.punches);
        all[(self.phrases / PHRASES_PER_LOOK) as usize % all.len()]
    }

    fn harmony(&self) -> &'static [Rgb; 3] {
        &PARTY[(self.phrases / PHRASES_PER_HARMONY) as usize % PARTY.len()]
    }

    /// The colours of the phrase: the back colour the room rests in, and the
    /// two the punches strike in. Each colour of the harmony takes each role
    /// in turn, one phrase after another.
    fn roles(&self) -> (Rgb, Rgb, Rgb) {
        let h = self.harmony();
        let p = self.phrases as usize;
        (h[(p + 1) % 3], h[p % 3], h[(p + 2) % 3])
    }

    /// Whether the white flashes are on. How often they may go out is not
    /// the show's to decide: the show's guard
    /// ([`crate::safety::FlashGuard::show`]) keeps them to three a second,
    /// protection on or off.
    fn white(&self, settings: &ShowSettings) -> bool {
        settings.flashes
    }

    fn enter(&mut self, section: Section) {
        // What the beats sounded like is only compared within a groove.
        if section != Section::Groove {
            self.sounds.clear();
            self.turns = [0.0; 2];
        }
        self.section = section;
        self.held = 0.0;
        self.roll = 0;
        self.roll_due = 0.0;
        if section == Section::Hold {
            for lamp in &mut self.lamps {
                lamp.flash = 0.0;
                lamp.white = 0.0;
            }
        }
    }

    /// The beat landing now is the first of a phrase. The clock may have
    /// played it already, a little early: then it is counted here.
    fn start_phrase(&mut self, levels: &Levels) {
        if levels.beat.is_some() {
            self.starting = true;
        } else {
            self.starting = false;
            self.beat = 0;
        }
    }

    /// The colours move on to the next phrase's.
    fn turn(&mut self) {
        self.phrases += 1;
        self.since_turn = 0;
    }

    /// Count a beat: the first of a phrase after a fresh start, otherwise
    /// the next. A phrase full, the colours move on, and a drop gives way to
    /// the patterns.
    fn count_beat(&mut self) {
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
    fn mark(&mut self) -> Option<u32> {
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
    fn follow_track(&mut self, settings: &ShowSettings, levels: &Levels, struck: bool) {
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
    fn drop(&mut self, settings: &ShowSettings, levels: &Levels) {
        self.enter(Section::Drop);
        self.phrases = (self.phrases / PHRASES_PER_HARMONY + 1) * PHRASES_PER_HARMONY;
        self.since_turn = 0;
        self.start_phrase(levels);
        let white = self.white(settings);
        for lamp in &mut self.lamps {
            lamp.hit(1.0, white);
        }
    }

    /// Advance by `dt` seconds with what the music is doing.
    pub fn frame(&mut self, settings: &ShowSettings, levels: Levels, dt: f32) -> Vec<BulbOutput> {
        (self.lead, self.waits) = timing(&self.fixtures, settings.delay());
        self.due = levels.due;
        // A hit fades out just before the next beat comes, like a pumping
        // sidechain, but never quicker than the bulb can show.
        let pump = self.period / 3.0;
        for lamp in &mut self.lamps {
            let fade = lamp.fade.max(pump);
            lamp.flash *= (-dt / fade).exp();
            lamp.white *= (-dt / (0.6 * fade)).exp();
            lamp.sharp = false;
        }
        self.held += dt;
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

        let outputs = self.light(settings);
        for (sharp, lamp) in self.sharp.iter_mut().zip(&self.lamps) {
            *sharp = lamp.sharp;
        }
        self.lag.run(dt, outputs, &mut self.sharp, &self.waits)
    }

    /// The beat is on: punch it in the current pattern, and answer the snare.
    fn groove(&mut self, settings: &ShowSettings, levels: &Levels) {
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
            let pattern = self.pattern();
            let (k, b) = (self.punches, self.beat);
            for lamp in &mut self.lamps {
                match lamp.part {
                    Part::Punch => lamp.hit(s * pattern.beat(lamp.rank, k, b), false),
                    // The wash only feels the strongest beats, softly.
                    Part::Wash => lamp.hit(0.25 * (s - 0.6).max(0.0) / 0.4, false),
                    Part::Accent => {}
                }
            }
        }
        if let Some(s) = levels.snare {
            // In a drop, and in loud music, the snare cracks white.
            let loud = self.section == Section::Drop || self.swell > 0.75;
            let has_accent = self.lamps.iter().any(|l| l.part == Part::Accent);
            let white =
                loud && (has_accent || self.section == Section::Drop) && self.white(settings);
            if has_accent {
                self.accent(s, white);
            } else {
                let (pattern, k, b) = (self.pattern(), self.punches, self.beat);
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
    fn build(&mut self, settings: &ShowSettings, levels: &Levels, dt: f32) {
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
    fn calm(&mut self, levels: &Levels) {
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

    /// How loud the music plays, as far as a breakdown cares: nothing in a
    /// quiet one, 1 when it plays near the top of its range.
    fn loud(&self) -> f32 {
        ((self.swell - 0.4) / 0.4).clamp(0.0, 1.0)
    }

    fn accent(&mut self, amount: f32, white: bool) {
        for lamp in &mut self.lamps {
            if lamp.part == Part::Accent {
                lamp.hit(amount, white);
            }
        }
    }

    /// What each light shows now.
    fn light(&self, settings: &ShowSettings) -> Vec<BulbOutput> {
        let (back, hit, alt) = self.roles();
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
                    // From near dark, so every beat lands.
                    Part::Punch => (
                        0.04 + 0.16 * swell,
                        if pattern.second(lamp.rank, self.punches) {
                            alt
                        } else {
                            hit
                        },
                    ),
                    Part::Wash => (
                        0.15 + 0.55 * self.bass.clamp(0.0, 1.0).powf(1.5) + 0.15 * swell,
                        rest,
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
                let level = base + (1.0 - base) * lamp.flash;
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
                    } else {
                        mix(rest, strike, lamp.flash)
                    };
                    let rgb = pale(rgb, 0.6 * climb);
                    output(rgb, level * bright * even(rgb))
                }
            })
            .collect()
    }
}

/// `rgb` mixed `t` of the way to white.
fn pale(rgb: Rgb, t: f32) -> Rgb {
    let mix = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * t).round() as u8;
    Rgb::new(mix(rgb.r), mix(rgb.g), mix(rgb.b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::full_chroma;
    use crate::safety::FlashGuard;

    fn tone(rate: u32, freq: f32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    /// White-ish noise from a fixed seed: a snare or a hat once filtered.
    fn hiss(amp: f32, n: usize, seed: u32) -> Vec<f32> {
        let mut x = seed.max(1);
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                amp * ((x >> 8) as f32 / (1u32 << 23) as f32 - 1.0)
            })
            .collect()
    }

    /// Four to the floor over a quiet pad, `amp` loud, at `bpm`, one frame
    /// (25 ms) at a time. Each frame's levels.
    fn four_on_the_floor(
        a: &mut Analyzer,
        rate: u32,
        bpm: f32,
        amp: f32,
        secs: f32,
    ) -> Vec<Levels> {
        let frame = (rate as f32 * 0.025) as usize;
        let beat = 60.0 / bpm;
        let total = (secs * rate as f32) as usize;
        let mut out = Vec::new();
        let mut chunk = Vec::with_capacity(frame);
        for i in 0..total {
            let t = i as f32 / rate as f32;
            let into = t % beat;
            let kick = if into < 0.08 { 0.7 } else { 0.03 };
            chunk.push(amp * kick * (TAU * 55.0 * t).sin());
            if chunk.len() == frame {
                out.push(a.feed(&chunk));
                chunk.clear();
            }
        }
        out
    }

    #[test]
    fn a_kick_after_quiet_is_a_beat() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let quiet = tone(rate, 60.0, 0.02, 0.04);
        for _ in 0..50 {
            assert!(a.feed(&quiet).beat.is_none());
        }
        let l = a.feed(&tone(rate, 60.0, 0.8, 0.04));
        assert!(l.kick);
        assert!(l.beat.unwrap() > 0.9, "{l:?}");
        assert!(l.energy > 0.9);
    }

    #[test]
    fn a_steady_tone_is_not_a_beat() {
        let rate = 44_100;
        let mut a = Analyzer::new(rate);
        let hum = tone(rate, 80.0, 0.5, 0.04);
        let beats = (0..200).filter(|_| a.feed(&hum).beat.is_some()).count();
        assert!(beats <= 1, "{beats} beats in a hum");
    }

    #[test]
    fn silence_is_calm() {
        let mut a = Analyzer::new(44_100);
        let l = a.feed(&vec![0.0; 1764]);
        assert_eq!(l, Levels::default());
        let mut s = Stage::new(&Fixture::row(1));
        let out = s.frame(&ShowSettings::default(), l, 0.025);
        assert!(out[0].bright() < 20);
    }

    #[test]
    fn the_tempo_is_found_and_kept() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let frames = four_on_the_floor(&mut a, rate, 128.0, 1.0, 12.0);
        let tempo = frames.last().unwrap().tempo.expect("no tempo found");
        assert!((tempo - 60.0 / 128.0).abs() < 0.01, "{tempo}");
        // Once locked, one beat per kick: none doubled, none dropped.
        let beats = frames[frames.len() / 2..]
            .iter()
            .filter(|l| l.beat.is_some())
            .count();
        let expected = (6.0 * 128.0 / 60.0) as usize;
        assert!(
            beats.abs_diff(expected) <= 1,
            "{beats} beats for {expected}"
        );
    }

    #[test]
    fn the_next_track_sets_the_clock_again() {
        // Straight from one track into the next, two thirds as fast: every
        // other kick of the new one falls on a beat of the old, which keeps
        // the old clock sure. Its beats must not play on over the new track
        // for long.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        four_on_the_floor(&mut a, rate, 120.0, 1.0, 12.0);
        let frames = four_on_the_floor(&mut a, rate, 80.0, 1.0, 12.0);
        let tempo = |l: &Levels| l.tempo.map(|p| 60.0 / p);
        let settled = frames[(8.0 / 0.025) as usize..]
            .iter()
            .all(|l| tempo(l).is_some_and(|bpm| (bpm - 80.0).abs() < 2.0));
        assert!(settled, "{:?}", frames.last().and_then(tempo));
    }

    #[test]
    fn a_pause_between_tracks_forgets_the_beat() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        four_on_the_floor(&mut a, rate, 128.0, 1.0, 12.0);
        let silence = vec![0.0; (rate as f32 * 0.025) as usize];
        for _ in 0..(1.6 / 0.025) as usize {
            a.feed(&silence);
        }
        assert!(a.tempo.hits.is_empty());
        let frames = four_on_the_floor(&mut a, rate, 90.0, 1.0, 8.0);
        let tempo = frames.last().unwrap().tempo.expect("no tempo found");
        assert!((tempo - 60.0 / 90.0).abs() < 0.02, "{}", 60.0 / tempo);
    }

    #[test]
    fn a_drop_gives_way_after_a_phrase() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        settle(&mut stage, 0.8);
        stage.drop(&s, &beat(1.0));
        // A phrase found late keeps setting the count back.
        for _ in 0..PHRASE_BEATS + 1 {
            stage.beat = stage.beat.min(4);
            stage.frame(&s, beat(1.0), 0.025);
            for _ in 0..19 {
                stage.frame(&s, between(), 0.025);
            }
        }
        assert_eq!(stage.section, Section::Groove);
    }

    #[test]
    fn the_clock_moving_back_keeps_the_tempo() {
        // The clock rewinds a few seconds in, while the tempo locks.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        a.time = 2.0 * CLOCK_SPAN - 5.0;
        let frames = four_on_the_floor(&mut a, rate, 128.0, 1.0, 12.0);
        assert!(a.time < CLOCK_SPAN + 10.0, "{}", a.time);
        let tempo = frames.last().unwrap().tempo.expect("tempo lost");
        assert!((tempo - 60.0 / 128.0).abs() < 0.01, "{tempo}");
        let beats = frames[frames.len() / 2..]
            .iter()
            .filter(|l| l.beat.is_some())
            .count();
        let expected = (6.0 * 128.0 / 60.0) as usize;
        assert!(
            beats.abs_diff(expected) <= 1,
            "{beats} beats for {expected}"
        );
    }

    #[test]
    fn a_locked_beat_goes_out_early() {
        // Once the clock is sure, the beat leaves before the kick is heard.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let frames = four_on_the_floor(&mut a, rate, 120.0, 1.0, 12.0);
        let early = frames[frames.len() / 2..]
            .iter()
            .filter(|l| l.beat.is_some() && !l.kick)
            .count();
        assert!(early >= 8, "only {early} beats played ahead of their kick");
    }

    #[test]
    fn the_volume_knob_does_not_change_the_show() {
        let rate = 48_000;
        let level = |amp: f32| {
            let mut a = Analyzer::new(rate);
            let frames = four_on_the_floor(&mut a, rate, 120.0, amp, 8.0);
            let tail = &frames[frames.len() - 80..];
            tail.iter().map(|l| l.energy).sum::<f32>() / tail.len() as f32
        };
        let (loud, soft) = (level(1.0), level(0.05));
        assert!((loud - soft).abs() < 0.05, "{loud} vs {soft}");
        assert!(loud > 0.5, "{loud}");
    }

    #[test]
    fn turning_the_volume_down_is_soon_forgotten() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let before = four_on_the_floor(&mut a, rate, 120.0, 1.0, 10.0);
        let after = four_on_the_floor(&mut a, rate, 120.0, 0.1, 8.0);
        let mean = |f: &[Levels]| f.iter().map(|l| l.energy).sum::<f32>() / f.len() as f32;
        let (was, now) = (
            mean(&before[before.len() - 80..]),
            mean(&after[after.len() - 80..]),
        );
        assert!(now > was - 0.08, "{was} -> {now}");
        // And the beat carried on throughout.
        assert!(after[after.len() / 2..].iter().any(|l| l.beat.is_some()));
    }

    #[test]
    fn a_breakdown_stays_quieter_than_the_groove() {
        // The kick drops out and only the hi-hats carry on over a soft pad,
        // far quieter: that is the music, not the volume knob, and the show
        // must still read it as quieter long into the breakdown.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let groove = four_on_the_floor(&mut a, rate, 120.0, 1.0, 10.0);
        let frame = (rate as f32 * 0.025) as usize;
        let mut breakdown = Vec::new();
        for k in 0..(16.0 / 0.025) as usize {
            let mut chunk: Vec<f32> = (0..frame)
                .map(|i| {
                    let t = (k * frame + i) as f32 / rate as f32;
                    0.02 * (TAU * 55.0 * t).sin()
                })
                .collect();
            if k % 10 == 0 {
                let hat = hiss(0.08, frame / 2, k as u32 + 1);
                for (s, h) in chunk.iter_mut().zip(hat) {
                    *s += h;
                }
            }
            breakdown.push(a.feed(&chunk));
        }
        assert!(breakdown.iter().any(|l| l.hat.is_some()), "no hats heard");
        let mean = |f: &[Levels]| f.iter().map(|l| l.energy).sum::<f32>() / f.len() as f32;
        let (loud, quiet) = (
            mean(&groove[groove.len() - 80..]),
            mean(&breakdown[breakdown.len() - 80..]),
        );
        assert!(
            quiet < loud - 0.15,
            "the breakdown reads {quiet}, the groove {loud}"
        );
    }

    #[test]
    fn the_bands_hear_their_own_instruments() {
        // A bass line and a hum, a hi-hat every fourth frame and a snare
        // every sixteenth, one 25 ms frame at a time. No kick.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let mut snare_band = Biquad::new(Pass::Band, rate as f32, 2200.0, 0.7);
        let mut hat_band = Biquad::new(Pass::High, rate as f32, 9000.0, 0.7);
        let (mut kicks, mut snares, mut hats) = (0, 0, 0);
        for frame in 0..96u32 {
            // Whole cycles in a frame, so the frames join without a click.
            let bass = tone(rate, 40.0, 0.2, 0.025);
            let hum = tone(rate, 400.0, 0.01, 0.025);
            let mut chunk: Vec<f32> = bass.iter().zip(&hum).map(|(a, b)| a + b).collect();
            let n = chunk.len();
            if frame % 16 == 8 {
                for (c, s) in chunk.iter_mut().zip(hiss(0.6, n, frame + 1)) {
                    *c += snare_band.run(s);
                }
            } else if frame % 4 == 0 {
                for (c, s) in chunk.iter_mut().zip(hiss(0.4, n, frame + 7)) {
                    *c += hat_band.run(s);
                }
            }
            let l = a.feed(&chunk);
            kicks += usize::from(l.kick);
            snares += usize::from(l.snare.is_some());
            hats += usize::from(l.hat.is_some());
        }
        assert_eq!(kicks, 0, "a kick in the treble");
        assert!((4..=6).contains(&snares), "{snares} snares for 5");
        assert!(hats >= 14, "{hats} hats for 19");
    }

    fn beat(strength: f32) -> Levels {
        Levels {
            energy: 0.8,
            bass: 0.8,
            beat: Some(strength),
            kick: true,
            ..Levels::default()
        }
    }

    fn between() -> Levels {
        Levels {
            energy: 0.8,
            bass: 0.6,
            ..Levels::default()
        }
    }

    fn rgb(o: BulbOutput) -> Rgb {
        match o {
            BulbOutput::Color { rgb, .. } => rgb,
            other => panic!("expected a colour, got {other:?}"),
        }
    }

    /// Play a groove: a beat every 20 frames (half a second).
    fn groove(stage: &mut Stage, s: &ShowSettings, beats: usize) -> Vec<Vec<BulbOutput>> {
        let mut frames = Vec::new();
        for k in 0..beats * 20 {
            let l = if k % 20 == 0 { beat(1.0) } else { between() };
            frames.push(stage.frame(s, l, 0.025));
        }
        frames
    }

    #[test]
    fn parts_follow_the_room() {
        let q = Duration::from_millis(0);
        let near = |x: f32, height: f32| Fixture {
            x,
            depth: 0.0,
            height,
            interval: q,
            latency: q,
        };
        // Three alike: all three on the beat, so the patterns can travel.
        let three = [near(-1.0, 0.5), near(0.0, 0.5), near(1.0, 0.5)];
        assert_eq!(cast(&three), vec![Part::Punch; 3]);
        // One at the ceiling takes the snare.
        let up = [near(-1.0, 0.5), near(0.0, 0.9), near(1.0, 0.5)];
        assert_eq!(cast(&up), vec![Part::Punch, Part::Accent, Part::Punch]);
        // A slow bulb, and one across the room, wash.
        let hue = Fixture {
            interval: Duration::from_millis(200),
            ..near(0.0, 0.5)
        };
        let far = Fixture {
            depth: 4.0,
            ..near(0.0, 0.5)
        };
        assert_eq!(
            cast(&[near(-1.0, 0.5), hue, far]),
            vec![Part::Punch, Part::Wash, Part::Wash]
        );
        // Someone always has the beat.
        assert_eq!(cast(&[hue, far]), vec![Part::Wash, Part::Punch]);
        assert_eq!(cast(&[hue]), vec![Part::Punch]);
    }

    #[test]
    fn the_patterns_move_across_the_room() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        let frames = groove(&mut stage, &s, 48);
        // Somewhere along the way, one light was hit alone: a chase.
        let alone = frames.iter().any(|f| {
            let lit = f.iter().filter(|o| o.bright() > 55).count();
            lit == 1
        });
        assert!(alone, "never a light on its own");
        // And somewhere, all three at once.
        assert!(frames.iter().any(|f| f.iter().all(|o| o.bright() > 55)));
    }

    #[test]
    fn a_chase_walks_left_to_right() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        stage.phrases = PHRASES_PER_LOOK; // Chase.
        assert_eq!(stage.pattern(), Pattern::Chase);
        let mut order = Vec::new();
        for _ in 0..3 {
            let f = stage.frame(&s, beat(1.0), 0.025);
            let top = (0..3).max_by_key(|&i| f[i].bright()).unwrap();
            order.push(top);
            for _ in 0..19 {
                stage.frame(&s, between(), 0.025);
            }
        }
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn the_colours_hold_through_a_phrase() {
        // On every beat the punches strike in the phrase's hit colour, from
        // the harmony playing; it holds for the whole phrase and moves on
        // as the next one lands. Over a few minutes, every colour of every
        // harmony strikes.
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        let beats = PHRASE_BEATS * PHRASES_PER_HARMONY * PARTY.len() as u32;
        let mut seen = Vec::new();
        let mut last: Option<(u32, Rgb)> = None;
        for k in 0..beats as usize * 20 {
            let l = if k % 20 == 0 { beat(1.0) } else { between() };
            let frame = stage.frame(&s, l, 0.025);
            if k % 20 != 0 {
                continue;
            }
            let harmony = stage.harmony().map(full_chroma);
            for o in frame.iter().filter(|o| o.bright() > 55) {
                let c = rgb(*o);
                assert!(harmony.contains(&c), "{c:?} is not in {harmony:?}");
                seen.push(c);
            }
            if stage.pattern() != Pattern::Together {
                continue;
            }
            let c = rgb(frame[0]);
            assert!(frame.iter().all(|o| rgb(*o) == c), "the punches disagree");
            match last {
                Some((p, before)) if p == stage.phrases => assert_eq!(c, before),
                Some((_, before)) => assert_ne!(c, before, "a new phrase, the same colour"),
                None => {}
            }
            last = Some((stage.phrases, c));
        }
        assert!(
            PARTY
                .iter()
                .flatten()
                .all(|c| seen.contains(&full_chroma(*c)))
        );
    }

    #[test]
    fn a_hit_fades_back_into_the_rooms_colour() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(1));
        stage.enter(Section::Groove);
        let struck = rgb(stage.frame(&s, beat(1.0), 0.025)[0]);
        let mut rest = struck;
        for _ in 0..19 {
            rest = rgb(stage.frame(&s, between(), 0.025)[0]);
        }
        let (back, hit, _) = stage.roles();
        assert_eq!(struck, full_chroma(hit));
        assert_ne!(rest, struck);
        // Nearly home by the next beat.
        let (b, r) = (full_chroma(back), rest);
        let off =
            b.r.abs_diff(r.r)
                .max(b.g.abs_diff(r.g))
                .max(b.b.abs_diff(r.b));
        assert!(off < 40, "{r:?} is far from {b:?}");
    }

    #[test]
    fn a_hit_lasts_as_long_as_the_beat() {
        // A slow track's hits fade more slowly than a fast one's, and each
        // is nearly out before its next beat.
        let s = ShowSettings::default();
        let level_after = |period: f32, secs: f32| {
            let mut stage = Stage::new(&Fixture::row(1));
            stage.enter(Section::Groove);
            let quiet = Levels {
                tempo: Some(period),
                ..Levels::default()
            };
            stage.frame(&s, quiet, 0.025);
            stage.frame(
                &s,
                Levels {
                    beat: Some(1.0),
                    kick: true,
                    ..quiet
                },
                0.025,
            );
            let mut last = 0;
            for _ in 0..(secs / 0.025) as usize {
                last = stage.frame(&s, quiet, 0.025)[0].bright();
            }
            last
        };
        assert!(level_after(0.8, 0.3) > level_after(0.4, 0.3));
        assert!(level_after(0.4, 0.4) < 15);
    }

    #[test]
    fn the_rings_move_from_the_middle_out_and_back() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(5));
        // Five alike at the same height: one takes the snare, four punch.
        let punches: Vec<usize> = (0..5)
            .filter(|&i| stage.lamps[i].part == Part::Punch)
            .collect();
        assert_eq!(punches.len(), 4);
        stage.enter(Section::Groove);
        let all = Pattern::for_lights(4);
        let at = |p: Pattern| all.iter().position(|&q| q == p).unwrap() as u32;
        for (pattern, first) in [(Pattern::CentreOut, [1, 2]), (Pattern::OutsideIn, [0, 3])] {
            stage.phrases = at(pattern) * PHRASES_PER_LOOK;
            stage.beat = 0;
            assert_eq!(stage.pattern(), pattern);
            // The first beat of the phrase is beat 1: the second ring, then
            // back to the first.
            let mut lit = Vec::new();
            for _ in 0..2 {
                let f = stage.frame(&s, beat(1.0), 0.025);
                let on: Vec<usize> = (0..punches.len())
                    .filter(|&r| f[punches[r]].bright() > 55)
                    .collect();
                lit.push(on);
                for _ in 0..19 {
                    stage.frame(&s, between(), 0.025);
                }
            }
            let other: Vec<usize> = (0..4).filter(|r| !first.contains(r)).collect();
            assert_eq!(lit[0], other, "{pattern:?}");
            assert_eq!(lit[1], first.to_vec(), "{pattern:?}");
        }
        assert_eq!(ring(0, 3), (1, 2));
        assert_eq!(ring(1, 3), (0, 2));
        assert_eq!(ring(2, 4), (0, 2));
    }

    #[test]
    fn a_build_quickens_as_it_climbs() {
        let s = ShowSettings {
            flashes: false,
            ..ShowSettings::default()
        };
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Build);
        let steps = |stage: &mut Stage, secs: f32| {
            let before = stage.roll;
            for _ in 0..(secs / 0.025) as usize {
                let l = Levels {
                    energy: 0.6,
                    ..Levels::default()
                };
                stage.frame(&s, l, 0.025);
                assert_eq!(stage.section, Section::Build);
            }
            stage.roll - before
        };
        let early = steps(&mut stage, 2.0);
        stage.held = 6.0;
        let late = steps(&mut stage, 2.0);
        assert!((3..=5).contains(&early), "{early} steps early on");
        assert!(late >= 3 * early, "{early} then {late}");
    }

    #[test]
    fn loud_music_still_pulses() {
        // Compressed music never leaves the top of its range; the beat must
        // still stand out against it.
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(1));
        stage.enter(Section::Groove);
        let loud = Levels {
            energy: 1.0,
            bass: 1.0,
            ..Levels::default()
        };
        let mut between = 0;
        for _ in 0..40 {
            between = stage.frame(&s, loud, 0.025)[0].bright();
        }
        let hit = Levels {
            beat: Some(1.0),
            kick: true,
            ..loud
        };
        let flash = stage.frame(&s, hit, 0.025)[0].bright();
        assert!(flash >= s.bright - 5 && flash <= s.bright, "{flash}");
        assert!(flash - between >= 20, "{between} -> {flash}");
        assert!(stage.sharp()[0]);
    }

    #[test]
    fn a_breakdown_dims_and_a_drop_lands_white() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(2));
        stage.enter(Section::Groove);
        let loud = groove(&mut stage, &s, 8);
        let peak = loud.iter().map(|f| f[0].bright()).max().unwrap();

        // The kick goes: a breakdown.
        let calm = Levels {
            energy: 0.5,
            bass: 0.3,
            ..Levels::default()
        };
        let mut dim = 100;
        for _ in 0..160 {
            dim = stage.frame(&s, calm, 0.025)[0].bright();
        }
        assert_eq!(stage.section, Section::Calm);
        assert!(dim < peak / 2, "{dim} in a breakdown, {peak} on the beat");

        // A snare roll that crowds in and climbs: a build.
        for k in 0..240 {
            let l = Levels {
                energy: 0.5 + 0.4 * k as f32 / 240.0,
                snare: (k % 3 == 0).then_some(0.8),
                ..Levels::default()
            };
            stage.frame(&s, l, 0.025);
        }
        assert_eq!(stage.section, Section::Build);

        // The breath before the drop: dark.
        let hush = Levels {
            energy: 0.05,
            ..Levels::default()
        };
        let held = (0..8)
            .map(|_| stage.frame(&s, hush, 0.025)[0])
            .last()
            .unwrap();
        assert_eq!(stage.section, Section::Hold);
        assert!(held.bright() < 10, "{held:?}");

        // The kick lands: white, everyone, sharp.
        let out = stage.frame(&s, beat(1.0), 0.025);
        assert_eq!(stage.section, Section::Drop);
        for (o, sharp) in out.iter().zip(stage.sharp()) {
            assert!(matches!(o, BulbOutput::White { .. }), "{o:?}");
            assert!(o.bright() >= 75);
            assert!(sharp);
        }
    }

    #[test]
    fn no_white_flashes_when_turned_off() {
        let s = ShowSettings {
            flashes: false,
            ..ShowSettings::default()
        };
        let mut stage = Stage::new(&Fixture::row(2));
        stage.enter(Section::Hold);
        let out = stage.frame(&s, beat(1.0), 0.025);
        assert_eq!(stage.section, Section::Drop);
        assert!(out.iter().all(|o| matches!(o, BulbOutput::Color { .. })));
    }

    /// Whether a light shows white, or a colour pale enough to pass for it.
    fn whitish(o: &BulbOutput) -> bool {
        match o {
            BulbOutput::White { bright, .. } => *bright >= 20,
            BulbOutput::Color { rgb, bright } => {
                *bright >= 20 && crate::color::saturation(*rgb) < 0.2
            }
        }
    }

    /// The most white flashes the room shows in any one second.
    ///
    /// Each light's white (its brightness while it shows white, else
    /// nothing) turns when it swings a tenth of the way back, the darker
    /// side under 0.8. Lights turning the same way within 0.1 s are one
    /// change of the room; a flash is a rise and a fall.
    fn room_white_flashes(frames: &[Vec<BulbOutput>], dt: f32) -> usize {
        let mut events: Vec<(f32, i8)> = Vec::new();
        for i in 0..frames[0].len() {
            let level = |f: &Vec<BulbOutput>| {
                if whitish(&f[i]) {
                    f32::from(f[i].bright()) / 100.0
                } else {
                    0.0
                }
            };
            let (mut ext, mut dir) = (level(&frames[0]), 0i8);
            for (k, f) in frames.iter().enumerate().skip(1) {
                let v = level(f);
                if v - ext >= 0.1 && dir <= 0 && ext < 0.8 {
                    (ext, dir) = (v, 1);
                    events.push((k as f32 * dt, 1));
                } else if ext - v >= 0.1 && dir >= 0 && v < 0.8 {
                    (ext, dir) = (v, -1);
                    events.push((k as f32 * dt, -1));
                } else if (dir > 0 && v > ext) || (dir < 0 && v < ext) {
                    ext = v;
                }
            }
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut room: Vec<(f32, i8)> = Vec::new();
        for (t, d) in events {
            if !room.iter().any(|&(u, e)| e == d && t - u < 0.1 - 1e-4) {
                room.push((t, d));
            }
        }
        room.iter()
            .map(|&(t, _)| {
                let within = |d: i8| {
                    room.iter()
                        .filter(|&&(u, e)| e == d && u <= t && t - u < 1.0)
                        .count()
                };
                within(1).max(within(-1))
            })
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn white_flashes_stay_under_three_a_second() {
        // The top of a build: every snare walks a white flash to the next
        // light, eight a second. Each light flashes only twice a second; the
        // room flashes eight times. With the protection off as on, the
        // show's guard holds the room to three.
        let s = ShowSettings::default();
        let dt = 0.025;
        let mut stage = Stage::new(&Fixture::row(4));
        stage.enter(Section::Build);
        stage.held = 7.0;
        let frames: Vec<Vec<BulbOutput>> = (0..160)
            .map(|k| {
                let l = Levels {
                    energy: 0.8,
                    snare: (k % 5 == 0).then_some(1.0),
                    ..Levels::default()
                };
                stage.frame(&s, l, dt)
            })
            .collect();
        assert_eq!(stage.section, Section::Build);
        let raw = room_white_flashes(&frames, dt);
        assert!(raw > 3, "the roll only flashes {raw} times a second");
        for on in [false, true] {
            let mut guard = FlashGuard::show(4);
            let sent: Vec<Vec<BulbOutput>> = frames
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    guard.apply(&mut f, dt, on);
                    f
                })
                .collect();
            let n = room_white_flashes(&sent, dt);
            assert!(n <= 3, "{n} white flashes a second, protection on: {on}");
        }
    }

    #[test]
    fn a_slow_bulb_washes_without_flicker() {
        // A Hue next to two Yeelights: the Yeelights punch, the Hue glides.
        let mut fixtures = Fixture::row(3);
        fixtures[1].interval = Duration::from_millis(200);
        let s = ShowSettings::default();
        let mut stage = Stage::new(&fixtures);
        stage.enter(Section::Groove);
        let frames = groove(&mut stage, &s, 16);
        for w in frames.windows(2) {
            let jump = w[0][1].bright().abs_diff(w[1][1].bright());
            assert!(jump <= 20, "the wash jumped by {jump}");
        }
        let swing = |i: usize| {
            let b: Vec<u8> = frames.iter().map(|f| f[i].bright()).collect();
            b.iter().max().unwrap() - b.iter().min().unwrap()
        };
        assert!(swing(0) > 40, "the punch barely moves");
    }

    /// A stage that has heard the music at `energy` for a good while.
    fn settle(stage: &mut Stage, energy: f32) {
        (stage.short, stage.recent, stage.long) = (energy, energy, energy);
    }

    /// A band without drums at 100 beats a minute, one 25 ms frame at a
    /// time: a strum on the one and the three, a snare (a rim, a clap) on
    /// the two and the four. Each frame's levels.
    fn unplugged(a: &mut Analyzer, rate: u32, secs: f32) -> Vec<Levels> {
        let beat = 0.6;
        let frame = (rate as f32 * 0.025) as usize;
        let mut snare_band = Biquad::new(Pass::Band, rate as f32, 2200.0, 0.7);
        let noise = hiss(0.5, (secs * rate as f32) as usize, 3);
        let mut out = Vec::new();
        let mut chunk = Vec::with_capacity(frame);
        for (i, &hush) in noise.iter().enumerate() {
            let t = i as f32 / rate as f32;
            let n = (t / beat) as u32;
            let into = t - n as f32 * beat;
            let ring = (-into / 0.15).exp();
            let s = if n.is_multiple_of(2) {
                // A chord of E, well above the kick's band and under the
                // snare's.
                let chord: f32 = [330.0, 415.0, 494.0, 659.0]
                    .iter()
                    .map(|f| (TAU * f * t).sin())
                    .sum();
                0.15 * ring * chord
            } else {
                ring * snare_band.run(hush)
            };
            // A bass drone holds the bottom steady, as an organ or a
            // synth would.
            chunk.push(s + 0.1 * (TAU * 55.0 * t).sin());
            if chunk.len() == frame {
                out.push(a.feed(&chunk));
                chunk.clear();
            }
        }
        out
    }

    #[test]
    fn a_track_without_a_kick_still_finds_its_tempo() {
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let frames = unplugged(&mut a, rate, 15.0);
        assert!(
            frames.iter().all(|l| !l.kick),
            "a kick in a band without drums"
        );
        let tempo = frames.last().unwrap().tempo.expect("no tempo found");
        assert!((tempo - 0.6).abs() < 0.01, "{tempo}");
        // Its beats come from the clock, one a beat.
        let beats = frames[frames.len() / 2..]
            .iter()
            .filter(|l| l.beat.is_some())
            .count();
        let expected = (7.5 / 0.6) as usize;
        assert!(
            beats.abs_diff(expected) <= 1,
            "{beats} beats for {expected}"
        );
    }

    #[test]
    fn the_clock_reads_on_between_packets_of_sound() {
        // The sound arrives 40 ms at a time and the show looks every 10 ms:
        // a beat due between two packets goes out then, not with the next
        // packet, and none is played twice.
        let rate = 48_000;
        let mut a = Analyzer::new(rate);
        let packet = (rate as f32 * 0.04) as usize;
        let beat = 0.5;
        let total = (12.0 * rate as f32) as usize;
        let (mut between, mut beats) = (0, 0);
        let mut chunk = Vec::with_capacity(packet);
        for i in 0..total {
            let t = i as f32 / rate as f32;
            let kick = if t % beat < 0.08 { 0.7 } else { 0.03 };
            chunk.push(kick * (TAU * 55.0 * t).sin());
            if chunk.len() < packet {
                continue;
            }
            let late = t > 6.0;
            let fired = a.feed_after(&chunk, 0.01).beat.is_some();
            beats += usize::from(late && fired);
            chunk.clear();
            for _ in 0..3 {
                let fired = a.feed_after(&[], 0.01).beat.is_some();
                between += usize::from(late && fired);
                beats += usize::from(late && fired);
            }
        }
        assert!(between >= 4, "only {between} beats between packets");
        let expected = (6.0 / beat) as usize;
        assert!(
            beats.abs_diff(expected) <= 1,
            "{beats} beats for {expected}"
        );
    }

    #[test]
    fn a_quick_light_waits_for_the_slow_one() {
        // Two lights struck together: the one that shows a command in no
        // time waits a tenth of a second for the other, and is sent sharp
        // when it does.
        let mut fixtures = Fixture::row(2);
        fixtures[0].latency = Duration::ZERO;
        fixtures[1].latency = Duration::from_millis(100);
        let s = ShowSettings::default();
        let mut stage = Stage::new(&fixtures);
        stage.enter(Section::Groove);
        for _ in 0..4 {
            stage.frame(&s, between(), 0.025);
        }
        let struck = stage.frame(&s, beat(1.0), 0.025);
        assert!((stage.lead() - 0.1).abs() < 1e-6);
        assert!(struck[1].bright() > 55 && stage.sharp()[1]);
        assert!(struck[0].bright() < 30 && !stage.sharp()[0]);
        assert!(stage.due(0).is_some_and(|d| d > 0.05 && d <= 0.1));
        let mut shown_at = None;
        for k in 1..=6 {
            let f = stage.frame(&s, between(), 0.025);
            if f[0].bright() > 55 && shown_at.is_none() {
                shown_at = Some(k);
                assert!(stage.sharp()[0]);
            }
        }
        assert_eq!(shown_at, Some(4));
    }

    #[test]
    fn the_phrase_is_set_right_where_the_sound_turns() {
        // The count starts wherever the clock found the beat; the track's
        // phrase starts at the 22nd beat heard, where the hats and the
        // vocals come in. A few beats on, the count runs from there.
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        let verse = Levels {
            energy: 0.6,
            bass: 0.6,
            mid: 0.2,
            treble: 0.1,
            ..Levels::default()
        };
        let chorus = Levels {
            energy: 0.9,
            bass: 0.9,
            mid: 0.6,
            treble: 0.7,
            ..Levels::default()
        };
        for j in 1..=60u32 {
            let sound = if j < 22 { verse } else { chorus };
            let struck = Levels {
                beat: Some(1.0),
                kick: true,
                ..sound
            };
            stage.frame(&s, struck, 0.025);
            if j >= 28 {
                assert_eq!(stage.beat, (j - 22) % PHRASE_BEATS, "beat {j}");
            }
            for _ in 0..19 {
                stage.frame(&s, sound, 0.025);
            }
        }
        // And a steady groove is never set anywhere.
        let mut steady = Stage::new(&Fixture::row(3));
        steady.enter(Section::Groove);
        for j in 1..=60u32 {
            steady.frame(&s, beat(1.0), 0.025);
            assert_eq!(steady.beat, j % PHRASE_BEATS);
            for _ in 0..19 {
                steady.frame(&s, between(), 0.025);
            }
        }
    }

    #[test]
    fn a_crash_on_a_jump_starts_a_phrase() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        // The verse sat at 0.6; the band jumps up as the cymbal crashes.
        let verse = Levels {
            energy: 0.6,
            ..between()
        };
        settle(&mut stage, verse.energy);
        for k in 0..7 * 20 {
            let l = if k % 20 == 0 {
                Levels {
                    beat: Some(1.0),
                    kick: true,
                    ..verse
                }
            } else {
                verse
            };
            stage.frame(&s, l, 0.025);
        }
        assert_eq!(stage.beat, 7);
        let phrases = stage.phrases;
        let crash = Levels {
            energy: 1.0,
            bass: 1.0,
            beat: Some(1.0),
            kick: true,
            hat: Some(1.0),
            ..Levels::default()
        };
        stage.frame(&s, crash, 0.025);
        assert_eq!(stage.phrases, phrases + 1);
        assert_eq!(stage.beat, 0);
    }

    #[test]
    fn the_kick_coming_back_starts_a_phrase() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        settle(&mut stage, 0.6);
        groove(&mut stage, &s, 5);
        let calm = Levels {
            energy: 0.5,
            ..Levels::default()
        };
        for _ in 0..260 {
            stage.frame(&s, calm, 0.025);
        }
        assert_eq!(stage.section, Section::Calm);
        let phrases = stage.phrases;
        stage.frame(&s, beat(1.0), 0.025);
        assert_eq!(stage.section, Section::Groove);
        assert_eq!((stage.phrases, stage.beat), (phrases + 1, 0));
    }

    #[test]
    fn a_kick_or_two_missed_is_not_a_breakdown() {
        // The bass hides a few kicks: the groove carries on through them,
        // and losing the beat a little longer picks it up in the same
        // colours.
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        settle(&mut stage, 0.8);
        groove(&mut stage, &s, 5);
        let between = Levels {
            energy: 0.8,
            ..Levels::default()
        };
        for _ in 0..60 {
            stage.frame(&s, between, 0.025);
        }
        assert_eq!(stage.section, Section::Groove);
        for _ in 0..60 {
            stage.frame(&s, between, 0.025);
        }
        assert_eq!(stage.section, Section::Calm);
        let (phrases, beat_was) = (stage.phrases, stage.beat);
        stage.frame(&s, beat(1.0), 0.025);
        assert_eq!(stage.section, Section::Groove);
        assert_eq!(stage.phrases, phrases);
        assert_eq!(stage.beat, beat_was + 1);
    }

    #[test]
    fn a_sure_clock_carries_the_groove_without_a_kick() {
        // The clock is sure and the music holds its level: its beats keep
        // the groove going when the kick cannot be heard.
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        settle(&mut stage, 0.8);
        groove(&mut stage, &s, 5);
        for k in 0..(10.0 / 0.025) as usize {
            let l = Levels {
                energy: 0.8,
                beat: (k % 20 == 0).then_some(0.8),
                tempo: Some(0.5),
                ..Levels::default()
            };
            stage.frame(&s, l, 0.025);
            assert_eq!(stage.section, Section::Groove, "at frame {k}");
        }
        // The level falls away: a breakdown, clock or not.
        for k in 0..(6.0 / 0.025) as usize {
            let l = Levels {
                energy: 0.3,
                beat: (k % 20 == 0).then_some(0.8),
                tempo: Some(0.5),
                ..Levels::default()
            };
            stage.frame(&s, l, 0.025);
        }
        assert_eq!(stage.section, Section::Calm);
    }

    #[test]
    fn a_track_without_a_kick_grooves_on_the_clock() {
        // Beats from the clock alone, no kick ever, the music carrying on
        // at its level: a groove from the first.
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(3));
        settle(&mut stage, 0.7);
        let mut peak = 0;
        for k in 0..(24.0 / 0.025) as usize {
            let l = Levels {
                energy: 0.7,
                bass: 0.5,
                beat: (k % 24 == 0).then_some(0.8),
                tempo: Some(0.6),
                ..Levels::default()
            };
            let f = stage.frame(&s, l, 0.025);
            let t = k as f32 * 0.025;
            if t > 1.0 {
                assert_eq!(stage.section, Section::Groove, "at {t}");
            }
            if t > 20.0 {
                peak = peak.max(f.iter().map(|o| o.bright()).max().unwrap());
            }
        }
        assert_eq!(stage.section, Section::Groove);
        assert!(peak > 55, "the beats never punched: {peak}");
    }

    #[test]
    fn zones_spread_along_the_wall() {
        let f = Fixture::row(1)[0];
        let zones = f.zones(3);
        let xs: Vec<f32> = zones.iter().map(|z| z.x).collect();
        assert_eq!(xs, vec![-ZONE_SPAN, 0.0, ZONE_SPAN]);
        assert_eq!(f.zones(1), vec![f]);
    }

    #[test]
    fn without_beats_the_colours_drift() {
        let s = ShowSettings::default();
        let mut stage = Stage::new(&Fixture::row(1));
        let first = rgb(stage.frame(&s, Levels::default(), 0.025)[0]);
        let mut last = first;
        for _ in 0..400 {
            last = rgb(stage.frame(&s, Levels::default(), 0.025)[0]);
        }
        assert_ne!(first, last);
    }
}
