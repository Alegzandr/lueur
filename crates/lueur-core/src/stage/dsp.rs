//! Hearing the sound: the filters that split it into bands, how loud each
//! plays against the track, and where each is struck.

use std::f32::consts::TAU;

/// Below this a signal is silence, whatever the gain (dBFS).
const SILENCE_DB: f32 = -75.0;

/// How far under the loud parts of a track a level reads as nothing, in dB.
const RANGE_DB: f32 = 30.0;

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-6).log10()
}

/// The share of the way an exponential follower with time constant `tau`
/// moves in `dt`.
pub(super) fn follow(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau).exp()
}

/// Which side of a frequency a [`Biquad`] keeps.
#[derive(Debug, Clone, Copy)]
pub(super) enum Pass {
    Low,
    High,
    Band,
}

/// A second-order filter (the audio cookbook's): enough to split the sound
/// into bands without an FFT. Run in double precision, which a low cut-off at
/// a high sample rate needs to stay stable.
#[derive(Debug, Clone, Copy)]
pub(super) struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub(super) fn new(pass: Pass, rate: f32, freq: f32, q: f32) -> Self {
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

    pub(super) fn run(&mut self, x: f32) -> f32 {
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
pub(super) struct Gain {
    /// The level of the loud parts, dB (NaN until something is heard).
    reference: f32,
    /// How long the level has been far under it with the music still going.
    low_for: f32,
    /// The volume went down: follow it fast until the loud parts are found
    /// again.
    sinking: bool,
}

impl Gain {
    pub(super) fn new() -> Self {
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
pub(super) struct Ear {
    /// How fast the band's recent level follows it, seconds: a hit is a rise
    /// above it.
    follow: f32,
    /// The least rise that can be a hit, dB.
    floor: f32,
    /// The least time between two hits, seconds.
    rest: f32,
}

pub(super) const KICK: Ear = Ear {
    follow: 0.25,
    floor: 4.0,
    rest: 0.22,
};
pub(super) const SNARE: Ear = Ear {
    follow: 0.15,
    floor: 5.0,
    rest: 0.12,
};
pub(super) const HAT: Ear = Ear {
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
    pub(super) fn new(ear: Ear) -> Self {
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
pub(super) struct Band {
    filters: Vec<Biquad>,
    /// Sum of squares over the hop being gathered.
    sum: f32,
    /// Fast attack, slower release: the lights jump with the music and settle.
    env: f32,
    gain: Gain,
    onset: Option<Onset>,
}

impl Band {
    pub(super) fn new(filters: Vec<Biquad>, ear: Option<Ear>) -> Self {
        Band {
            filters,
            sum: 0.0,
            env: 0.0,
            gain: Gain::new(),
            onset: ear.map(Onset::new),
        }
    }

    pub(super) fn push(&mut self, s: f32) {
        let s = self.filters.iter_mut().fold(s, |s, f| f.run(s));
        self.sum += s * s;
    }

    /// Close a hop of `n` samples. Its level, in dB. `steady` is whether the
    /// groove carries on (see [`Gain::learn`]).
    pub(super) fn close(&mut self, n: usize, dt: f32, steady: bool) -> f32 {
        let rms = (self.sum / n as f32).sqrt();
        self.sum = 0.0;
        let tau = if rms > self.env { 0.01 } else { 0.25 };
        self.env += (rms - self.env) * follow(dt, tau);
        self.gain.learn(db(self.env), dt, steady);
        db(rms)
    }

    /// Whether it was struck in the hop at `level` dB.
    pub(super) fn hit(&mut self, level: f32, dt: f32, warm: bool) -> Option<f32> {
        // Hiss in a pause is no hit: the band must carry some of the music.
        let live = warm && self.gain.read(level) > 0.1;
        self.onset.as_mut()?.step(level, dt, live)
    }

    /// How loud it plays now, against how loud the track plays it.
    pub(super) fn level(&self) -> f32 {
        self.gain.read(db(self.env))
    }
}

/// The broadband ear: a strum, a chord, a stab, anything that starts at
/// once across the mix.
pub(super) const PULSE: Ear = Ear {
    follow: 0.15,
    floor: 4.0,
    rest: 0.1,
};
