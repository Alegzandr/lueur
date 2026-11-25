//! Sharing the bulbs with other computers that run Lueur.
//!
//! Two computers playing an effect on the same bulb would fight over it: a
//! Yeelight has one music channel, which each would take back from the
//! other in turn, and every other family would show both streams at once,
//! flickering. So each instance says on the network which bulbs it drives
//! (a [`Claim`] each, in an [`Announce`] broadcast every second while it
//! drives any), hears what the others drive, and leaves alone a bulb that
//! someone else holds: its light waits, and picks up again once the other
//! lets go. Only what the user does takes a bulb from another computer:
//! starting an effect on it ([`Leases::take`]) or setting it by hand
//! ([`Leases::set`]). Nothing Lueur starts by itself ever does.
//!
//! A bulb set by hand is claimed with what it was set to (a [`Set`]), so
//! the other computers do more than let go: they show the same state, and
//! the effect they played on it stops there ([`Leases::heard_sets`]).
//!
//! Who holds a bulb is decided the same way on every computer, from what
//! the claims say: the highest `round` wins, then the oldest `since`, then
//! the smallest instance id. A claim is made at `round` 0; taking a bulb
//! claims it one above whoever holds it. Both numbers travel as the
//! claimer wrote them, so clocks that disagree only change who wins a tie,
//! never whether the two computers agree on it.
//!
//! An instance that falls silent (quit, crashed, asleep) loses its claims
//! after [`EXPIRY`]. One that wakes from sleep drops its own and listens
//! for [`LISTEN`] before claiming again, so it never takes back with an
//! old claim a bulb another computer picked up meanwhile.
//!
//! Nothing here is secret or proved: anything on the network can claim a
//! bulb. The worst it can do is make Lueur wait, or stop an effect on a
//! light and change what the window says it was set to, which the user's
//! next change overrides; it never gets to drive a bulb through Lueur.

use crate::scene::LightState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The version of the announcements this module speaks.
pub const PROTOCOL: u32 = 1;
/// How often an instance that drives bulbs says so.
pub const HEARTBEAT: Duration = Duration::from_secs(1);
/// How long a silent instance's claims stand: a few heartbeats missed.
pub const EXPIRY: Duration = Duration::from_millis(3500);
/// How long an instance listens before its first claim, at launch and
/// after a sleep: longer than a heartbeat, so it has heard everyone.
pub const LISTEN: Duration = Duration::from_millis(1200);
/// How long a released claim still stands: the effect that replaces the
/// one that let go claims the bulb again meanwhile, rather than losing it to
/// a computer that was waiting.
pub const GRACE: Duration = Duration::from_millis(1500);
/// How long a claim nothing drives stands: a loop that ended without
/// releasing, a bulb taken and never driven.
pub const IDLE: Duration = Duration::from_secs(5);
/// A gap this long between two ticks means the computer slept.
pub const WAKE_GAP: Duration = Duration::from_secs(3);
/// The largest announcement read; anything longer is dropped unread.
pub const MAX_DATAGRAM: usize = 8192;
/// The most claims an announcement may carry, and the most instances heard.
const MAX_CLAIMS: usize = 128;
const MAX_PEERS: usize = 32;
/// The longest computer name shown, in characters.
const MAX_HOST: usize = 48;
/// The highest `round` a claim may carry. Each *Take over* adds one, so no
/// house gets near it; a claim above it is forged, sent to sit where no
/// other computer's next round could ever outrank it.
const MAX_ROUND: u32 = 1 << 20;

/// One bulb an instance drives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// The bulb's device id, the same on every computer (it comes from the
    /// bulb: its MAC, its serial, the bridge's id).
    pub device: String,
    /// Raised by each take.
    pub round: u32,
    /// When the claim was made, in milliseconds since 1970 by the
    /// claimer's clock.
    pub since: u64,
    /// What the claimer's user set the bulb to by hand, when that is why
    /// it holds it; none while an effect plays on it. A version from
    /// before this field reads the claim as an effect's, and waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<Set>,
}

/// What a bulb was set to by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Set {
    pub state: LightState,
    /// Its light follows the natural light, and this is where the day has
    /// brought it.
    #[serde(default)]
    pub natural: bool,
}

/// What an instance broadcasts: who it is, and the bulbs it drives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announce {
    /// [`PROTOCOL`].
    pub lueur: u32,
    /// The instance, drawn at random at each launch.
    pub id: String,
    /// The computer's name, to tell the user who holds a light.
    pub host: String,
    #[serde(default)]
    pub claims: Vec<Claim>,
}

