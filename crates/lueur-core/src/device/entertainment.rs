//! Hue's Entertainment API: every lamp of a bridge, streamed together.
//!
//! The bridge's REST API takes about ten commands a second in all, so its
//! lamps can only glide. Streaming, the bridge takes a frame of every lamp
//! at once, as often as it is sent, and passes it on to them 25 times a
//! second: fast enough for a lamp to punch the beat, and a gradient strip's
//! segments each take a colour of their own.
//!
//! A stream belongs to a bridge, not to a lamp: an entertainment area
//! (Lueur's own, named [`AREA`]) lists the lamps, and one DTLS session
//! sends them all. Each lamp's stream takes a [`Seat`] in its bridge's
//! [`Bridge`], which gathers the lamps asking for one, sets the area up
//! with all of them and starts streaming; the last seat to go stops it.
//!
//! It needs the key the bridge hands out for streaming, which it only gives
//! at pairing ([`super::HueBridge::clientkey`]), and HTTPS for the newer API
//! the area is set up with: a bridge paired before, or the first, round
//! bridge, keeps the REST API.

use super::dtls::DtlsClient;
use super::http;
use super::hue::{self, parse_pin};
use super::{BulbOutput, zone_output};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The bridge's streaming port.
const STREAM_PORT: u16 = 2100;
/// The name of the area Lueur sets up for its lamps.
pub const AREA: &str = "Lueur";
/// How often a frame goes to the bridge. It passes 25 a second on to the
/// lamps; sending twice that keeps a lost datagram from showing.
const SEND_EVERY: Duration = Duration::from_millis(20);
/// The gap between two frames a streamed lamp shows.
pub const INTERVAL: Duration = Duration::from_millis(40);
/// How long a streamed lamp takes to show a frame: up to one of the
/// bridge's own frames, and the Zigbee hop.
pub const LATENCY: Duration = Duration::from_millis(70);
/// How long the first lamp waits for the others before the area is set up:
/// an effect opens all its lamps at once.
const GATHER: Duration = Duration::from_millis(300);
/// How long after streaming failed a bridge is left to its REST API.
const RETRY: Duration = Duration::from_secs(30);
/// How long a lamp at rest waits for the stream to stop before settling.
const SETTLE: Duration = Duration::from_millis(1500);
/// The most channels one frame carries.
const MAX_CHANNELS: usize = 20;
/// The most segments of a gradient lamp streamed apart.
const MAX_SEGMENTS: usize = 7;
const TIMEOUT: Duration = Duration::from_secs(3);

/// How to reach a bridge's streaming: where, as whom, with which key.
#[derive(Clone)]
pub struct Access {
    pub ip: String,
    pub username: String,
    /// The streaming key, hex.
    pub clientkey: String,
    /// The bridge's pinned certificate, hex.
    pub cert: String,
}

impl Access {
    /// Whether this pairing can stream at all.
    pub fn can_stream(&self) -> bool {
        !self.clientkey.is_empty() && parse_pin(&self.cert).is_some()
    }

    fn api(&self, method: &str, path: &str, body: Option<&Value>) -> io::Result<Value> {
        let pin = parse_pin(&self.cert).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "bad Hue certificate pin")
        })?;
        let body = body.map(Value::to_string);
        let resp = http::request_headed(
            pin,
            method,
            &self.ip,
            hue::TLS_PORT,
            path,
            body.as_deref(),
            &[("hue-application-key", hue::safe(&self.username)?)],
            TIMEOUT,
        )?;
        let v: Value = serde_json::from_str(&resp.body)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if !resp.ok() {
            return Err(io::Error::other(format!(
                "bridge answered {}: {}",
                resp.status,
                errors(&v)
            )));
        }
        Ok(v)
    }
}

