//! Tuya / Smart Life bulbs on the LAN, through the `rustuya` crate.
//!
//! Each bulb needs the local key imported by the Smart Life sign-in
//! (`tuya_cloud.rs`). Which DPs to write comes from
//! [`lueur_core::device::tuya`]; this module finds the bulbs, opens the
//! encrypted connection and moves those writes off the frame loop.
//!
//! Finding them: Tuya devices announce themselves over UDP broadcasts, but
//! many PCs' firewalls drop those. So Lueur also knocks on TCP 6668 across
//! the local subnet (see [`lan_hosts`]) and asks each answering host for its
//! status with every imported key: only the right key gets a readable answer.

use lueur_core::color::{BulbOutput, Rgb};
use lueur_core::device::common::{LastSent, Throttle, ipv4, lan_hosts};
use lueur_core::device::tuya::{self as dps, ID_PREFIX, PORT, TuyaDps};
use lueur_core::device::wire::Wire;
use lueur_core::device::{
    DeviceKind, DiscoveredDevice, Endpoint, LightStream, REST_BRIGHT, REST_KELVIN, TuyaDevice,
};
use lueur_core::yeelight::Effect;
use rustuya::tokio::{Device, Discovery, Event, Version};
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

/// What a Tuya bulb follows smoothly (measured: 20 updates a second hold).
const FRAME_INTERVAL: Duration = Duration::from_millis(50);
/// How long a connection attempt with one key and version may take.
const PROBE_TIMEOUT: Duration = Duration::from_millis(2500);
/// Versions to try on a bulb whose version is unknown, most common first.
/// A 3.4 handshake fails fast on a 3.3 bulb, so 3.4 goes first.
const VERSIONS: [Version; 3] = [Version::V3_4, Version::V3_3, Version::V3_5];
/// Most hosts to try keys on, so a LAN full of Tuya plugs stays quick.
const MAX_CANDIDATES: usize = 24;
/// Most hosts knocked on at once. A /24 goes in one wave; a wider subnet
/// takes a few, rather than a thousand connections opened in one burst.
const MAX_KNOCKS: usize = 256;

/// The runtime every Tuya connection lives on. Small: it only moves a few
/// datagrams a second per bulb.
fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("lueur-tuya")
            .enable_all()
            .build()
            .expect("the Tuya runtime starts")
    })
}

