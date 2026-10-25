//! Bringing back a bulb that dropped.
//!
//! A Wi-Fi hiccup, a bulb cut at its wall switch, a bridge that rebooted: a
//! stream that fails is not the end of the light. [`Relink`] keeps opening a
//! new one, further apart each time (see [`Backoff`]), on a thread of its
//! own, so the frame loop never waits on a bulb that is not there. Once one
//! opens, the very next frame goes to it, and the bulb picks up whatever the
//! room shows at that moment.

use super::LightStream;
use crate::color::BulbOutput;
use crate::yeelight::Effect;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// The wait before the first attempt after a bulb drops.
pub const RELINK_FIRST: Duration = Duration::from_millis(500);
/// The longest wait between two attempts: a bulb that is gone for good costs
/// one connection attempt every half minute.
pub const RELINK_CAP: Duration = Duration::from_secs(30);
/// How much earlier than its nominal time an attempt may come, as a share
/// of the wait, so lights that dropped together do not all knock at once.
pub const RELINK_JITTER: f64 = 0.2;
/// How long a reopened stream must take frames without an error before the
/// bulb counts as back. Opening proves little for some families (a UDP
/// socket, a Hue client: neither talks to the bulb), and a bulb still away
/// fails its first frames: until then it keeps its place in the backoff and
/// shows as reconnecting, rather than flapping between the two. Longer than
/// [`super::common::REFRESH`], so even a still scene has sent again by then.
pub const RELINK_STABLE: Duration = Duration::from_secs(8);

/// How long to wait before each attempt: doubling from [`RELINK_FIRST`] up to
/// [`RELINK_CAP`], each wait shortened by a random part of [`RELINK_JITTER`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Backoff {
    failures: u32,
}

impl Backoff {
    /// The wait after `failures` failed attempts in a row, for a `jitter`
    /// drawn in `0.0..=1.0`. Never longer than [`RELINK_CAP`].
    pub fn delay(failures: u32, jitter: f64) -> Duration {
        let nominal = RELINK_FIRST
            .saturating_mul(1u32 << failures.min(31))
            .min(RELINK_CAP);
        nominal.mul_f64(1.0 - RELINK_JITTER * jitter.clamp(0.0, 1.0))
    }

    /// The wait before the next attempt, counting it as one more failure.
    pub fn next_wait(&mut self) -> Duration {
        let wait = Self::delay(self.failures, jitter());
        self.failures = self.failures.saturating_add(1);
        wait
    }

    /// The link is back: the next drop starts from the shortest wait again.
    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

/// A number in `0.0..1.0` that differs from call to call. The standard
/// library's hasher keys are random per instance, which is plenty to spread
/// a few reconnections apart.
fn jitter() -> f64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u8(0);
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Opens a fresh stream to one bulb. Called on a thread of its own, so it may
/// take as long as the family's handshake does.
pub type Opener = Arc<dyn Fn() -> io::Result<Box<dyn LightStream>> + Send + Sync>;

/// Called when a link goes down or comes back.
pub type OnChange = Arc<dyn Fn() + Send + Sync>;

type Attempt = Receiver<io::Result<Box<dyn LightStream>>>;

/// A stream to one bulb that reopens itself when it fails.
///
/// [`Relink::push`] never fails and never waits: while the bulb is away it
/// only checks, once a frame, whether an attempt is due or has finished.
pub struct Relink {
    open: Opener,
    stream: Option<Box<dyn LightStream>>,
    /// The stream that failed, closed by the next attempt's thread rather
    /// than on the frame loop.
    stale: Option<Box<dyn LightStream>>,
    attempt: Option<Attempt>,
    backoff: Backoff,
    next_try: Instant,
    interval: Duration,
    /// When the stream on trial was reopened (see [`RELINK_STABLE`]).
    trial: Option<Instant>,
    stable: Duration,
    /// True while the bulb takes frames, for whoever reports on the lights.
    up: Arc<AtomicBool>,
    /// Set once this link is done with, so an attempt that has not started
    /// yet is not made.
    gone: Arc<AtomicBool>,
    on_change: Option<OnChange>,
    /// The light this link feeds, for the logs.
    name: String,
}

impl Relink {
    /// Wrap a stream `open` has just opened; it is called again whenever the
    /// stream fails.
    pub fn new(stream: Box<dyn LightStream>, open: Opener) -> Self {
        Relink {
            interval: stream.interval(),
            open,
            stream: Some(stream),
            stale: None,
            attempt: None,
            backoff: Backoff::default(),
            next_try: Instant::now(),
            trial: None,
            stable: RELINK_STABLE,
            up: Arc::new(AtomicBool::new(true)),
            gone: Arc::new(AtomicBool::new(false)),
            on_change: None,
            name: String::new(),
        }
    }