/// The descriptions in a reply's `errors`.
fn errors(v: &Value) -> String {
    v.get("errors")
        .and_then(Value::as_array)
        .map(|e| {
            e.iter()
                .filter_map(|x| x.get("description")?.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

/// A streamed lamp: the entertainment service that plays it, and how many
/// segments it streams apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub id: String,
    pub segments: usize,
}

/// The lamps of a `/resource/entertainment` listing that can be streamed
/// to, by their REST id (`5` for `/lights/5`).
pub fn services(listing: &Value) -> HashMap<String, Service> {
    let Some(items) = listing.get("data").and_then(Value::as_array) else {
        return HashMap::new();
    };
    items
        .iter()
        .filter(|s| s.get("renderer").and_then(Value::as_bool) == Some(true))
        .filter_map(|s| {
            let light = s.get("id_v1")?.as_str()?.strip_prefix("/lights/")?;
            let segments = s
                .pointer("/segments/max_segments")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, MAX_SEGMENTS as u64) as usize;
            Some((
                light.to_string(),
                Service {
                    id: s.get("id")?.as_str()?.to_string(),
                    segments,
                },
            ))
        })
        .collect()
}

/// The body that sets Lueur's area up with `lamps`, left to right, each
/// as many times as it has `segments` (or once each when `whole`).
pub fn area_body(lamps: &[Service], whole: bool) -> Value {
    let n = lamps.len().max(1) as f64;
    let locations: Vec<Value> = lamps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let x = if n > 1.0 {
                -1.0 + 2.0 * i as f64 / (n - 1.0)
            } else {
                0.0
            };
            let count = if whole { 1 } else { s.segments };
            let positions: Vec<Value> = (0..count)
                .map(|k| {
                    let spread = if count > 1 {
                        0.8 / n * (k as f64 / (count - 1) as f64 - 0.5)
                    } else {
                        0.0
                    };
                    json!({"x": (x + spread).clamp(-1.0, 1.0), "y": 1.0, "z": 0.0})
                })
                .collect();
            json!({
                "service": {"rid": s.id, "rtype": "entertainment"},
                "positions": positions,
            })
        })
        .collect();
    json!({
        "type": "entertainment_configuration",
        "metadata": {"name": AREA},
        "configuration_type": "screen",
        "locations": {"service_locations": locations},
    })
}

/// Lueur's area in a `/resource/entertainment_configuration` listing.
fn our_area(listing: &Value) -> Option<&Value> {
    listing
        .get("data")?
        .as_array()?
        .iter()
        .find(|a| a.pointer("/metadata/name").and_then(Value::as_str) == Some(AREA))
}

