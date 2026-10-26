//! The listening half of the show: raw samples in, [`Levels`] out.

use super::dsp::{Band, Biquad, HAT, KICK, PULSE, Pass, SNARE, follow};
use super::tempo::{PULSE_WEIGHT, SNARE_WEIGHT, Tempo};

/// Length of one analysis step: short enough to place a beat within a few
/// milliseconds whatever the frame rate, long enough to measure the bass.
const HOP_SECS: f32 = 0.01;

/// How early a beat goes out until the lights say how long they take, so it
/// reaches the bulb as the kick reaches the ear: the trip over the network
/// and the LEDs' own rise.
pub const LEAD: f32 = 0.05;
/// The earliest a beat may go out. Past this the clock would be guessing
/// more than a short way ahead of what it heard.
pub const MOST_LEAD: f32 = 0.25;

/// Seconds of silence after which the clock forgets the beat altogether
/// (counted once the music has faded out, some seven tenths in).
const QUIET_RESET: f32 = 0.75;

/// The analyser's clock moves back by this many seconds whenever it reaches
/// twice that, so an `f32` keeps it to the sample after hours of music.
pub(super) const CLOCK_SPAN: f32 = 600.0;
/// The furthest past the last sample heard the clock reads the music to
/// have got: about the longest gap between two packets of sound.
const AHEAD: f32 = 0.04;
/// A gap between two packets this much longer than usual is a stall, not
/// the two clocks drifting apart.
const STALL: f32 = 0.2;

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

/// Turns raw samples into [`Levels`].
///
/// The sound is split into three bands, each with its own instrument: under
/// 150 Hz the kick and the bass line, from 1 to 4 kHz the snare and claps, over
/// 7 kHz the hi-hats and cymbals. Each is read against its own level in the
/// track (see [`Gain`](super::dsp::Gain)), so the show looks the same with
/// the volume at 10% or at 100%, and from one master to the next.
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
    pub(super) time: f32,
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
    pub(super) tempo: Tempo,
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
    /// (see [`Stage::lead`](super::Stage::lead)).
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