impl Announce {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Read an announcement heard on the network. Anything malformed, too
    /// large or of another version is `None`; the name is cut to what the
    /// interface can show.
    pub fn decode(bytes: &[u8]) -> Option<Announce> {
        if bytes.len() > MAX_DATAGRAM {
            return None;
        }
        let mut a: Announce = serde_json::from_slice(bytes).ok()?;
        let fits = |s: &str, max: usize| !s.is_empty() && s.len() <= max;
        if a.lueur != PROTOCOL || !fits(&a.id, 64) || a.claims.len() > MAX_CLAIMS {
            return None;
        }
        if !a
            .claims
            .iter()
            .all(|c| fits(&c.device, 128) && c.round <= MAX_ROUND)
        {
            return None;
        }
        a.host = clean_host(&a.host);
        // Only what a bulb accepts is ever recorded from the network.
        for set in a.claims.iter_mut().filter_map(|c| c.set.as_mut()) {
            set.state = set.state.clamped();
        }
        Some(a)
    }
}

/// A computer name fit to show: printable characters only, trimmed, cut
/// to [`MAX_HOST`]. Invisible formatting characters go too, so a name cannot
/// turn the text around it backwards.
pub fn clean_host(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && !is_format(*c))
        .take(MAX_HOST)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Whether `c` is one of the invisible formatting characters that matter in
/// a name: direction marks and overrides, zero-width characters, the byte
/// order mark.
fn is_format(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
    )
}

/// Something else drives the bulb, so a stream that asked politely was not
/// opened (see [`is_taken`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Taken;

impl std::fmt::Display for Taken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("something else drives the bulb")
    }
}

impl std::error::Error for Taken {}

/// The error a polite open fails with when something else drives the bulb.
pub fn taken() -> io::Error {
    io::Error::new(io::ErrorKind::ResourceBusy, Taken)
}

/// Whether `e` says something else drives the bulb.
pub fn is_taken(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<Taken>())
}

/// Whether claim `a` of instance `a_id` wins over claim `b` of `b_id`.
fn outranks(a: &Claim, a_id: &str, b: &Claim, b_id: &str) -> bool {
    (a.round, std::cmp::Reverse(a.since), std::cmp::Reverse(a_id))
        > (b.round, std::cmp::Reverse(b.since), std::cmp::Reverse(b_id))
}

struct Peer {
    host: String,
    claims: Vec<Claim>,
    heard: Instant,
}

/// One of our own claims.
struct Mine {
    claim: Claim,
    /// When a light last drove the bulb.
    touched: Instant,
    /// When it let go, if it did: the claim stands for [`GRACE`] more.
    released: Option<Instant>,
}

/// Who holds what, as this instance knows it. Every method takes the time,
/// so the rules are tested without waiting.
struct Table {
    me: String,
    host: String,
    peers: HashMap<String, Peer>,
    mine: HashMap<String, Mine>,
    /// Bulbs the user took, whose next stream goes ahead whoever streams.
    forced: HashSet<String>,
    /// The hand-set claim last followed on each bulb, and whose it is, so
    /// its heartbeats are not followed again.
    followed: HashMap<String, (String, Claim)>,
    /// What other computers set by hand since [`Leases::heard_sets`] was
    /// last asked.
    sets: Vec<(String, Set)>,
    /// No claim is made before this: the instance is still listening.
    quiet_until: Instant,
    last_tick: Instant,
    last_sent: Option<Instant>,
    /// Our claims changed since they were last sent.
    dirty: bool,
    /// Bumped whenever what the others hold changes.
    version: u64,
}

impl Table {
    fn new(me: String, host: String, listen: Duration, now: Instant) -> Self {
        Table {
            me,
            host: clean_host(&host),
            peers: HashMap::new(),
            mine: HashMap::new(),
            forced: HashSet::new(),
            followed: HashMap::new(),
            sets: Vec::new(),
            quiet_until: now + listen,
            last_tick: now,
            last_sent: None,
            dirty: false,
            version: 0,
        }
    }

    /// The strongest claim on `device` among the instances still heard.
    fn rival(&self, device: &str, now: Instant) -> Option<(&str, &Peer, &Claim)> {
        let mut best: Option<(&str, &Peer, &Claim)> = None;
        for (id, peer) in &self.peers {
            if now.saturating_duration_since(peer.heard) > EXPIRY {
                continue;
            }
            for c in peer.claims.iter().filter(|c| c.device == device) {
                if best.is_none_or(|(bid, _, bc)| outranks(c, id, bc, bid)) {
                    best = Some((id, peer, c));
                }
            }
        }
        best
    }