/// The entertainment services an area plays.
fn area_services(area: &Value) -> Vec<String> {
    let mut ids: Vec<String> = area
        .pointer("/locations/service_locations")
        .and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(|s| Some(s.pointer("/service/rid")?.as_str()?.to_string()))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// The channels of an area that play the service `id`, in its segments'
/// order.
pub fn channels_of(area: &Value, id: &str) -> Vec<u8> {
    let mut found: Vec<(u64, u8)> =
        area.get("channels")
            .and_then(Value::as_array)
            .map(|channels| {
                channels
                    .iter()
                    .filter_map(|c| {
                        let channel = u8::try_from(c.get("channel_id")?.as_u64()?).ok()?;
                        let member = c.get("members")?.as_array()?.iter().find(|m| {
                            m.pointer("/service/rid").and_then(Value::as_str) == Some(id)
                        })?;
                        Some((
                            member.get("index").and_then(Value::as_u64).unwrap_or(0),
                            channel,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
    found.sort_unstable();
    found.into_iter().map(|(_, c)| c).collect()
}

/// One frame for area `area_id` (its 36-character id): each channel's
/// colour as CIE xy and brightness.
pub fn frame(area_id: &str, sequence: u8, channels: &[(u8, Colour)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(52 + 7 * channels.len());
    out.extend_from_slice(b"HueStream");
    // Version 2.0, the sequence, two reserved bytes, xy and brightness, one
    // more reserved.
    out.extend_from_slice(&[2, 0, sequence, 0, 0, 1, 0]);
    out.extend_from_slice(area_id.as_bytes());
    for &(channel, c) in channels.iter().take(MAX_CHANNELS) {
        out.push(channel);
        for v in [c.x, c.y, c.bri] {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

/// A channel's colour, as the stream carries it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Colour {
    x: u16,
    y: u16,
    bri: u16,
}

impl From<BulbOutput> for Colour {
    fn from(output: BulbOutput) -> Self {
        let (rgb, bright) = match output {
            BulbOutput::Color { rgb, bright } => (rgb, bright),
            BulbOutput::White { kelvin, bright } => (super::common::kelvin_to_rgb(kelvin), bright),
        };
        let [x, y] = hue::rgb_to_xy(rgb);
        let full = |v: f64| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
        Colour {
            x: full(x),
            y: full(y),
            bri: full(f64::from(bright.min(100)) / 100.0),
        }
    }
}

/// The streaming key, from its hex.
fn psk(hex: &str) -> io::Result<Vec<u8>> {
    let bad = || io::Error::new(io::ErrorKind::InvalidInput, "bad Hue streaming key");
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return Err(bad());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2).ok_or_else(bad)?, 16).map_err(|_| bad()))
        .collect()
}

/// A stream under way: the area it plays, and the thread sending it.
struct Running {
    area: String,
    /// The lamps it was set up for, and the channels of those it plays, by
    /// REST id.
    asked: Vec<String>,
    channels: HashMap<String, Vec<u8>>,
    stop: Arc<AtomicBool>,
    sender: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct State {
    /// The lamps that asked for a seat, by REST id.
    wanted: Vec<String>,
    seats: usize,
    building: bool,
    running: Option<Running>,
    /// A stream taken down whose bridge has not been told yet.
    halting: bool,
    /// When streaming last failed, and why.
    failed: Option<(Instant, String)>,
    colours: HashMap<u8, Colour>,
}

/// One bridge's stream, shared by every lamp of it that plays.
pub struct Bridge {
    access: Access,
    state: Mutex<State>,
    changed: Condvar,
}

/// Every bridge streaming, or setting up to, by address.
fn bridges() -> &'static Mutex<HashMap<String, Weak<Bridge>>> {
    static BRIDGES: OnceLock<Mutex<HashMap<String, Weak<Bridge>>>> = OnceLock::new();
    BRIDGES.get_or_init(Default::default)
}

impl Bridge {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The bridge `access` reaches, shared with the lamps already on it.
    fn of(access: &Access) -> Arc<Bridge> {
        let mut all = bridges().lock().unwrap_or_else(|p| p.into_inner());
        all.retain(|_, b| b.strong_count() > 0);
        if let Some(b) = all.get(&access.ip).and_then(Weak::upgrade) {
            return b;
        }
        let b = Arc::new(Bridge {
            access: access.clone(),
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        all.insert(access.ip.clone(), Arc::downgrade(&b));
        b
    }

    /// Set the area up for every lamp that asked, and start streaming to it.
    fn start(self: &Arc<Self>, wanted: &[String]) -> io::Result<Running> {
        let a = &self.access;
        let listing = a.api("GET", "/clip/v2/resource/entertainment", None)?;
        let all = services(&listing);
        let lamps: Vec<Service> = wanted.iter().filter_map(|l| all.get(l).cloned()).collect();
        if lamps.is_empty() {
            return Err(io::Error::other("none of these lamps can be streamed to"));
        }
        let areas = a.api("GET", "/clip/v2/resource/entertainment_configuration", None)?;
        let mut ids: Vec<String> = lamps.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        let area_id = match our_area(&areas) {
            Some(area) if area_services(area) == ids => id_of(area)?,
            Some(area) => {
                let id = id_of(area)?;
                let path = format!("/clip/v2/resource/entertainment_configuration/{id}");
                // Another app may have left it streaming.
                let _ = a.api("PUT", &path, Some(&json!({"action": "stop"})));
                self.set_area(Some(&path), &lamps)?;
                id
            }
            None => self.set_area(None, &lamps)?,
        };
        let path = format!("/clip/v2/resource/entertainment_configuration/{area_id}");
        let area = a.api("GET", &path, None)?;
        let area = area
            .get("data")
            .and_then(Value::as_array)
            .and_then(|d| d.first())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no such area"))?;
        let channels: HashMap<String, Vec<u8>> = wanted
            .iter()
            .filter_map(|l| {
                let found = channels_of(area, &all.get(l)?.id);
                (!found.is_empty()).then(|| (l.clone(), found))
            })
            .collect();
        a.api("PUT", &path, Some(&json!({"action": "start"})))?;
        let addr = (a.ip.as_str(), STREAM_PORT)
            .to_socket_addrs()?
            .find(SocketAddr::is_ipv4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no address"))?;
        let client =
            match DtlsClient::connect(addr, a.username.as_bytes(), &psk(&a.clientkey)?, TIMEOUT) {
                Ok(client) => client,
                Err(e) => {
                    let _ = a.api("PUT", &path, Some(&json!({"action": "stop"})));
                    return Err(e);
                }
            };
        let stop = Arc::new(AtomicBool::new(false));
        let sender = {
            let bridge = Arc::downgrade(self);
            let stop = Arc::clone(&stop);
            let area_id = area_id.clone();
            std::thread::Builder::new()
                .name("lueur-hue-stream".into())
                .spawn(move || send(bridge, client, &area_id, &stop))?
        };
        log::info!(ip = a.ip.as_str(), lamps = channels.len(); "streaming to a Hue bridge");
        Ok(Running {
            area: area_id,
            asked: wanted.to_vec(),
            channels,
            stop,
            sender: Some(sender),
        })
    }

    /// Write Lueur's area for `lamps`: a new one, or the one at `path`.
    /// Their segments apart if the bridge takes it, else each lamp whole.
    fn set_area(&self, path: Option<&str>, lamps: &[Service]) -> io::Result<String> {
        let a = &self.access;
        let write = |whole: bool| -> io::Result<Value> {
            let body = area_body(lamps, whole);
            match path {
                Some(p) => a.api("PUT", p, Some(&body)),
                None => a.api(
                    "POST",
                    "/clip/v2/resource/entertainment_configuration",
                    Some(&body),
                ),
            }
        };
        let reply = match write(false) {
            Ok(v) => v,
            Err(_) if lamps.iter().any(|s| s.segments > 1) => write(true)?,
            Err(e) => return Err(e),
        };
        match path {
            Some(p) => Ok(p.rsplit('/').next().unwrap_or_default().to_string()),
            None => reply
                .pointer("/data/0/rid")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no area id in reply")),
        }
    }

    /// Take the stream down, if there is one: the sender is told to stop,
    /// and [`Bridge::halt`] (called without the lock, which the sender
    /// takes) finishes the job.
    fn take_down(&self, state: &mut State) -> Option<Running> {
        let running = state.running.take()?;
        running.stop.store(true, Ordering::SeqCst);
        state.colours.clear();
        state.halting = true;
        Some(running)
    }

    /// Wait for the sender to end, then tell the bridge to stop streaming.
    fn halt(&self, running: Option<Running>) {
        let Some(mut running) = running else {
            return;
        };
        if let Some(sender) = running.sender.take() {
            let _ = sender.join();
        }
        let path = format!(
            "/clip/v2/resource/entertainment_configuration/{}",
            running.area
        );
        if let Err(e) = self
            .access
            .api("PUT", &path, Some(&json!({"action": "stop"})))
        {
            log::debug!("the Hue bridge would not stop streaming: {e}");
        }
        self.lock().halting = false;
        self.changed.notify_all();
    }
}

fn id_of(area: &Value) -> io::Result<String> {
    area.get("id")
        .and_then(Value::as_str)
        .filter(|id| id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        .map(str::to_string)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad area id"))
}

/// The sender: the latest colour of every channel, [`SEND_EVERY`], until
/// told to stop or the bridge is gone.
fn send(bridge: Weak<Bridge>, mut client: DtlsClient, area: &str, stop: &AtomicBool) {
    let mut sequence = 0u8;
    while !stop.load(Ordering::SeqCst) {
        let Some(b) = bridge.upgrade() else {
            break;
        };
        let channels: Vec<(u8, Colour)> = {
            let state = b.lock();
            let mut c: Vec<(u8, Colour)> = state.colours.iter().map(|(&k, &v)| (k, v)).collect();
            c.sort_unstable_by_key(|&(k, _)| k);
            c
        };
        if !channels.is_empty() {
            sequence = sequence.wrapping_add(1);
            if let Err(e) = client.send(&frame(area, sequence, &channels)) {
                log::warn!("the Hue stream broke: {e}");
                let mut state = b.lock();
                state.failed = Some((Instant::now(), e.to_string()));
                b.changed.notify_all();
                break;
            }
        }
        drop(b);
        std::thread::sleep(SEND_EVERY);
    }
    client.close();
}

/// One lamp's place in its bridge's stream.
pub struct Seat {
    bridge: Arc<Bridge>,
    light: String,
    channels: Vec<u8>,
    left: bool,
}

impl Seat {
    /// Take a seat for lamp `light` (its REST id) in the stream `access`
    /// reaches, setting the stream up if this is the first.
    ///
    /// Fails when the bridge cannot stream to it (another app streams, the
    /// lamp cannot be streamed to, the bridge is too old), or failed to
    /// lately: the lamp then keeps the REST API.
    pub fn take(access: &Access, light: &str) -> io::Result<Seat> {
        if !access.can_stream() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "no streaming key",
            ));
        }
        let bridge = Bridge::of(access);
        let mut state = bridge.lock();
        state.seats += 1;
        if !state.wanted.iter().any(|w| w == light) {
            state.wanted.push(light.to_string());
        }
        let deadline = Instant::now() + GATHER + TIMEOUT * 6;
        let outcome = loop {
            if let Some((at, why)) = &state.failed
                && at.elapsed() < RETRY
            {
                break Err(io::Error::other(why.clone()));
            }
            // A stream that broke is set up anew once it has rested.
            if let Some(running) = state.running.as_ref().filter(|_| state.failed.is_none()) {
                if let Some(found) = running.channels.get(light) {
                    break Ok(found.clone());
                }
                if running.asked.iter().any(|w| w == light) {
                    // Set up with it, yet nothing plays it: the bridge
                    // cannot stream to this lamp.
                    break Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "this lamp cannot be streamed to",
                    ));
                }
            }
            if Instant::now() >= deadline {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the Hue stream did not start",
                ));
            }
            if state.building || state.halting {
                state = bridge
                    .changed
                    .wait_timeout(state, Duration::from_millis(200))
                    .map(|(s, _)| s)
                    .unwrap_or_else(|p| p.into_inner().0);
                continue;
            }
            // Not running, or running without this lamp: set it up anew,
            // once the lamps opening with it have asked too.
            let old = bridge.take_down(&mut state);
            state.building = true;
            drop(state);
            bridge.halt(old);
            std::thread::sleep(GATHER);
            let wanted = bridge.lock().wanted.clone();
            let started = bridge.start(&wanted);
            state = bridge.lock();
            state.building = false;
            match started {
                Ok(running) => {
                    state.failed = None;
                    state.running = Some(running);
                }
                Err(e) => {
                    log::info!(ip = access.ip.as_str(); "a Hue bridge will not stream ({e}), its lamps take commands one by one");
                    state.failed = Some((Instant::now(), e.to_string()));
                }
            }
            bridge.changed.notify_all();
        };
        match outcome {
            Ok(channels) => {
                drop(state);
                Ok(Seat {
                    bridge,
                    light: light.to_string(),
                    channels,
                    left: false,
                })
            }
            Err(e) => {
                state.seats -= 1;
                state.wanted.retain(|w| w != light);
                Err(e)
            }
        }
    }

    /// How many parts of the lamp the stream colours apart.
    pub fn zones(&self) -> usize {
        self.channels.len()
    }

    /// Show `outputs` on the lamp, spread over its channels.
    pub fn show(&self, outputs: &[BulbOutput]) -> io::Result<()> {
        let mut state = self.bridge.lock();
        if let Some((_, why)) = &state.failed {
            return Err(io::Error::other(why.clone()));
        }
        let n = self.channels.len();
        for (j, &channel) in self.channels.iter().enumerate() {
            if let Some(o) = zone_output(outputs, j, n) {
                state.colours.insert(channel, Colour::from(o));
            }
        }
        Ok(())
    }

    /// Give the seat up, and wait (a moment at most) for the stream to stop
    /// once every lamp has: until then it would cover whatever the lamp is
    /// told over REST.
    pub fn leave(&mut self) {
        let bridge = Arc::clone(&self.bridge);
        let old = self.give_up(&mut bridge.lock());
        bridge.halt(old);
        let mut state = bridge.lock();
        let deadline = Instant::now() + SETTLE;
        while (state.running.is_some() || state.halting) && state.failed.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = bridge
                .changed
                .wait_timeout(state, left)
                .map(|(s, _)| s)
                .unwrap_or_else(|p| p.into_inner().0);
        }
    }

    /// Give the seat up; the stream to halt if it was the last.
    fn give_up(&mut self, state: &mut State) -> Option<Running> {
        if std::mem::replace(&mut self.left, true) {
            return None;
        }
        state.seats = state.seats.saturating_sub(1);
        let light = self.light.as_str();
        state.wanted.retain(|w| w != light);
        for c in &self.channels {
            state.colours.remove(c);
        }
        if state.seats == 0 {
            self.bridge.take_down(state)
        } else {
            None
        }
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let bridge = Arc::clone(&self.bridge);
        let old = self.give_up(&mut bridge.lock());
        bridge.halt(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;

    fn colour(rgb: Rgb) -> Colour {
        Colour::from(BulbOutput::Color { rgb, bright: 100 })
    }

    const AREA_ID: &str = "1a8d99cc-967b-44f2-9202-43f976c0fa6b";

    #[test]
    fn frames_follow_huestream_v2() {
        let red = colour(Rgb::new(255, 0, 0));
        let f = frame(AREA_ID, 7, &[(0, red), (3, Colour::default())]);
        assert_eq!(&f[..9], b"HueStream");
        assert_eq!(&f[9..16], &[2, 0, 7, 0, 0, 1, 0]);
        assert_eq!(&f[16..52], AREA_ID.as_bytes());
        assert_eq!(f.len(), 52 + 2 * 7);
        assert_eq!(f[52], 0);
        assert_eq!(&f[57..59], &[0xff, 0xff], "full brightness");
        assert_eq!(f[59], 3);
        assert_eq!(&f[64..66], &[0, 0], "black");
        // Red sits far along x.
        assert!(u16::from_be_bytes([f[53], f[54]]) > 40_000);
    }

    #[test]
    fn only_renderers_are_streamed_to() {
        let v: Value = serde_json::from_str(
            r#"{"errors":[],"data":[
                {"id":"e1","id_v1":"/lights/5","renderer":true,"segments":{"max_segments":1}},
                {"id":"e2","id_v1":"/lights/6","renderer":true,"segments":{"max_segments":10}},
                {"id":"e3","id_v1":"/lights/7","renderer":false},
                {"id":"e4","renderer":true}
            ]}"#,
        )
        .unwrap();
        let s = services(&v);
        assert_eq!(s.len(), 2);
        assert_eq!(
            s["5"],
            Service {
                id: "e1".into(),
                segments: 1
            }
        );
        assert_eq!(s["6"].segments, MAX_SEGMENTS);
    }

    #[test]
    fn a_lamp_s_channels_come_in_segment_order() {
        let area: Value = serde_json::from_str(
            r#"{"id":"x","channels":[
                {"channel_id":0,"members":[{"service":{"rid":"strip"},"index":2}]},
                {"channel_id":1,"members":[{"service":{"rid":"bulb"},"index":0}]},
                {"channel_id":2,"members":[{"service":{"rid":"strip"},"index":0}]},
                {"channel_id":3,"members":[{"service":{"rid":"strip"},"index":1}]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(channels_of(&area, "strip"), vec![2, 3, 0]);
        assert_eq!(channels_of(&area, "bulb"), vec![1]);
        assert!(channels_of(&area, "other").is_empty());
    }

    #[test]
    fn the_area_lists_every_lamp_and_segment() {
        let lamps = [
            Service {
                id: "a".into(),
                segments: 1,
            },
            Service {
                id: "b".into(),
                segments: 3,
            },
        ];
        let body = area_body(&lamps, false);
        assert_eq!(body["metadata"]["name"], AREA);
        let locs = body["locations"]["service_locations"].as_array().unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[1]["service"]["rid"], "b");
        assert_eq!(locs[1]["positions"].as_array().unwrap().len(), 3);
        let whole = area_body(&lamps, true);
        assert_eq!(
            whole["locations"]["service_locations"][1]["positions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let area = json!({"locations": body["locations"]});
        assert_eq!(area_services(&area), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn the_key_is_read_from_its_hex() {
        assert_eq!(psk("00ff10").unwrap(), vec![0, 255, 16]);
        for bad in ["", "abc", "zz", "0g"] {
            assert!(psk(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_pairing_without_its_key_does_not_stream() {
        let access = Access {
            ip: "192.0.2.1".into(),
            username: "user".into(),
            clientkey: String::new(),
            cert: "ab".repeat(32),
        };
        assert!(!access.can_stream());
        assert!(Seat::take(&access, "1").is_err());
        let clear = Access {
            clientkey: "00".repeat(16),
            cert: String::new(),
            ..access
        };
        assert!(!clear.can_stream());
    }
}
