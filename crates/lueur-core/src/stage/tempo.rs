//! The beat's clock: the tempo, read from where the hits fall.

use std::collections::VecDeque;

/// Tempos the clock looks for, seconds a beat: 180 down to 80 beats a minute.
pub(super) const FASTEST: f32 = 60.0 / 180.0;
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

/// How far back the hits the tempo is read from go, seconds.
const MEMORY: f32 = 6.0;
/// The most hits the tempo is read from: a snare roll must not make
/// reading it slow.
const MEMORY_HITS: usize = 48;
/// The least time between two readings of the tempo while the clock is
/// unsure, seconds.
const REREAD: f32 = 0.25;

/// How often a sure clock checks that the hits still fall on its beats,
/// seconds.
const RECHECK: f32 = 1.0;

/// What a hit is worth to the clock, by what struck it. The kick sets the
/// beat; a snare, or a strum or a chord heard in the whole mix, keeps a
/// track without a kick in time, and the kick's own clock in phase through
/// a fill. Hi-hats do not count: they come so thick that they would keep
/// any clock going.
const KICK_WEIGHT: f32 = 1.0;
pub(super) const SNARE_WEIGHT: f32 = 0.6;
pub(super) const PULSE_WEIGHT: f32 = 0.5;

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
pub(super) struct Tempo {
    /// When the recent hits fell, seconds, and what each is worth.
    pub(super) hits: VecDeque<(f32, f32)>,
    /// Seconds a beat; 0 while none is known.
    pub(super) period: f32,
    /// When the next beat is due.
    next: f32,
    sure: i32,
    /// Beats in a row that came with nothing struck on them.
    missed: u32,
    /// The next beat has already been played.
    played: bool,
    /// How hard the kicks have been landing.
    pub(super) strength: f32,
    /// When the tempo was last read.
    read_at: f32,
}

impl Tempo {
    /// Move every time it holds `by` seconds earlier, with the clock.
    pub(super) fn rewind(&mut self, by: f32) {
        for (k, _) in &mut self.hits {
            *k -= by;
        }
        self.next -= by;
        self.read_at -= by;
    }

    pub(super) fn locked(&self) -> bool {
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
    pub(super) fn kick(&mut self, t: f32, strength: f32) -> bool {
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
    pub(super) fn hit(&mut self, t: f32, weight: f32) {
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
    pub(super) fn recheck(&mut self, t: f32) {
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
    pub(super) fn tick(&mut self, now: f32, live: bool, lead: f32) -> bool {
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
    pub(super) fn due(&self, now: f32, lead: f32) -> Option<f32> {
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