    /// Whether an effect of this instance may drive `device` now, claiming
    /// it if nobody holds it. A claim another one outranks is dropped.
    fn drive(&mut self, device: &str, now: Instant, clock: u64) -> bool {
        let rival = self
            .rival(device, now)
            .map(|(id, _, c)| (id.to_string(), c.clone()));
        match self.mine.get_mut(device) {
            Some(m) => {
                if rival.is_none_or(|(id, c)| outranks(&m.claim, &self.me, &c, &id)) {
                    m.touched = now;
                    // An effect plays on it now, whatever it was set to.
                    let was_set = m.claim.set.take().is_some();
                    if m.released.take().is_some() || was_set {
                        self.dirty = true;
                    }
                    true
                } else {
                    self.mine.remove(device);
                    self.forced.remove(device);
                    self.dirty = true;
                    false
                }
            }
            None if rival.is_some() || now < self.quiet_until => false,
            None => {
                let claim = Claim {
                    device: device.to_string(),
                    round: 0,
                    since: clock,
                    set: None,
                };
                self.mine.insert(
                    device.to_string(),
                    Mine {
                        claim,
                        touched: now,
                        released: None,
                    },
                );
                self.dirty = true;
                true
            }
        }
    }

    /// Whether the claim this instance made on `device` by hand still
    /// holds, keeping it alive if so. One that another outranks is dropped.
    fn keep(&mut self, device: &str, now: Instant) -> bool {
        let rival = self
            .rival(device, now)
            .map(|(id, _, c)| (id.to_string(), c.clone()));
        let Some(m) = self.mine.get_mut(device) else {
            return false;
        };
        if rival.is_none_or(|(id, c)| outranks(&m.claim, &self.me, &c, &id)) {
            m.touched = now;
            m.released = None;
            return true;
        }
        self.mine.remove(device);
        self.forced.remove(device);
        self.dirty = true;
        false
    }

    /// Claim `device` for what the user just did: above whoever holds it,
    /// or as it is already claimed when this instance holds it. `set` is
    /// what the user set it to by hand; none for an effect, whose next
    /// stream then goes ahead whoever streams.
    fn take(&mut self, device: &str, set: Option<Set>, now: Instant, clock: u64) {
        let rival = self
            .rival(device, now)
            .map(|(id, _, c)| (id.to_string(), c.clone()));
        let held = match self.mine.get_mut(device) {
            Some(m)
                if rival
                    .as_ref()
                    .is_none_or(|(id, c)| outranks(&m.claim, &self.me, c, id)) =>
            {
                m.touched = now;
                m.released = None;
                if m.claim.set != set {
                    m.claim.set = set;
                    self.dirty = true;
                }
                true
            }
            _ => false,
        };
        if !held {
            let round = rival.map_or(0, |(_, c)| c.round.saturating_add(1).min(MAX_ROUND));
            self.mine.insert(
                device.to_string(),
                Mine {
                    claim: Claim {
                        device: device.to_string(),
                        round,
                        since: clock,
                        set,
                    },
                    touched: now,
                    released: None,
                },
            );
            self.dirty = true;
        }
        if set.is_none() {
            self.forced.insert(device.to_string());
        } else {
            self.forced.remove(device);
        }
    }

    fn release(&mut self, device: &str, now: Instant) {
        if let Some(m) = self.mine.get_mut(device) {
            m.released.get_or_insert(now);
        }
        self.forced.remove(device);
    }

    /// The claim that holds `device`, when it is not ours: the one that
    /// outranks ours, or that stands while we claim nothing.
    fn held(&self, device: &str, now: Instant) -> Option<(&str, &Peer, &Claim)> {
        let (id, peer, claim) = self.rival(device, now)?;
        match self.mine.get(device) {
            Some(m) if outranks(&m.claim, &self.me, claim, id) => None,
            _ => Some((id, peer, claim)),
        }
    }

    /// The name of the computer that holds `device`, when it is not us.
    fn holder(&self, device: &str, now: Instant) -> Option<String> {
        self.held(device, now).map(|(_, peer, _)| peer.host.clone())
    }

    /// The name of the computer that plays an effect on `device`, when it
    /// is not us. One that only set it by hand is not named.
    fn player(&self, device: &str, now: Instant) -> Option<String> {
        self.held(device, now)
            .filter(|(_, _, claim)| claim.set.is_none())
            .map(|(_, peer, _)| peer.host.clone())
    }

    /// Note what `devices` were set to by hand on the computer that holds
    /// them, once for each change.
    fn follow(&mut self, devices: Vec<String>, now: Instant) {
        for device in devices {
            let Some((id, _, claim)) = self.held(&device, now) else {
                continue;
            };
            let Some(set) = claim.set else {
                continue;
            };
            let seen = (id.to_string(), claim.clone());
            if self.followed.get(&device) != Some(&seen) {
                self.followed.insert(device.clone(), seen);
                self.sets.push((device, set));
            }
        }
    }

