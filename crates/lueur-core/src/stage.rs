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
//! colour, a light at the ceiling answers the snare. And the show follows the
//! shape of the track: dim and slow in a breakdown, rising and whitening
//! through a build, dark for the breath before the drop, everything at once
//! when it lands.
//!
//! The music never picks the colours. They glide through the colour show's
//! combinations at the chosen speed, or hold the colours picked (see
//! [`MusicColors`]); the music plays the brightness on top of them, passes
//! the beat from light to light, and flashes white.

use crate::color::{BulbOutput, Rgb};
use crate::geometry::{Light, Screen};
use crate::show::{CLOCK_WRAP, MusicColors, ShowSettings, even, output, places, shade, spectrum};
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
/// slowly, so a quiet verse still reads as quieter than the chorus. When the
/// level falls a long way and stays there while the music plainly carries on
/// (onsets still coming), that is the volume being turned down rather than a
/// quiet passage, and it lets go far faster. In silence it holds, so a track
/// that comes back comes back at the level it left.
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

    /// Learn from `level`, in dB; `playing` is whether the music is still
    /// marking onsets.
    fn learn(&mut self, level: f32, dt: f32, playing: bool) {
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
        self.low_for = if playing && self.reference - level > 9.0 {
            self.low_for + dt
        } else {
            0.0
        };
        self.sinking |= self.low_for > 2.0;
        let tau = if self.sinking { 1.0 } else { 12.0 };
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

    /// Close a hop of `n` samples. Its level, in dB.
    fn close(&mut self, n: usize, dt: f32, playing: bool) -> f32 {
        let rms = (self.sum / n as f32).sqrt();
        self.sum = 0.0;
        let tau = if rms > self.env { 0.01 } else { 0.25 };
        self.env += (rms - self.env) * follow(dt, tau);
        self.gain.learn(db(self.env), dt, playing);
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
/// Beats in a row with no kick before the clock lets go: a kick on every
/// other beat (rock's one and three) keeps it, a breakdown does not.
const MISSES: u32 = 3;
/// How early a beat goes out, so it reaches the bulb as the kick reaches the
/// ear: the trip over the network and the LEDs' own rise.
pub const LEAD: f32 = 0.05;
/// How far back the kicks the tempo is read from go, seconds.
const MEMORY: f32 = 6.0;
/// The analyser's clock moves back by this many seconds whenever it reaches
/// twice that, so an `f32` keeps it to the sample after hours of music.
const CLOCK_SPAN: f32 = 600.0;

/// The beat's clock: the tempo, read from where the kicks fall, and when the
/// next beat is due.
///
/// Once the kicks agree on a tempo, the show plays the beats from this clock
/// rather than from each kick as it is heard. A beat can then go out a little
/// early, so it lands on the bulbs with the kick instead of after it, and a
/// kick the ear hears but the bass did not show still gets its beat. Every
/// kick heard on the beat pulls the clock back into phase; when they stop
/// coming (a breakdown, the next track) it lets go, and every kick plays as
/// it is heard until a tempo is found again.
#[derive(Debug, Clone, Default)]
struct Tempo {
    /// When the recent kicks fell, seconds.
    kicks: VecDeque<f32>,
    /// Seconds a beat; 0 while none is known.
    period: f32,
    /// When the next beat is due.
    next: f32,
    sure: i32,
    /// Beats in a row that came without a kick.
    missed: u32,
    /// The next beat has already been played.
    played: bool,
    /// How hard the kicks have been landing.
    strength: f32,
}

impl Tempo {
    /// Move every time it holds `by` seconds earlier, with the clock.
    fn rewind(&mut self, by: f32) {
        for k in &mut self.kicks {
            *k -= by;
        }
        self.next -= by;
    }

    fn locked(&self) -> bool {
        self.period > 0.0 && self.sure >= SURE
    }

    /// A kick heard at `t`. Whether to play it as a beat.
    fn kick(&mut self, t: f32, strength: f32) -> bool {
        self.kicks.push_back(t);
        while self.kicks.front().is_some_and(|&k| t - k > MEMORY) {
            self.kicks.pop_front();
        }
        self.strength += (strength - self.strength) * 0.3;
        if self.period > 0.0 && (t - self.next).abs() < ON_BEAT * self.period {
            // On the beat: pull the clock into phase and move on to the next.
            let play = !self.locked() || !self.played;
            let e = t - self.next;
            self.sure = (self.sure + 1).min(SUREST);
            self.missed = 0;
            self.period = (self.period + 0.05 * e).clamp(FASTEST, SLOWEST);
            self.next += 0.5 * e + self.period;
            self.played = false;
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

    /// Read the tempo from the recent kicks, and set the clock going from
    /// the latest, at `t`.
    ///
    /// Every pair of kicks votes for the tempos it is a whole number of beats
    /// of; a pair one beat apart counts most, so half the tempo (which fits
    /// every pair too) does not win. A slight lean towards 120 beats a minute
    /// settles the rest.
    fn find(&mut self, t: f32) {
        if self.kicks.len() < 4 {
            return;
        }
        let kicks: Vec<f32> = self.kicks.iter().copied().collect();
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
            for (i, a) in kicks.iter().enumerate() {
                for b in &kicks[i + 1..] {
                    score += fit(b - a, p);
                }
            }
            score *= (-0.5 * lean * lean).exp();
            if score > best.1 {
                best = (p, score);
            }
        }
        let p = best.0;
        if p == 0.0 {
            return;
        }
        // How many of the kicks the beats through the latest one pass by.
        let on = kicks
            .iter()
            .filter(|&&k| {
                let x = (t - k) / p;
                (x - x.round()).abs() * p < 0.03
            })
            .count() as i32;
        self.period = p;
        self.next = t + p;
        self.sure = (on - 2).clamp(0, SUREST);
        self.missed = 0;
        self.played = false;
    }

    /// The clock at `now`. Whether a beat is to be played from it.
    fn tick(&mut self, now: f32, live: bool) -> bool {
        if self.period == 0.0 {
            return false;
        }
        while now > self.next + ON_BEAT * self.period {
            // A beat went by with no kick on it.
            self.missed += 1;
            self.next += self.period;
            self.played = false;
        }
        if self.missed >= MISSES {
            *self = Tempo {
                kicks: std::mem::take(&mut self.kicks),
                strength: self.strength,
                ..Tempo::default()
            };
            return false;
        }
        if live && self.locked() && !self.played && self.next - now <= LEAD {
            self.played = true;
            return true;
        }
        false
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
    /// Seconds a beat, once the tempo is sure.
    pub tempo: Option<f32>,
}

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
    /// Seconds since any band was last struck.
    quiet: f32,
    tempo: Tempo,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(1) as f32;
        let q = std::f32::consts::FRAC_1_SQRT_2;
        Analyzer {
            rate,
            hop: ((rate * HOP_SECS) as usize).max(1),
            count: 0,
            mix: Band::new(Vec::new(), None),
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
            quiet: 10.0,
            tempo: Tempo::default(),
        }
    }

    /// Read the mono samples captured since the last call.
    pub fn feed(&mut self, samples: &[f32]) -> Levels {
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
        levels.energy = self.mix.level();
        levels.bass = self.low.level();
        levels.tempo = self.tempo.locked().then_some(self.tempo.period);
        levels
    }

    /// Close one hop, adding what it heard to `levels`.
    fn step(&mut self, levels: &mut Levels) {
        let n = self.count;
        self.count = 0;
        let dt = n as f32 / self.rate;
        self.time += dt;
        if self.time > 2.0 * CLOCK_SPAN {
            self.time -= CLOCK_SPAN;
            self.tempo.rewind(CLOCK_SPAN);
        }
        let playing = self.quiet < 1.0;
        self.mix.close(n, dt, playing);
        let low = self.low.close(n, dt, playing);
        let mid = self.mid.close(n, dt, playing);
        let high = self.high.close(n, dt, playing);

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
        self.quiet = if kick.or(snare).or(hat).is_some() {
            0.0
        } else {
            self.quiet + dt
        };

        let mut beat = None;
        if let Some(s) = kick {
            levels.kick = true;
            if self.tempo.kick(self.time, s) {
                beat = Some(s);
            }
        }
        // No beat plays into silence, however sure the clock is. The
        // envelope, not this hop: just before a kick is the quietest moment.
        let live = self.mix.level() > 0.2;
        if self.tempo.tick(self.time, live) {
            beat = Some(beat.unwrap_or(0.0).max(self.tempo.strength));
        }
        let most = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        levels.beat = most(levels.beat, beat);
        levels.snare = most(levels.snare, snare);
        levels.hat = most(levels.hat, hat);
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
}

impl Fixture {
    /// Where `light` stands, seen from the seat facing `screen`.
    pub fn place(screen: &Screen, light: &Light, interval: Duration) -> Self {
        let half = (screen.width * 0.5).max(f32::EPSILON);
        let rel = light.pos - screen.center;
        Fixture {
            x: rel.dot(screen.right_axis()) / half,
            depth: rel.dot(screen.facing()) / half,
            height: light.height.clamp(0.0, 1.0),
            interval,
        }
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
            })
            .collect()
    }

    fn quick(&self) -> bool {
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
    /// Each beat sends a pass of light across the room, one way then the
    /// other, reaching every light as it passes where it stands.
    Sweep,
    /// Across and back again.
    Bounce,
    /// Every other light, every other beat.
    Alternate,
    /// The left half on the kick, the right half on the snare.
    Split,
}

impl Pattern {
    /// The patterns `k` punching lights can play, in the order the show
    /// moves through them.
    fn for_lights(k: usize) -> &'static [Pattern] {
        match k {
            0 | 1 => &[Pattern::Together],
            2 => &[
                Pattern::Together,
                Pattern::Alternate,
                Pattern::Sweep,
                Pattern::Split,
            ],
            _ => &[
                Pattern::Together,
                Pattern::Chase,
                Pattern::Sweep,
                Pattern::Alternate,
                Pattern::Bounce,
                Pattern::Split,
            ],
        }
    }

    /// How much of beat `b` the light `rank` of `k` (left to right) takes
    /// as it lands. A sweep's lights take theirs as the pass reaches them.
    fn beat(self, rank: usize, k: usize, b: u32) -> f32 {
        let b = b as usize;
        let on = match self {
            Pattern::Together => true,
            Pattern::Sweep => false,
            Pattern::Chase => rank == b % k,
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
            Pattern::Together | Pattern::Sweep => 0.4,
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
}

/// A pass of light crossing the room, sent by a beat.
#[derive(Debug, Clone, Copy)]
struct Sweep {
    /// How far across the room the pass has come, 0..=1 from where it set
    /// out.
    at: f32,
    /// Room widths a second.
    speed: f32,
    /// It runs left to right, or back.
    rightwards: bool,
    strength: f32,
}

/// The share of a beat a pass takes to cross the room: it is over well
/// before the next beat comes.
const SWEEP_BEATS: f32 = 0.45;

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

/// Beats the show stays on one pattern: two bars of four.
const BEATS_PER_LOOK: u32 = 8;
/// Beats a drop plays everyone together before the patterns come back.
const DROP_BEATS: u32 = 16;
/// The same, in looks.
const DROP_LOOKS: u32 = DROP_BEATS / BEATS_PER_LOOK;
/// How long a build takes to reach white, seconds.
const BUILD_SECS: f32 = 8.0;
/// A white flash's colour temperature.
const WHITE_KELVIN: u16 = 6500;

/// One light on the stage.
#[derive(Debug, Clone, Copy)]
struct Lamp {
    part: Part,
    /// Left to right among the punching lights.
    rank: usize,
    /// How long its flash takes to fade, seconds: longer for a slow bulb, so
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

/// The music show.
///
/// Its colours are laid across the room by where each light stands, as the
/// colour show's are, and never move with the music: a beat changes how
/// bright a light burns, or turns it white, never its colour, so even a sharp
/// punch lands on the colour the light was already gliding through.
#[derive(Debug, Clone)]
pub struct Stage {
    lamps: Vec<Lamp>,
    /// Where each light stands across the room, 0 at the left to 1 at the
    /// right.
    places: Vec<f32>,
    /// The colours' own clock, ticking at the chosen pace.
    clock: f32,
    /// The pass crossing the room, if a beat sent one.
    sweep: Option<Sweep>,
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
    since_kick: f32,
    /// Seconds a beat: the clock's, or a guess.
    period: f32,
    /// Beats into the current look, and looks so far.
    beat: u32,
    looks: u32,
    /// Snares heard in this build: they walk it across the room.
    roll: usize,
    sharp: Vec<bool>,
}

impl Stage {
    pub fn new(fixtures: &[Fixture]) -> Self {
        let parts = cast(fixtures);
        let mut order: Vec<usize> = (0..fixtures.len())
            .filter(|&i| parts[i] == Part::Punch)
            .collect();
        order.sort_by(|&a, &b| fixtures[a].x.total_cmp(&fixtures[b].x));
        let lamps = fixtures
            .iter()
            .zip(&parts)
            .enumerate()
            .map(|(i, (f, &part))| Lamp {
                part,
                rank: order.iter().position(|&o| o == i).unwrap_or(0),
                fade: (2.5 * f.interval.as_secs_f32()).max(0.16),
                flash: 0.0,
                white: 0.0,
                sharp: false,
            })
            .collect();
        Stage {
            lamps,
            places: places(fixtures),
            clock: 0.0,
            sweep: None,
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
            since_kick: 10.0,
            period: 0.5,
            beat: 0,
            looks: 0,
            roll: 0,
            sharp: vec![false; fixtures.len()],
        }
    }

    /// The lights that should be sent sharp this frame rather than faded in.
    pub fn sharp(&self) -> &[bool] {
        &self.sharp
    }

    fn pattern(&self) -> Pattern {
        if self.section == Section::Drop {
            return Pattern::Together;
        }
        let all = Pattern::for_lights(self.punches);
        all[self.looks as usize % all.len()]
    }

    /// Whether the white flashes are on. How often they may go out is not
    /// the show's to decide: the show's guard
    /// ([`crate::safety::FlashGuard::show`]) keeps them to three a second,
    /// protection on or off.
    fn white(&self, settings: &ShowSettings) -> bool {
        settings.flashes
    }

    fn enter(&mut self, section: Section) {
        self.section = section;
        self.held = 0.0;
        self.roll = 0;
        if section == Section::Hold {
            for lamp in &mut self.lamps {
                lamp.flash = 0.0;
                lamp.white = 0.0;
            }
        }
    }

    /// Read where the track is from what was heard this frame.
    fn follow_track(&mut self, settings: &ShowSettings, levels: &Levels) {
        let gone = self.since_kick > (2.5 * self.period).max(1.2);
        let back = levels.kick;
        match self.section {
            Section::Groove | Section::Drop if gone => self.enter(Section::Calm),
            Section::Calm if back => {
                // After a long break the kick's return is a drop of its own.
                if self.held > 6.0 {
                    self.drop(settings);
                } else {
                    self.enter(Section::Groove);
                }
            }
            Section::Calm => {
                let crowding = self.hits > 3.0 && self.hits > 1.3 * self.hits_long;
                let climbing = self.short > self.recent + 0.06;
                if self.held > 1.0 && (crowding || climbing) {
                    self.enter(Section::Build);
                }
            }
            Section::Build | Section::Hold if back => self.drop(settings),
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

    /// The drop: everyone at once, white if allowed, and a new pattern after.
    fn drop(&mut self, settings: &ShowSettings) {
        self.enter(Section::Drop);
        self.looks = (self.looks / DROP_LOOKS + 1) * DROP_LOOKS;
        self.sweep = None;
        self.beat = 0;
        let white = self.white(settings);
        for lamp in &mut self.lamps {
            lamp.hit(1.0, white);
        }
    }

    /// Advance by `dt` seconds with what the music is doing.
    pub fn frame(&mut self, settings: &ShowSettings, levels: Levels, dt: f32) -> Vec<BulbOutput> {
        for lamp in &mut self.lamps {
            lamp.flash *= (-dt / lamp.fade).exp();
            lamp.white *= (-dt / (0.6 * lamp.fade)).exp();
            lamp.sharp = false;
        }
        self.held += dt;
        self.clock = (self.clock + dt * settings.pace()) % CLOCK_WRAP;
        self.since_kick = if levels.kick {
            0.0
        } else {
            self.since_kick + dt
        };
        if let Some(p) = levels.tempo {
            self.period = p;
        }
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
        let tau = if levels.bass > self.bass { 0.06 } else { 0.4 };
        self.bass += (levels.bass - self.bass) * follow(dt, tau);

        self.follow_track(settings, &levels);
        match self.section {
            Section::Groove | Section::Drop => self.groove(settings, &levels),
            Section::Build => self.build(settings, &levels),
            Section::Calm => {
                if let Some(s) = levels.snare {
                    self.accent(0.5 * s, false);
                }
            }
            Section::Hold => {}
        }
        self.pass(dt);
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
        outputs
    }

    /// The beat is on: punch it in the current pattern, and answer the snare.
    fn groove(&mut self, settings: &ShowSettings, levels: &Levels) {
        if let Some(s) = levels.beat {
            self.beat += 1;
            if self.beat >= BEATS_PER_LOOK {
                self.beat = 0;
                self.looks += 1;
                if self.section == Section::Drop && self.looks.is_multiple_of(DROP_LOOKS) {
                    self.enter(Section::Groove);
                }
            }
            let pattern = self.pattern();
            let (k, b) = (self.punches, self.beat);
            if pattern == Pattern::Sweep {
                self.sweep = Some(Sweep {
                    at: -1e-3,
                    speed: 1.0 / (SWEEP_BEATS * self.period.max(FASTEST)),
                    rightwards: !b.is_multiple_of(2),
                    strength: s,
                });
            }
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

    /// A build: every snare walks the light one step further across the
    /// room, harder and whiter as it climbs.
    fn build(&mut self, settings: &ShowSettings, levels: &Levels) {
        let climb = (self.held / BUILD_SECS).min(1.0);
        if let Some(s) = levels.snare.or(levels.hat.filter(|_| climb > 0.5)) {
            let k = self.punches.max(1);
            let at = self.roll % k;
            self.roll += 1;
            let white = climb > 0.8 && self.white(settings);
            for lamp in &mut self.lamps {
                if lamp.part == Part::Punch && lamp.rank == at {
                    lamp.hit((0.3 + 0.7 * climb) * s, white);
                }
            }
            self.accent((0.3 + 0.7 * climb) * s, false);
        }
    }

    /// Move the pass on by `dt`, hitting every punching light it reaches.
    fn pass(&mut self, dt: f32) {
        let Some(sweep) = &mut self.sweep else {
            return;
        };
        let from = sweep.at;
        sweep.at += sweep.speed * dt;
        for (lamp, &place) in self.lamps.iter_mut().zip(&self.places) {
            let along = if sweep.rightwards { place } else { 1.0 - place };
            if lamp.part == Part::Punch && from < along && along <= sweep.at {
                lamp.hit(sweep.strength, false);
            }
        }
        if sweep.at >= 1.0 {
            self.sweep = None;
        }
    }

    /// The colour light `i` glides through, before the music touches it.
    fn colour(&self, settings: &ShowSettings, i: usize) -> Rgb {
        let place = self.places[i];
        match settings.music {
            MusicColors::Spectrum => spectrum(self.clock, place),
            MusicColors::Fixed => shade(settings.color, place),
        }
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
        let bright = settings.bright();
        let swell = self.swell.clamp(0.0, 1.0).powf(2.5);
        let climb = if self.section == Section::Build {
            (self.held / BUILD_SECS).min(1.0)
        } else {
            0.0
        };
        self.lamps
            .iter()
            .enumerate()
            .map(|(i, lamp)| {
                let rgb = self.colour(settings, i);
                // What it glows at between hits.
                let base = match lamp.part {
                    Part::Punch => 0.1 + 0.4 * swell,
                    Part::Wash => 0.15 + 0.55 * self.bass.clamp(0.0, 1.0).powf(1.5) + 0.15 * swell,
                    Part::Accent => 0.05 + 0.2 * self.shimmer + 0.15 * swell,
                };
                let base = match self.section {
                    // Lights out for the breath before the drop.
                    Section::Hold => 0.0,
                    Section::Calm => 0.6 * base,
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
                    // A build drains the colour towards white as it climbs.
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
        stage.looks = 1; // Chase.
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

    /// How far apart two colours are round the wheel, in degrees.
    fn gap(a: Rgb, b: Rgb) -> f32 {
        let hue = |c: Rgb| {
            let (r, g, b) = (f32::from(c.r), f32::from(c.g), f32::from(c.b));
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            let d = (max - min).max(1e-3);
            if max == r {
                60.0 * ((g - b) / d).rem_euclid(6.0)
            } else if max == g {
                60.0 * ((b - r) / d + 2.0)
            } else {
                60.0 * ((r - g) / d + 4.0)
            }
        };
        let d = (hue(a) - hue(b)).abs();
        d.min(360.0 - d)
    }

    #[test]
    fn the_beat_never_changes_a_colour() {
        // The music plays the brightness; the colours glide on regardless,
        // even across a punch sent sharp.
        let s = ShowSettings {
            flashes: false,
            ..ShowSettings::default()
        };
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        let frames = groove(&mut stage, &s, 96);
        for w in frames.windows(2) {
            for (a, b) in w[0].iter().zip(&w[1]) {
                let g = gap(rgb(*a), rgb(*b));
                assert!(g < 2.0, "a colour jumped {g}°: {a:?} -> {b:?}");
            }
        }
        // And the lights still show a combination, not one colour.
        let last = &frames[frames.len() - 1];
        assert!(gap(rgb(last[0]), rgb(last[2])) > 20.0);
    }

    #[test]
    fn a_fixed_colour_holds_in_shades() {
        let red = Rgb::new(255, 0, 0);
        let s = ShowSettings {
            flashes: false,
            music: MusicColors::Fixed,
            color: red,
            ..ShowSettings::default()
        };
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        let frames = groove(&mut stage, &s, 16);
        for frame in &frames {
            // The middle light holds the colour itself, the others a shade
            // either side of it, all through the music.
            assert_eq!(rgb(frame[1]), red);
            for o in [frame[0], frame[2]] {
                let g = gap(rgb(o), red);
                assert!((10.0..=20.0).contains(&g), "a shade {g}° off");
            }
        }
        // A lone light holds it exactly.
        let mut stage = Stage::new(&Fixture::row(1));
        for frame in groove(&mut stage, &s, 4) {
            assert_eq!(rgb(frame[0]), red);
        }
    }

    #[test]
    fn a_sweep_passes_across_the_room_and_back() {
        let s = ShowSettings {
            flashes: false,
            ..ShowSettings::default()
        };
        let mut stage = Stage::new(&Fixture::row(3));
        stage.enter(Section::Groove);
        stage.looks = 2;
        assert_eq!(stage.pattern(), Pattern::Sweep);
        // The frame each light is struck on, for two beats in a row.
        let mut struck = Vec::new();
        for _ in 0..2 {
            let mut at = [usize::MAX; 3];
            for k in 0..20 {
                let l = if k == 0 { beat(1.0) } else { between() };
                stage.frame(&s, l, 0.025);
                for (i, sharp) in stage.sharp().iter().enumerate() {
                    if *sharp && at[i] == usize::MAX {
                        at[i] = k;
                    }
                }
            }
            struck.push(at);
        }
        let (there, back) = (struck[0], struck[1]);
        assert!(
            there.iter().all(|&k| k < 20),
            "a light was missed: {there:?}"
        );
        assert!(there.windows(2).all(|w| w[0] < w[1]), "{there:?}");
        assert!(back.windows(2).all(|w| w[0] > w[1]), "{back:?}");
        // The pass is over well before the next beat.
        assert!(there[2] < 12, "{there:?}");
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
        let out = stage.frame(&s, hit, 0.025)[0];
        let flash = out.bright();
        // Full brightness, evened for the colour it lands on.
        let top = (f32::from(s.bright) * even(rgb(out))).round() as u8;
        assert!(flash >= top - 5 && flash <= top, "{flash}, {top} at most");
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