fn parse_version(v: &str) -> Option<Version> {
    Version::parse(v).filter(|v| *v != Version::Auto)
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// Connect to `id` at `ip` with `version`, and prove the key works by reading
/// the bulb's status: a 3.3 bulb accepts any TCP client, so only a readable
/// answer means the key is right. Returns the device and its DPs.
async fn connect(
    id: &str,
    key: &str,
    ip: &str,
    version: Version,
    wait: Duration,
) -> io::Result<(Device, Value)> {
    let dev = Device::builder(id.to_string(), key.as_bytes().to_vec())
        .address(ip.to_string())
        .version(version)
        .auto_reconnect(false)
        .connect()
        .map_err(io_err)?;
    let attempt = async {
        dev.wait_connected(wait).await.map_err(io_err)?;
        let mut events = dev.listener();
        dev.query().await.map_err(io_err)?;
        loop {
            match events.recv().await {
                Some(Event::Frame(m)) if !m.payload.is_empty() => {
                    if let Ok(v) = serde_json::from_slice::<Value>(&m.payload)
                        && let Some(dps) = v.get("dps")
                    {
                        return Ok(dps.clone());
                    }
                }
                Some(_) => continue,
                None => return Err(io::Error::from(io::ErrorKind::ConnectionAborted)),
            }
        }
    };
    match tokio::time::timeout(wait * 2, attempt).await {
        Ok(Ok(status)) => Ok((dev, status)),
        Ok(Err(e)) => {
            dev.close().await;
            Err(e)
        }
        Err(_) => {
            dev.close().await;
            Err(io::Error::from(io::ErrorKind::TimedOut))
        }
    }
}

/// Connect, trying `known` first and then every version.
async fn connect_any(
    id: &str,
    key: &str,
    ip: &str,
    known: Option<Version>,
) -> io::Result<(Device, Value, Version)> {
    let mut order: Vec<Version> = known.into_iter().collect();
    order.extend(VERSIONS.iter().copied().filter(|v| Some(*v) != known));
    let mut last = io::Error::from(io::ErrorKind::TimedOut);
    for version in order {
        match connect(id, key, ip, version, PROBE_TIMEOUT).await {
            Ok((dev, status)) => return Ok((dev, status, version)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Trace the DPs about to be written. They go out encrypted with the local
/// key, which is never part of them.
fn trace(wire: &Wire, value: &Value) {
    if Wire::enabled() {
        wire.text(&value.to_string());
    }
}

fn parts(ep: &Endpoint) -> io::Result<(&str, &str, &str, &TuyaDps)> {
    match &ep.kind {
        DeviceKind::Tuya { key, version, dps } if !key.is_empty() => {
            ipv4(&ep.ip)?;
            let id = ep.id.strip_prefix(ID_PREFIX).unwrap_or(&ep.id);
            Ok((id, key, version, dps))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a paired Tuya bulb",
        )),
    }
}

/// Write some DPs once and disconnect.
fn write_once(ep: &Endpoint, value: impl FnOnce(&TuyaDps) -> Value) -> io::Result<()> {
    let (id, key, version, layout) = parts(ep)?;
    let value = value(layout);
    trace(&Wire::of(ep), &value);
    rt().block_on(async {
        let (dev, _, _) = connect_any(id, key, &ep.ip, parse_version(version)).await?;
        let sent = dev.set_dps(value).await.map_err(io_err);
        // Commands are queued; give the write a moment before hanging up.
        tokio::time::sleep(Duration::from_millis(300)).await;
        dev.close().await;
        sent
    })
}

pub fn set_power(ep: &Endpoint, on: bool) -> io::Result<()> {
    write_once(ep, |layout| dps::power_dps(layout, on))
}

/// Light the bulb at `color`, full brightness.
pub fn identify(ep: &Endpoint, color: Rgb) -> io::Result<()> {
    let output = BulbOutput::Color {
        rgb: lueur_core::color::full_chroma(color),
        bright: 100,
    };
    write_once(ep, |layout| dps::steady_dps(layout, output))
}

/// The per-frame channel to one bulb.
///
/// `push` never waits on the network: it hands the DPs to a task on the Tuya
/// runtime, which always sends the newest (merged, so a mode switch is never
/// lost to a later frame).
pub struct TuyaStream {
    dev: Device,
    layout: TuyaDps,
    tx: Option<mpsc::UnboundedSender<Value>>,
    task: Option<tokio::task::JoinHandle<()>>,
    last: LastSent,
    shown: Option<BulbOutput>,
    pace: Throttle,
}

pub fn open_stream(ep: &Endpoint) -> io::Result<Box<dyn LightStream>> {
    let (id, key, version, layout) = parts(ep)?;
    let layout = *layout;
    let wire = Wire::of(ep);
    // Opening writes nothing: the bulb is switched on with the first frame
    // (see `push`), so a stream opened too late, after the effect was
    // stopped, leaves it as it was.
    let dev = rt().block_on(async {
        let (dev, _, _) = connect_any(id, key, &ep.ip, parse_version(version)).await?;
        Ok::<_, io::Error>(dev)
    })?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Value>();
    let sender = dev.clone();
    let task = rt().spawn(async move {
        let wire = wire;
        while let Some(mut next) = rx.recv().await {
            // Fold whatever else is waiting into one write.
            while let Ok(more) = rx.try_recv() {
                if let (Value::Object(a), Value::Object(b)) = (&mut next, more) {
                    a.extend(b);
                }
            }
            trace(&wire, &next);
            if let Err(e) = sender.set_dps(next).await {
                log::debug!(family = wire.family(), id = wire.id(); "write failed: {}", io_err(e));
                break;
            }
        }
    });
    Ok(Box::new(TuyaStream {
        dev,
        layout,
        tx: Some(tx),
        task: Some(task),
        last: LastSent::default(),
        shown: None,
        pace: Throttle::new(FRAME_INTERVAL),
    }))
}

impl LightStream for TuyaStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn push(&mut self, output: BulbOutput, effect: Effect, _: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        let set_mode = self.shown.is_none_or(|s| !dps::same_mode(s, output));
        let mut value = dps::frame_dps(&self.layout, output, effect, set_mode);
        // The first frame also switches the bulb on.
        if self.shown.is_none()
            && let (Value::Object(frame), Value::Object(on)) =
                (&mut value, dps::power_dps(&self.layout, true))
        {
            frame.extend(on);
        }
        self.tx
            .as_ref()
            .and_then(|tx| tx.send(value).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "Tuya sender stopped"))?;
        self.last.mark(output);
        self.shown = Some(output);
        Ok(())
    }

    fn rest(&mut self) {
        let rest = dps::steady_dps(
            &self.layout,
            BulbOutput::White {
                kelvin: REST_KELVIN,
                bright: REST_BRIGHT,
            },
        );
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(rest);
        }
        let task = self.task.take();
        let dev = self.dev.clone();
        rt().block_on(async move {
            if let Some(task) = task {
                let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            dev.close().await;
        });
    }
}

impl Drop for TuyaStream {
    fn drop(&mut self) {
        self.tx.take();
        let dev = self.dev.clone();
        rt().spawn(async move { dev.close().await });
    }
}

/// Hosts with TCP 6668 open on the local subnet.
async fn open_hosts(timeout: Duration) -> Vec<Ipv4Addr> {
    let knocks = Arc::new(tokio::sync::Semaphore::new(MAX_KNOCKS));
    let probes = lan_hosts().into_iter().map(|ip| {
        let knocks = Arc::clone(&knocks);
        async move {
            let _turn = knocks.acquire_owned().await.ok()?;
            let addr = SocketAddr::from((ip, PORT));
            match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await {
                Ok(Ok(_)) => Some(ip),
                _ => None,
            }
        }
    });
    let mut set = tokio::task::JoinSet::new();
    for p in probes {
        set.spawn(p);
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(ip)) = r {
            out.push(ip);
        }
    }
    out
}

/// Where each imported bulb is, and the version it speaks.
///
/// `hints` are the addresses and versions bindings last saw, tried first.
pub fn discover(
    timeout: Duration,
    devices: &[TuyaDevice],
    hints: &HashMap<String, (String, String)>,
) -> Vec<DiscoveredDevice> {
    if devices.is_empty() {
        return Vec::new();
    }
    rt().block_on(async {
        // Broadcast announcements, where the firewall lets them through,
        // alongside the TCP sweep that works where it does not.
        let announced = async move {
            match Discovery::new() {
                Ok(disco) => {
                    let seen = disco.scan(timeout).await;
                    disco.close().await;
                    seen
                }
                Err(_) => Vec::new(),
            }
        };
        let announced = tokio::spawn(announced);
        let mut hosts = open_hosts(Duration::from_millis(600)).await;
        let announced = announced.await.unwrap_or_default();

        let mut found: HashMap<String, (String, Option<Version>)> = HashMap::new();
        for info in announced {
            // A forged announcement must not send the keys off the LAN.
            if devices.iter().any(|d| d.id == info.id) && ipv4(&info.ip.to_string()).is_ok() {
                found.insert(info.id.clone(), (info.ip.to_string(), info.version));
            }
        }

        // The rest: try their keys on every unclaimed host. One task per
        // host, since many bulbs take a single connection at a time; on each
        // host the bulbs last seen there go first.
        let claimed: Vec<String> = found.values().map(|(ip, _)| ip.clone()).collect();
        hosts.retain(|h| !claimed.contains(&h.to_string()));
        hosts.truncate(MAX_CANDIDATES);
        let pending: Vec<(String, String)> = devices
            .iter()
            .filter(|d| !found.contains_key(&d.id))
            .map(|d| (d.id.clone(), d.key.clone()))
            .collect();
        let located = Arc::new(Mutex::new(HashMap::<String, (String, Version)>::new()));
        let mut set = tokio::task::JoinSet::new();
        for host in hosts {
            let ip = host.to_string();
            let mut order = pending.clone();
            order.sort_by_key(|(id, _)| hints.get(id).is_none_or(|(h, _)| *h != ip));
            let hints = hints.clone();
            let located = Arc::clone(&located);
            set.spawn(async move {
                for (id, key) in order {
                    let taken = located.lock().map(|l| l.contains_key(&id)).unwrap_or(true);
                    if taken {
                        continue;
                    }
                    let known = hints.get(&id).and_then(|(_, v)| parse_version(v));
                    if let Ok((dev, _, version)) = connect_any(&id, &key, &ip, known).await {
                        dev.close().await;
                        if let Ok(mut l) = located.lock() {
                            l.insert(id, (ip, version));
                        }
                        // One device per address.
                        return;
                    }
                }
            });
        }
        while set.join_next().await.is_some() {}
        let mut verified = located.lock().map(|l| l.clone()).unwrap_or_default();
        // An announcement without a version still needs one.
        for (id, (ip, version)) in found {
            match version {
                Some(v) => {
                    verified.insert(id, (ip, v));
                }
                None => {
                    if let Some(d) = devices.iter().find(|d| d.id == id)
                        && let Ok((dev, _, v)) = connect_any(&id, &d.key, &ip, None).await
                    {
                        dev.close().await;
                        verified.insert(id, (ip, v));
                    }
                }
            }
        }

        devices
            .iter()
            .filter_map(|d| {
                let (ip, version) = verified.get(&d.id)?;
                Some(DiscoveredDevice {
                    id: format!("{ID_PREFIX}{}", d.id),
                    ip: ip.clone(),
                    port: PORT,
                    model: d.product.clone(),
                    name: d.name.clone(),
                    support: Vec::new(),
                    fw_ver: None,
                    kind: DeviceKind::Tuya {
                        key: d.key.clone(),
                        version: version.as_str().to_string(),
                        dps: d.dps,
                    },
                    paired: true,
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a real bulb: finds it, sweeps the hues for three seconds, then
    /// leaves it on warm white. `LUEUR_TUYA_DEVICES` names a JSON list whose
    /// first entry has the bulb's `id` and `local_key`, so the key stays in
    /// that file.
    /// `LUEUR_TUYA_DEVICES=devices.json cargo test -p lueur tuya_bulb -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tuya_bulb_follows_a_sweep() {
        let path = std::env::var("LUEUR_TUYA_DEVICES").expect("LUEUR_TUYA_DEVICES");
        let list: Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("devices file"))
                .expect("JSON");
        let text = |k: &str| list[0][k].as_str().expect(k).to_string();
        let device = TuyaDevice {
            id: text("id"),
            name: "test".into(),
            key: text("local_key"),
            product: String::new(),
            dps: TuyaDps::default(),
        };
        let started = std::time::Instant::now();
        let found = discover(Duration::from_secs(3), &[device], &HashMap::new());
        let d = found.first().expect("the bulb is found");
        let DeviceKind::Tuya { version, .. } = &d.kind else {
            panic!("not a Tuya bulb");
        };
        println!(
            "found {} at {} (v{version}) in {:?}",
            d.id,
            d.ip,
            started.elapsed()
        );

        let mut stream = open_stream(&d.endpoint()).expect("stream");
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(3) {
            let h = (started.elapsed().as_millis() as f32 / 3000.0 * 6.0) as usize % 6;
            let rgb = [
                Rgb::new(255, 0, 0),
                Rgb::new(255, 255, 0),
                Rgb::new(0, 255, 0),
                Rgb::new(0, 255, 255),
                Rgb::new(0, 0, 255),
                Rgb::new(255, 0, 255),
            ][h];
            stream
                .push(BulbOutput::Color { rgb, bright: 80 }, Effect::Smooth, 50)
                .expect("push");
            std::thread::sleep(Duration::from_millis(16));
        }
        stream.rest();
    }
}