    fn hear(&mut self, a: Announce, now: Instant) {
        if a.id == self.me {
            return;
        }
        // A full table makes room only by forgetting who fell silent: a flood
        // of new names must not push out a computer still driving a bulb,
        // which would then share it with us.
        if !self.peers.contains_key(&a.id) && self.peers.len() >= MAX_PEERS {
            let before = self.peers.len();
            self.peers
                .retain(|_, p| now.saturating_duration_since(p.heard) <= EXPIRY);
            if self.peers.len() != before {
                self.version += 1;
            }
            if self.peers.len() >= MAX_PEERS {
                return;
            }
        }
        let same = self
            .peers
            .get(&a.id)
            .is_some_and(|p| p.claims == a.claims && p.host == a.host);
        if !same {
            self.version += 1;
        }
        let set: Vec<String> = a
            .claims
            .iter()
            .filter(|c| c.set.is_some())
            .map(|c| c.device.clone())
            .collect();
        self.peers.insert(
            a.id,
            Peer {
                host: a.host,
                claims: a.claims,
                heard: now,
            },
        );
        self.follow(set, now);
    }

    /// Forget who fell silent, and our claims nobody drives any more.
    fn tick(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_tick) > WAKE_GAP {
            // Asleep: what we held may be someone else's by now.
            self.mine.clear();
            self.forced.clear();
            self.quiet_until = now + LISTEN;
            self.dirty = true;
        }
        self.last_tick = now;
        let before = self.peers.len();
        self.peers
            .retain(|_, p| now.saturating_duration_since(p.heard) <= EXPIRY);
        if self.peers.len() != before {
            self.version += 1;
        }
        // A claim that is gone will not be heard again: nothing to tell
        // apart from, and a flood of bulb names does not pile up here.
        let peers = &self.peers;
        self.followed
            .retain(|_, (id, claim)| peers.get(id).is_some_and(|p| p.claims.contains(claim)));
        let before = self.mine.len();
        self.mine.retain(|_, m| {
            let let_go = m
                .released
                .is_some_and(|r| now.saturating_duration_since(r) > GRACE);
            !let_go && now.saturating_duration_since(m.touched) <= IDLE
        });
        if self.mine.len() != before {
            self.forced.retain(|d| self.mine.contains_key(d));
            self.dirty = true;
        }
    }

    /// What to broadcast now, if anything: our claims when they changed,
    /// and every [`HEARTBEAT`] while there are any.
    fn outgoing(&mut self, now: Instant) -> Option<Announce> {
        let due = self.dirty
            || (!self.mine.is_empty()
                && self
                    .last_sent
                    .is_none_or(|t| now.saturating_duration_since(t) >= HEARTBEAT));
        if !due {
            return None;
        }
        self.dirty = false;
        self.last_sent = Some(now);
        let mut claims: Vec<Claim> = self.mine.values().map(|m| m.claim.clone()).collect();
        claims.sort_by(|a, b| a.device.cmp(&b.device));
        Some(Announce {
            lueur: PROTOCOL,
            id: self.me.clone(),
            host: self.host.clone(),
            claims,
        })
    }
}

/// The milliseconds since 1970 by this computer's clock.
fn clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Who holds which bulb, shared by every light's [`Gate`] and the thread
/// that talks to the other instances.
pub struct Leases(Mutex<Table>);

impl Leases {
    /// `me` names this instance (drawn at random at each launch), `host`
    /// the computer. Nothing is claimed for `listen`.
    pub fn new(me: impl Into<String>, host: impl Into<String>, listen: Duration) -> Self {
        Leases(Mutex::new(Table::new(
            me.into(),
            host.into(),
            listen,
            Instant::now(),
        )))
    }

    fn lock(&self) -> MutexGuard<'_, Table> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The gate a light that drives `device` goes through.
    pub fn gate(self: &Arc<Self>, device: &str) -> Gate {
        Gate {
            leases: Arc::clone(self),
            device: device.to_string(),
        }
    }

    /// Take `device` for an effect the user starts on it, from whoever
    /// holds it.
    pub fn take(&self, device: &str) {
        let mut t = self.lock();
        let now = Instant::now();
        if t.held(device, now).is_some() {
            log::info!(device = device; "taking a bulb from another computer");
        }
        t.take(device, None, now, clock());
    }

    /// Take `device` because the user set it by hand to `set`, from whoever
    /// holds it, and tell the other computers what it shows now. The claim
    /// stands while a [`Gate`] keeps it ([`Gate::holds`]), or for [`IDLE`].
    pub fn set(&self, device: &str, set: Set) {
        let mut t = self.lock();
        let now = Instant::now();
        if t.held(device, now).is_some() {
            log::info!(device = device; "taking a bulb from another computer");
        }
        t.take(device, Some(set), now, clock());
    }

    /// The name of the computer that holds `device`, when it is not this
    /// one.
    pub fn holder(&self, device: &str) -> Option<String> {
        self.lock().holder(device, Instant::now())
    }

    /// What this instance last told the others it set `device` to by hand,
    /// while that claim stands and nothing plays on the bulb here since.
    pub fn own_set(&self, device: &str) -> Option<Set> {
        let t = self.lock();
        let m = t.mine.get(device)?;
        m.released.is_none().then_some(m.claim.set).flatten()
    }

