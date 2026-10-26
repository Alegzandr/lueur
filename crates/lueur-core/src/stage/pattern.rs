//! How the punching lights share out the beats.

/// How the punching lights share out the beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pattern {
    /// All of them, every beat.
    Together,
    /// One after another across the room, left to right: each light holds
    /// the beat until the next takes it over, then goes out.
    Chase,
    /// From the middle out to the ends, ring by ring.
    CentreOut,
    /// Across and back again, handed on like a chase.
    Bounce,
    /// Every other light holds the beat in its colour and hands it to the
    /// others on the next: the room turns over on every beat.
    Alternate,
    /// From the ends in to the middle, ring by ring.
    OutsideIn,
    /// The left half on the kick, the right half on the snare.
    Split,
    /// Every light on, every other one in the second colour: the two swap
    /// on every beat.
    Swap,
    /// Every light on in the harmony's three colours, which move one light
    /// along on every beat.
    Carousel,
}

/// How bright a light holding the beat burns between two beats: one
/// handing the beat on, and every light while they all trade colours.
const PASS_HOLD: f32 = 0.6;
const SHARED_HOLD: f32 = 0.4;

/// How far light `rank` of `k` (left to right) stands from the middle, in
/// rings: 0 for the middle one (or two). And how many rings there are.
pub(super) fn ring(rank: usize, k: usize) -> (usize, usize) {
    ((2 * rank).abs_diff(k.saturating_sub(1)) / 2, k.div_ceil(2))
}

/// How hard a passage plays, as the looks are chosen for it: a verse, the
/// body of the track, its chorus.
pub(super) const GENTLE: usize = 0;
pub(super) const DRIVING: usize = 1;
pub(super) const PEAK: usize = 2;

impl Pattern {
    /// The patterns `k` punching lights can play, by how hard the music
    /// plays, the way a lighting desk keeps its looks: in a gentle passage
    /// every light stays lit and the colours trade places; in the body of
    /// the track the beat travels across the room; at its peak the room
    /// strikes at once or bursts from the middle. The ones that need a
    /// middle and two ends wait for three lights.
    pub(super) fn for_lights(k: usize, tier: usize) -> &'static [Pattern] {
        const ONE: [&[Pattern]; 3] = [&[Pattern::Together]; 3];
        const TWO: [&[Pattern]; 3] = [
            &[Pattern::Swap, Pattern::Alternate],
            &[Pattern::Split, Pattern::Alternate],
            &[Pattern::Together],
        ];
        const MORE: [&[Pattern]; 3] = [
            &[Pattern::Swap, Pattern::Carousel, Pattern::Alternate],
            &[Pattern::Chase, Pattern::Bounce, Pattern::Split],
            &[Pattern::Together, Pattern::CentreOut, Pattern::OutsideIn],
        ];
        let tiers = match k {
            0 | 1 => &ONE,
            2 => &TWO,
            _ => &MORE,
        };
        tiers[tier.min(PEAK)]
    }

    /// How much of beat `b` the light `rank` of `k` (left to right) takes.
    pub(super) fn beat(self, rank: usize, k: usize, b: u32) -> f32 {
        let b = b as usize;
        let (r, rings) = ring(rank, k);
        let on = match self {
            Pattern::Together | Pattern::Swap | Pattern::Carousel => true,
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
    pub(super) fn snare(self, rank: usize, k: usize, b: u32) -> f32 {
        match self {
            Pattern::Together | Pattern::Swap | Pattern::Carousel => 0.4,
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

    /// How bright light `rank` of `k` holds on after beat `b` until the
    /// next, if it holds the beat at all: in the patterns that hand it on,
    /// the light that has it keeps it, and goes out as the next one takes
    /// it.
    pub(super) fn hold(self, rank: usize, k: usize, b: u32) -> f32 {
        match self {
            Pattern::Chase | Pattern::Bounce | Pattern::Alternate => {
                PASS_HOLD * self.beat(rank, k, b)
            }
            Pattern::Swap | Pattern::Carousel => SHARED_HOLD,
            _ => 0.0,
        }
    }

    /// Which colour light `rank` of `k` strikes in on beat `b`: 0 the first
    /// hit colour, 1 the second, 2 the back colour. The two sides of a
    /// pattern that has sides answer each other in colour too, and some
    /// trade their colours on every beat.
    pub(super) fn colour(self, rank: usize, k: usize, b: u32) -> usize {
        let b = b as usize;
        match self {
            // The half that has the beat strikes in its own colour: the
            // colours turn over with it.
            Pattern::Alternate => rank % 2,
            Pattern::Split => usize::from(rank >= k.div_ceil(2)),
            Pattern::CentreOut | Pattern::OutsideIn => ring(rank, k).0 % 2,
            Pattern::Swap => (rank + b) % 2,
            // Each light takes the colour its left neighbour had: the
            // colours travel left to right.
            Pattern::Carousel => (rank + 2 * b) % 3,
            _ => 0,
        }
    }
}