    /// Name the light this link feeds, so its logs say which one it is.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Call `f` (on the frame loop: it must return at once) each time the
    /// link goes down or comes back.
    pub fn on_change(mut self, f: OnChange) -> Self {
        self.on_change = Some(f);
        self
    }

    /// Whether the bulb takes frames now: its stream opened with the effect,
    /// or reopened and has held for [`RELINK_STABLE`].
    pub fn is_up(&self) -> bool {
        self.up.load(Ordering::SeqCst)
    }

    /// The same as [`Relink::is_up`], readable from another thread.
    pub fn up_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.up)
    }

    /// The pace of the stream the link opened with (see
    /// [`LightStream::interval`]).
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Send one frame if the bulb is there; otherwise move the reconnection
    /// along. A stream that opened since the last frame gets this one in
    /// full, since it has sent nothing yet.
    pub fn push(&mut self, output: BulbOutput, effect: Effect, duration_ms: u32) {
        self.poll();
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        match stream.push(output, effect, duration_ms) {
            Ok(()) => {
                if self.trial.is_some_and(|t| t.elapsed() >= self.stable) {
                    log::info!(light = self.name.as_str(); "a light is back");
                    self.trial = None;
                    self.backoff.reset();
                    self.set_up(true);
                }
            }
            Err(e) => {
                // A stream on trial that fails was only one more attempt:
                // the next waits longer still, and nothing flaps.
                if self.trial.take().is_some() {
                    log::debug!(light = self.name.as_str(); "reconnection failed: {e}");
                } else {
                    log::warn!(light = self.name.as_str(); "a light dropped, reconnecting: {e}");
                }
                self.stale = self.stream.take();
                self.next_try = Instant::now() + self.backoff.next_wait();
                self.set_up(false);
            }
        }
    }

    /// Settle the bulb, if it is there, and give up on bringing it back.
    pub fn rest(&mut self) {
        self.gone.store(true, Ordering::SeqCst);
        self.attempt = None;
        if let Some(stream) = self.stream.as_mut() {
            stream.rest();
        }
    }

    /// Take the outcome of the attempt under way, or start one when due.
    fn poll(&mut self) {
        if self.gone.load(Ordering::SeqCst) {
            return;
        }
        if let Some(attempt) = &self.attempt {
            match attempt.try_recv() {
                Err(TryRecvError::Empty) => {}
                // On trial until it has held (see `push`).
                Ok(Ok(stream)) => {
                    self.attempt = None;
                    self.stream = Some(stream);
                    self.trial = Some(Instant::now());
                }
                Ok(Err(e)) => {
                    log::debug!(light = self.name.as_str(); "reconnection failed: {e}");
                    self.attempt = None;
                    self.next_try = Instant::now() + self.backoff.next_wait();
                }
                Err(TryRecvError::Disconnected) => {
                    self.attempt = None;
                    self.next_try = Instant::now() + self.backoff.next_wait();
                }
            }
            return;
        }
        if self.stream.is_none() && Instant::now() >= self.next_try {
            self.attempt = self.start_attempt();
            if self.attempt.is_none() {
                self.next_try = Instant::now() + self.backoff.next_wait();
            }
        }
    }

    fn start_attempt(&mut self) -> Option<Attempt> {
        let (tx, rx) = mpsc::sync_channel(1);
        let open = Arc::clone(&self.open);
        let stale = self.stale.take();
        let gone = Arc::clone(&self.gone);
        std::thread::Builder::new()
            .name("lueur-relink".into())
            .spawn(move || {
                drop(stale);
                if gone.load(Ordering::SeqCst) {
                    return;
                }
                let opened = open();
                // Stopped meanwhile ("all off", the sleep timer): the new
                // stream is dropped here, off the frame loop, without a frame
                // that would light the bulb again.
                if !gone.load(Ordering::SeqCst) {
                    let _ = tx.send(opened);
                }
            })
            .ok()
            .map(|_| rx)
    }

    fn set_up(&self, up: bool) {
        if self.up.swap(up, Ordering::SeqCst) != up
            && let Some(f) = &self.on_change
        {
            f();
        }
    }
}