    /// The name of the computer that plays an effect on `device`, when it
    /// is not this one.
    pub fn player(&self, device: &str) -> Option<String> {
        self.lock().player(device, Instant::now())
    }

    /// The bulbs other computers set by hand since last asked, each with
    /// what it was set to, in the order heard.
    pub fn heard_sets(&self) -> Vec<(String, Set)> {
        std::mem::take(&mut self.lock().sets)
    }

    /// Take in what another instance says.
    pub fn hear(&self, a: Announce) {
        self.lock().hear(a, Instant::now());
    }

    /// Forget who fell silent, and the claims nothing drives; called a few
    /// times a second.
    pub fn tick(&self) {
        self.lock().tick(Instant::now());
    }

    /// What to broadcast now, if anything.
    pub fn outgoing(&self) -> Option<Announce> {
        self.lock().outgoing(Instant::now())
    }

    /// Let go of every bulb at once, without the grace: the app quits.
    pub fn leave(&self) {
        let mut t = self.lock();
        if !t.mine.is_empty() {
            t.mine.clear();
            t.forced.clear();
            t.dirty = true;
        }
    }

    /// Changes whenever what the other instances hold does.
    pub fn version(&self) -> u64 {
        self.lock().version
    }
}

/// One light's way to its bulb, through the leases.
pub struct Gate {
    leases: Arc<Leases>,
    device: String,
}

impl Gate {
    /// Whether the light may drive its bulb now. It claims the bulb if
    /// nobody holds it; asked every frame, which keeps the claim alive.
    pub fn may_drive(&self) -> bool {
        self.leases
            .lock()
            .drive(&self.device, Instant::now(), clock())
    }

    /// Whether the claim made on the bulb when the user set it by hand
    /// ([`Leases::set`]) still holds. Asked while the light is held by
    /// hand, which keeps the claim alive.
    pub fn holds(&self) -> bool {
        self.leases.lock().keep(&self.device, Instant::now())
    }

    /// Whether no other computer holds the bulb.
    pub fn is_free(&self) -> bool {
        self.leases.holder(&self.device).is_none()
    }

    /// Whether the user took the bulb since last asked: its next stream
    /// goes ahead even if something else streams to it.
    pub fn take_forced(&self) -> bool {
        self.leases.lock().forced.remove(&self.device)
    }