impl Drop for Relink {
    fn drop(&mut self) {
        self.gone.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn waits_double_from_half_a_second_up_to_the_cap() {
        let waits: Vec<u128> = (0..9).map(|n| Backoff::delay(n, 0.0).as_millis()).collect();
        assert_eq!(
            waits,
            [500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]
        );
        assert_eq!(Backoff::delay(u32::MAX, 0.0), RELINK_CAP);
    }

    #[test]
    fn jitter_only_ever_shortens_a_wait() {
        for n in 0..12 {
            let nominal = Backoff::delay(n, 0.0);
            let early = Backoff::delay(n, 1.0);
            assert!(early <= nominal && early >= nominal.mul_f64(1.0 - RELINK_JITTER));
            // Out-of-range draws are held to the range.
            assert_eq!(Backoff::delay(n, 7.0), early);
            assert_eq!(Backoff::delay(n, -1.0), nominal);
        }
    }

    #[test]
    fn a_backoff_grows_with_failures_and_starts_over_once_reset() {
        let mut b = Backoff::default();
        let first = b.next_wait();
        assert!(first <= RELINK_FIRST && first >= RELINK_FIRST.mul_f64(1.0 - RELINK_JITTER));
        let second = b.next_wait();
        assert!(second > first, "{second:?} after {first:?}");
        for _ in 0..20 {
            assert!(b.next_wait() <= RELINK_CAP);
        }
        b.reset();
        assert!(b.next_wait() <= RELINK_FIRST);
    }

    #[test]
    fn draws_vary_and_stay_in_range() {
        let draws: Vec<f64> = (0..32).map(|_| jitter()).collect();
        assert!(draws.iter().all(|d| (0.0..1.0).contains(d)), "{draws:?}");
        assert!(draws.windows(2).any(|w| w[0] != w[1]), "{draws:?}");
    }

    /// A stream that fails while its bulb is `down` or `broken`, and logs
    /// what it is sent.
    struct Fake {
        bulb: Bulb,
    }

    impl LightStream for Fake {
        fn push(&mut self, output: BulbOutput, _: Effect, _: u32) -> io::Result<()> {
            let b = &self.bulb;
            if b.down.load(Ordering::SeqCst) || b.broken.load(Ordering::SeqCst) {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            b.log.lock().unwrap().push(output.bright());
            Ok(())
        }
        fn rest(&mut self) {}
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.bulb.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn frame(bright: u8) -> BulbOutput {
        BulbOutput::Color {
            rgb: Rgb::new(255, 0, 0),
            bright,
        }
    }

    /// A fake bulb's switches, and what happened to it.
    #[derive(Clone, Default)]
    struct Bulb {
        /// Unreachable: opening fails, and so does every frame.
        down: Arc<AtomicBool>,
        /// Opening "succeeds" but every frame fails, like a Hue client or a
        /// UDP socket to a bulb that is not there.
        broken: Arc<AtomicBool>,
        log: Arc<Mutex<Vec<u8>>>,
        opens: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
    }

    /// A link to a fake bulb, each connection taking `handshake`. A reopened
    /// stream counts as back after 100 ms here.
    fn rig(handshake: Duration) -> (Relink, Bulb) {
        let bulb = Bulb::default();
        let open: Opener = {
            let bulb = bulb.clone();
            Arc::new(move || {
                bulb.opens.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(handshake);
                if bulb.down.load(Ordering::SeqCst) {
                    return Err(io::Error::from(io::ErrorKind::TimedOut));
                }
                Ok(Box::new(Fake { bulb: bulb.clone() }) as Box<dyn LightStream>)
            })
        };
        let mut link = Relink::new(open().unwrap(), open);
        link.stable = ms(100);
        bulb.opens.store(0, Ordering::SeqCst);
        (link, bulb)
    }

    fn counter(n: &Arc<AtomicUsize>) -> OnChange {
        let n = Arc::clone(n);
        Arc::new(move || {
            n.fetch_add(1, Ordering::SeqCst);
        })
    }

    /// Push frames every 10 ms for `span`, numbering them from `from`; the
    /// longest a push took.
    fn play(link: &mut Relink, from: u8, span: Duration) -> Duration {
        let start = Instant::now();
        let mut worst = Duration::ZERO;
        let mut n = from;
        while start.elapsed() < span {
            let t = Instant::now();
            link.push(frame(n), Effect::Sudden, 0);
            worst = worst.max(t.elapsed());
            n = n.wrapping_add(1).max(1);
            std::thread::sleep(ms(10));
        }
        worst
    }

    #[test]
    fn a_dropped_bulb_comes_back_on_the_current_frame() {
        let changes = Arc::new(AtomicUsize::new(0));
        let (link, bulb) = rig(Duration::ZERO);
        let mut link = link.on_change(counter(&changes));
        link.push(frame(10), Effect::Sudden, 0);
        assert!(link.is_up());

        bulb.down.store(true, Ordering::SeqCst);
        link.push(frame(11), Effect::Sudden, 0);
        assert!(!link.is_up(), "the failed push takes the link down");
        assert!(!link.up_flag().load(Ordering::SeqCst));
        // Before the first wait is over, nothing is tried.
        play(&mut link, 20, ms(300));
        assert_eq!(bulb.opens.load(Ordering::SeqCst), 0);

        bulb.down.store(false, Ordering::SeqCst);
        play(&mut link, 30, ms(800));
        assert!(link.is_up(), "back once the new stream has held");
        assert_eq!(bulb.opens.load(Ordering::SeqCst), 1);
        assert_eq!(changes.load(Ordering::SeqCst), 2, "down, then up");
        let log = bulb.log.lock().unwrap();
        assert_eq!(log[0], 10);
        assert!(
            log[1] >= 30,
            "the new stream starts on a current frame: {log:?}"
        );
    }

    #[test]
    fn a_stream_that_opens_but_never_holds_does_not_flap() {
        // A Hue bridge unplugged, or one whose pin is refused: every open
        // "succeeds", every frame fails.
        let changes = Arc::new(AtomicUsize::new(0));
        let (link, bulb) = rig(Duration::ZERO);
        let mut link = link.on_change(counter(&changes));
        bulb.broken.store(true, Ordering::SeqCst);
        play(&mut link, 1, ms(3300));
        assert!(!link.is_up());
        assert_eq!(changes.load(Ordering::SeqCst), 1, "down once, never up");
        // Waits of ~0.5, ~1 and ~2 s, as if each open had failed: never
        // back to the shortest one.
        let made = bulb.opens.load(Ordering::SeqCst);
        assert!((2..=3).contains(&made), "{made} attempts in 3.3 s");
    }

    #[test]
    fn a_stream_back_for_good_resets_the_backoff() {
        let (mut link, bulb) = rig(Duration::ZERO);
        bulb.broken.store(true, Ordering::SeqCst);
        // Two failed trials: the next wait would be ~2 s.
        play(&mut link, 1, ms(1600));
        bulb.broken.store(false, Ordering::SeqCst);
        play(&mut link, 1, ms(2600));
        assert!(link.is_up());
        // Dropping again starts over from the shortest wait.
        bulb.broken.store(true, Ordering::SeqCst);
        let opens = bulb.opens.load(Ordering::SeqCst);
        play(&mut link, 1, ms(600));
        assert_eq!(bulb.opens.load(Ordering::SeqCst), opens + 1);
    }

    #[test]
    fn a_slow_handshake_never_holds_the_frames_up() {
        let (mut link, bulb) = rig(ms(400));
        bulb.down.store(true, Ordering::SeqCst);
        let worst = play(&mut link, 1, ms(1500));
        assert!(
            bulb.opens.load(Ordering::SeqCst) >= 1,
            "an attempt was made"
        );
        assert!(worst < ms(50), "a push waited {worst:?}");
        assert!(!link.is_up());
    }

    #[test]
    fn failed_attempts_space_out() {
        let (mut link, bulb) = rig(Duration::ZERO);
        bulb.down.store(true, Ordering::SeqCst);
        // Waits of ~0.5, ~1 and ~2 s: three attempts in 3.6 s at most, and
        // never one per frame.
        play(&mut link, 1, ms(2200));
        let made = bulb.opens.load(Ordering::SeqCst);
        assert!((1..=2).contains(&made), "{made} attempts in 2.2 s");
    }

    #[test]
    fn a_link_at_rest_stops_trying() {
        let (mut link, bulb) = rig(Duration::ZERO);
        bulb.down.store(true, Ordering::SeqCst);
        link.push(frame(1), Effect::Sudden, 0);
        link.rest();
        play(&mut link, 1, ms(800));
        assert_eq!(bulb.opens.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_attempt_that_ends_after_a_rest_never_reaches_the_bulb() {
        // A Tuya handshake takes seconds; "all off" may come meanwhile.
        let (mut link, bulb) = rig(ms(300));
        bulb.down.store(true, Ordering::SeqCst);
        link.push(frame(1), Effect::Sudden, 0);
        bulb.down.store(false, Ordering::SeqCst);
        // The attempt starts after the first wait, then takes 300 ms.
        play(&mut link, 1, ms(600));
        assert_eq!(bulb.opens.load(Ordering::SeqCst), 1, "under way");
        let dropped = bulb.dropped.load(Ordering::SeqCst);
        link.rest();
        std::thread::sleep(ms(500));
        play(&mut link, 50, ms(100));
        assert!(!link.is_up());
        assert_eq!(
            bulb.dropped.load(Ordering::SeqCst),
            dropped + 1,
            "the late stream is closed"
        );
        assert!(
            bulb.log.lock().unwrap().is_empty(),
            "and never sent a frame"
        );
    }
}