    /// The light is done with the bulb. Its claim stands for [`GRACE`], in
    /// case another light of ours takes it straight back.
    pub fn release(&self) {
        self.leases.lock().release(&self.device, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAMP: &str = "0x00000000123";

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A table that has listened already.
    fn table(me: &str, t0: Instant) -> Table {
        Table::new(me.into(), format!("PC-{me}"), Duration::ZERO, t0)
    }

    /// Tick `t` from `from` to `to`, a tenth of a second apart, as the
    /// network thread does (a longer gap reads as a sleep).
    fn run(t: &mut Table, from: Instant, to: Instant) {
        let mut now = from;
        while now < to {
            now = (now + ms(100)).min(to);
            t.tick(now);
        }
    }

    /// Let `from` hear what `to` broadcasts, at `now`.
    fn relay(from: &mut Table, to: &mut Table, now: Instant) {
        if let Some(a) = from.outgoing(now) {
            let a = Announce::decode(&a.encode()).expect("round trip");
            to.hear(a, now);
        }
    }

    #[test]
    fn an_announcement_survives_the_wire_and_junk_does_not() {
        let a = Announce {
            lueur: PROTOCOL,
            id: "a".into(),
            host: "  Salon\u{7}  ".into(),
            claims: vec![Claim {
                device: LAMP.into(),
                round: 2,
                since: 17,
                set: None,
            }],
        };
        let back = Announce::decode(&a.encode()).unwrap();
        assert_eq!(back.host, "Salon");
        assert_eq!(back.claims, a.claims);
        assert!(Announce::decode(b"not json").is_none());
        let future = Announce {
            lueur: 2,
            ..a.clone()
        };
        assert!(Announce::decode(&future.encode()).is_none());
        let nameless = Announce {
            id: String::new(),
            ..a.clone()
        };
        assert!(Announce::decode(&nameless.encode()).is_none());
        let flood = Announce {
            claims: vec![a.claims[0].clone(); MAX_CLAIMS + 1],
            ..a.clone()
        };
        assert!(Announce::decode(&flood.encode()).is_none());
        assert!(Announce::decode(&vec![b' '; MAX_DATAGRAM + 1]).is_none());
        assert_eq!(clean_host(&"x".repeat(200)).chars().count(), MAX_HOST);
        assert_eq!(clean_host("Salon\u{202E}gnp.exe\u{200B}"), "Salongnp.exe");
        // A round no take-over reaches sits out of everyone's reach: forged.
        let mut stuck = a.clone();
        stuck.claims[0].round = MAX_ROUND + 1;
        assert!(Announce::decode(&stuck.encode()).is_none());
        stuck.claims[0].round = MAX_ROUND;
        assert!(Announce::decode(&stuck.encode()).is_some());
    }

    #[test]
    fn the_first_to_claim_keeps_the_bulb_and_the_second_waits() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 1000));
        relay(&mut a, &mut b, t0);
        assert!(!b.drive(LAMP, t0, 2000));
        assert_eq!(b.holder(LAMP, t0).as_deref(), Some("PC-a"));
        assert_eq!(a.holder(LAMP, t0), None);
        // Waiting claims nothing: a has no rival to hear of.
        assert!(b.outgoing(t0).is_none());
    }

    #[test]
    fn two_that_claim_at_once_agree_on_who_keeps_it() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        // Neither heard the other: both drive for a moment.
        assert!(a.drive(LAMP, t0, 5000));
        assert!(b.drive(LAMP, t0, 4000));
        relay(&mut a, &mut b, t0);
        relay(&mut b, &mut a, t0);
        // b claimed earlier by its own clock: both say so.
        assert!(!a.drive(LAMP, t0, 5001));
        assert!(b.drive(LAMP, t0, 4001));
        // The same instant on both clocks: the smaller id keeps it.
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 7));
        assert!(b.drive(LAMP, t0, 7));
        relay(&mut a, &mut b, t0);
        relay(&mut b, &mut a, t0);
        assert!(a.drive(LAMP, t0, 8));
        assert!(!b.drive(LAMP, t0, 8));
    }

    #[test]
    fn a_take_wins_and_the_one_it_was_taken_from_waits() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 1000));
        relay(&mut a, &mut b, t0);
        assert!(!b.drive(LAMP, t0, 2000));
        b.take(LAMP, None, t0, 3000);
        assert!(b.drive(LAMP, t0, 3001));
        assert!(b.forced.remove(LAMP), "its next stream goes ahead");
        relay(&mut b, &mut a, t0);
        assert!(!a.drive(LAMP, t0, 3002));
        assert_eq!(a.holder(LAMP, t0).as_deref(), Some("PC-b"));
        // a's claim is gone, and it says so.
        let said = a.outgoing(t0).expect("a announces it let go");
        assert!(said.claims.is_empty());
        // Taking it back raises the stakes again.
        relay(&mut b, &mut a, t0 + ms(10));
        a.take(LAMP, None, t0 + ms(10), 4000);
        relay(&mut a, &mut b, t0 + ms(10));
        assert!(a.drive(LAMP, t0 + ms(10), 4001));
        assert!(!b.drive(LAMP, t0 + ms(10), 4001));
    }

    fn white(bright: u8) -> Set {
        Set {
            state: LightState {
                bright,
                ..LightState::default()
            },
            natural: false,
        }
    }

    #[test]
    fn what_a_bulb_was_set_to_survives_the_wire_within_what_bulbs_accept() {
        let mut a = Announce {
            lueur: PROTOCOL,
            id: "a".into(),
            host: "Salon".into(),
            claims: vec![Claim {
                device: LAMP.into(),
                round: 0,
                since: 1,
                set: Some(Set {
                    natural: true,
                    ..white(40)
                }),
            }],
        };
        let back = Announce::decode(&a.encode()).unwrap();
        assert_eq!(back.claims, a.claims);
        a.claims[0].set = Some(white(250));
        let back = Announce::decode(&a.encode()).unwrap();
        assert_eq!(back.claims[0].set.unwrap().state.bright, 100);
        // An effect's claim reads as it did before bulbs set by hand were
        // told, and is written the same.
        a.claims[0].set = None;
        assert!(!String::from_utf8(a.encode()).unwrap().contains("set"));
        let old =
            br#"{"lueur":1,"id":"a","host":"Salon","claims":[{"device":"x","round":0,"since":1}]}"#;
        assert_eq!(Announce::decode(old).unwrap().claims[0].set, None);
    }

    #[test]
    fn a_bulb_set_by_hand_is_taken_from_the_effect_playing_on_it() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 1000));
        relay(&mut a, &mut b, t0);
        assert_eq!(b.player(LAMP, t0).as_deref(), Some("PC-a"));
        // b's user sets the lamp: no waiting, no button.
        b.take(LAMP, Some(white(40)), t0, 2000);
        assert!(b.keep(LAMP, t0));
        assert!(!b.forced.contains(LAMP));
        relay(&mut b, &mut a, t0);
        assert!(!a.drive(LAMP, t0, 2001), "a's effect lets go");
        assert_eq!(a.holder(LAMP, t0).as_deref(), Some("PC-b"));
        assert_eq!(a.player(LAMP, t0), None, "b plays nothing on it");
        assert_eq!(std::mem::take(&mut a.sets), vec![(LAMP.into(), white(40))]);
    }

    #[test]
    fn a_hand_set_is_followed_once_and_again_when_it_changes() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        b.take(LAMP, Some(white(40)), t0, 1000);
        relay(&mut b, &mut a, t0);
        assert_eq!(a.sets.len(), 1);
        // Its heartbeats say nothing new.
        b.keep(LAMP, t0 + ms(900));
        relay(&mut b, &mut a, t0 + ms(1000));
        assert_eq!(a.sets.len(), 1);
        // The slider moves on: the same claim, another state.
        b.take(LAMP, Some(white(60)), t0 + ms(1100), 2100);
        assert_eq!(b.mine[LAMP].claim.round, 0, "holding it, nothing to outbid");
        relay(&mut b, &mut a, t0 + ms(1100));
        assert_eq!(
            std::mem::take(&mut a.sets),
            vec![(LAMP.into(), white(40)), (LAMP.into(), white(60))]
        );
        // Once the claim is gone, nothing is kept of it.
        b.release(LAMP, t0 + ms(1200));
        let t = t0 + ms(1200) + GRACE + ms(100);
        b.tick(t);
        relay(&mut b, &mut a, t);
        a.tick(t);
        assert!(a.followed.is_empty());
        assert!(a.sets.is_empty());
    }

    #[test]
    fn the_last_to_set_a_bulb_by_hand_has_it() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        a.take(LAMP, Some(white(20)), t0, 1000);
        relay(&mut a, &mut b, t0);
        b.sets.clear();
        b.take(LAMP, Some(white(80)), t0 + ms(500), 1500);
        relay(&mut b, &mut a, t0 + ms(500));
        assert!(!a.keep(LAMP, t0 + ms(500)), "a's hold on it ends");
        assert!(b.keep(LAMP, t0 + ms(500)));
        assert_eq!(a.sets, vec![(LAMP.into(), white(80))]);
        // a's stale word, heard late, changes nothing for b.
        relay(&mut a, &mut b, t0 + ms(500));
        assert!(b.sets.is_empty());
        assert!(b.keep(LAMP, t0 + ms(600)));
    }

    #[test]
    fn what_this_computer_set_by_hand_stands_until_something_else_plays() {
        let leases = std::sync::Arc::new(Leases::new("a", "PC-a", Duration::ZERO));
        assert_eq!(leases.own_set(LAMP), None);
        let off = Set {
            state: LightState {
                on: false,
                ..LightState::default()
            },
            natural: false,
        };
        leases.set(LAMP, off);
        assert_eq!(leases.own_set(LAMP), Some(off));
        leases.set(LAMP, white(30));
        assert_eq!(leases.own_set(LAMP), Some(white(30)));
        // An effect starts on it: nothing set by hand any more.
        assert!(leases.gate(LAMP).may_drive());
        assert_eq!(leases.own_set(LAMP), None);
        leases.set(LAMP, off);
        leases.gate(LAMP).release();
        assert_eq!(leases.own_set(LAMP), None, "let go");
    }

    #[test]
    fn an_effect_started_on_a_bulb_set_by_hand_says_so() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        a.take(LAMP, Some(white(20)), t0, 1000);
        relay(&mut a, &mut b, t0);
        assert_eq!(b.player(LAMP, t0), None);
        assert!(a.drive(LAMP, t0 + ms(100), 1100));
        relay(&mut a, &mut b, t0 + ms(100));
        assert_eq!(b.player(LAMP, t0 + ms(100)).as_deref(), Some("PC-a"));
    }

    #[test]
    fn a_bulb_let_go_passes_to_the_one_that_waits() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 1000));
        relay(&mut a, &mut b, t0);
        a.release(LAMP, t0);
        // Within the grace, the claim stands: another effect of a's may
        // take the bulb straight back.
        a.tick(t0 + ms(500));
        relay(&mut a, &mut b, t0 + ms(1000));
        assert!(!b.drive(LAMP, t0 + ms(1000), 2000));
        let t = t0 + GRACE + ms(100);
        a.tick(t);
        relay(&mut a, &mut b, t);
        assert!(b.drive(LAMP, t, 3000));
        assert_eq!(b.holder(LAMP, t), None);
    }

    #[test]
    fn a_light_back_within_the_grace_keeps_its_claim() {
        let t0 = Instant::now();
        let mut a = table("a", t0);
        assert!(a.drive(LAMP, t0, 1000));
        a.release(LAMP, t0);
        assert!(a.drive(LAMP, t0 + ms(800), 1800));
        a.tick(t0 + GRACE + ms(500));
        let claims = a.outgoing(t0 + GRACE + ms(500)).unwrap().claims;
        assert_eq!(claims[0].since, 1000, "the same claim, as old as it was");
    }

    #[test]
    fn a_silent_instance_loses_its_claims() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 1000));
        relay(&mut a, &mut b, t0);
        let v = b.version;
        assert!(!b.drive(LAMP, t0 + ms(3000), 2000));
        let t = t0 + EXPIRY + ms(100);
        run(&mut b, t0, t);
        assert!(b.version > v, "the window hears of it");
        assert!(b.drive(LAMP, t, 4600));
    }

    #[test]
    fn a_claim_nothing_drives_runs_out() {
        let t0 = Instant::now();
        let mut a = table("a", t0);
        a.take(LAMP, None, t0, 1000);
        run(&mut a, t0, t0 + IDLE - ms(100));
        assert!(!a.mine.is_empty());
        run(&mut a, t0 + IDLE - ms(100), t0 + IDLE + ms(100));
        assert!(a.mine.is_empty());
        assert!(a.forced.is_empty());
    }

    #[test]
    fn waking_up_listens_before_claiming_again() {
        let t0 = Instant::now();
        let (mut a, mut b) = (table("a", t0), table("b", t0));
        assert!(a.drive(LAMP, t0, 1000));
        relay(&mut a, &mut b, t0);
        // a sleeps; b picks the bulb up once a falls silent.
        let t1 = t0 + Duration::from_secs(60);
        run(&mut b, t0, t1);
        assert!(b.drive(LAMP, t1, 61_000));
        // a wakes: its old claim would win, so it is dropped, and a waits
        // until it has heard who is there.
        a.tick(t1);
        assert!(!a.drive(LAMP, t1, 61_000));
        relay(&mut b, &mut a, t1 + ms(500));
        assert!(!a.drive(LAMP, t1 + LISTEN + ms(100), 62_300));
        assert_eq!(a.holder(LAMP, t1 + ms(600)).as_deref(), Some("PC-b"));
    }

    #[test]
    fn nothing_is_claimed_while_listening() {
        let t0 = Instant::now();
        let mut a = Table::new("a".into(), "A".into(), LISTEN, t0);
        assert!(!a.drive(LAMP, t0, 0));
        assert!(a.drive(LAMP, t0 + LISTEN, 1200));
    }

    #[test]
    fn heartbeats_go_out_while_driving_and_stop_after() {
        let t0 = Instant::now();
        let mut a = table("a", t0);
        assert!(a.outgoing(t0).is_none(), "idle instances keep quiet");
        a.drive(LAMP, t0, 0);
        assert!(a.outgoing(t0).is_some(), "a new claim goes out at once");
        assert!(a.outgoing(t0 + ms(500)).is_none());
        a.drive(LAMP, t0 + ms(900), 0);
        assert!(a.outgoing(t0 + ms(1000)).is_some());
        a.release(LAMP, t0 + ms(1000));
        a.tick(t0 + ms(1000) + GRACE + ms(1));
        let last = a.outgoing(t0 + ms(2600)).expect("one last word");
        assert!(last.claims.is_empty());
        assert!(a.outgoing(t0 + ms(5000)).is_none());
    }

    #[test]
    fn our_own_echo_is_ignored() {
        let t0 = Instant::now();
        let mut a = table("a", t0);
        a.drive(LAMP, t0, 0);
        let echo = a.outgoing(t0).unwrap();
        a.hear(echo, t0);
        assert!(a.peers.is_empty());
        assert!(a.drive(LAMP, t0, 1));
    }

    #[test]
    fn a_crowd_of_instances_stays_bounded() {
        let t0 = Instant::now();
        let mut a = table("a", t0);
        for i in 0..(MAX_PEERS + 10) {
            a.hear(
                Announce {
                    lueur: PROTOCOL,
                    id: format!("p{i}"),
                    host: String::new(),
                    claims: Vec::new(),
                },
                t0 + ms(i as u64),
            );
        }
        assert_eq!(a.peers.len(), MAX_PEERS);
        // The first ones heard are still there: newcomers did not push them out.
        assert!(a.peers.contains_key("p0"));
        assert!(!a.peers.contains_key(&format!("p{MAX_PEERS}")));
        // Once they fall silent, their places go to whoever speaks next.
        let later = t0 + EXPIRY + ms(100);
        a.hear(
            Announce {
                lueur: PROTOCOL,
                id: "late".into(),
                host: String::new(),
                claims: Vec::new(),
            },
            later,
        );
        assert_eq!(a.peers.len(), 1);
        assert!(a.peers.contains_key("late"));
    }

    #[test]
    fn a_taken_error_is_told_apart() {
        assert!(is_taken(&taken()));
        assert!(!is_taken(&io::Error::from(io::ErrorKind::ResourceBusy)));
        assert!(!is_taken(&io::Error::other("x")));
    }
}
