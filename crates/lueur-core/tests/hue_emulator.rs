//! Lueur against an emulated Hue bridge: discovery, pairing, the REST API
//! and the Entertainment stream, end to end.
//!
//! It needs a [diyHue](https://github.com/diyhue/diyHue) bridge whose
//! address is in `LUEUR_HUE_EMULATOR`, with a few lamps added, on a network
//! where this test is not that address's own host (Lueur talks to no
//! address of its own computer). In Docker, both on one network:
//!
//! ```text
//! docker network create lueur-e2e
//! docker run -d --name lueur-hue --network lueur-e2e diyhue/core:latest
//! docker run --rm --network lueur-e2e -v .:/src -w /src -e CARGO_TARGET_DIR=/tmp/target \
//!     -e LUEUR_HUE_EMULATOR=<its address> \
//!     rust:latest cargo test -p lueur-core --test hue_emulator -- --ignored --nocapture
//! ```
//!
//! Lamps are added with the key diyHue's web interface uses (`GET /get-key`),
//! as WLED lights, which diyHue streams to only with all of their settings:
//!
//! ```text
//! POST /api/<key>/lights {"ip": "<any>", "protocol": "wled", "config": {
//!     "lightModelID": "LCT015", "lightName": "Desk", "segmentId": 0,
//!     "ledCount": 30, "segment_start": 0, "udp_port": 21324}}
//! ```
//!
//! `LCT015` is a colour lamp, `LTW001` a White Ambiance one, `LWB010` one that
//! only dims. The link button is pressed through the same key, which a real
//! bridge has no equivalent of.

use lueur_core::color::{BulbOutput, Rgb};
use lueur_core::device::http::{self, Trust};
use lueur_core::device::{self, DeviceKind, DiscoveredDevice, Endpoint, HueBridge, hue};
use lueur_core::yeelight::Effect;
use serde_json::{Value, json};
use std::io;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(3);

fn bridge_ip() -> String {
    std::env::var("LUEUR_HUE_EMULATOR").expect("LUEUR_HUE_EMULATOR: the emulated bridge's address")
}

/// The key diyHue's web interface uses, which can press its link button.
fn admin_key(ip: &str) -> String {
    let r = http::request("GET", ip, hue::PORT, "/get-key", None, TIMEOUT).unwrap();
    r.body.trim().trim_matches('"').to_string()
}

fn press_link_button(ip: &str) {
    let path = format!("/api/{}/config", admin_key(ip));
    let body = json!({"linkbutton": true}).to_string();
    let r = http::request("PUT", ip, hue::PORT, &path, Some(&body), TIMEOUT).unwrap();
    assert!(r.ok(), "{r:?}");
}

fn pin(b: &HueBridge) -> [u8; 32] {
    hue::parse_pin(&b.cert).expect("a pinned certificate")
}

/// The state of lamp `light`, over the pinned TLS.
fn lamp_state(b: &HueBridge, light: &str) -> Value {
    let path = format!("/api/{}/lights/{light}", b.username);
    let r = http::request_with(
        Trust::Pinned(pin(b)),
        "GET",
        &b.ip,
        hue::TLS_PORT,
        &path,
        None,
        TIMEOUT,
    )
    .unwrap();
    serde_json::from_str::<Value>(&r.body).unwrap()["state"].clone()
}

/// A call to the older API, over the pinned TLS.
fn v1(b: &HueBridge, method: &str, path: &str, body: Option<Value>) -> Value {
    let body = body.map(|v| v.to_string());
    let r = http::request_with(
        Trust::Pinned(pin(b)),
        method,
        &b.ip,
        hue::TLS_PORT,
        path,
        body.as_deref(),
        TIMEOUT,
    )
    .unwrap();
    serde_json::from_str(&r.body).unwrap_or(Value::Null)
}

fn endpoint(d: &DiscoveredDevice, b: &HueBridge, streamed: bool) -> Endpoint {
    let mut ep = d.endpoint();
    if let DeviceKind::Hue { clientkey, .. } = &mut ep.kind
        && streamed
    {
        clientkey.clone_from(&b.clientkey);
    }
    ep
}

fn colour(r: u8, g: u8, b: u8) -> BulbOutput {
    BulbOutput::Color {
        rgb: Rgb::new(r, g, b),
        bright: 80,
    }
}

#[test]
#[ignore = "needs an emulated Hue bridge (LUEUR_HUE_EMULATOR)"]
fn a_bridge_from_discovery_to_the_stream() {
    let ip = bridge_ip();

    // 1. Discovery: the bridge answers mDNS, and is offered for pairing.
    let found = hue::discover(Duration::from_secs(3), &[]).unwrap();
    println!("discovered: {found:#?}");
    let heard = found
        .iter()
        .find(|d| d.ip == ip)
        .expect("the bridge answers mDNS");
    assert!(!heard.paired && heard.id.starts_with("hue:"), "{heard:?}");

    // 2. Pairing waits for the button, then pins the certificate and gets the
    //    streaming key.
    let waiting = hue::pair(&ip, &[]).unwrap_err();
    assert_eq!(waiting.kind(), io::ErrorKind::WouldBlock, "{waiting}");
    press_link_button(&ip);
    let bridge = hue::pair(&ip, &[]).unwrap();
    assert_eq!(format!("hue:{}", bridge.id), heard.id);
    assert_eq!(bridge.cert.len(), 64, "the certificate is pinned");
    assert!(
        !bridge.clientkey.is_empty(),
        "the streaming key came with it"
    );
    println!("paired: {bridge:?}");

    // 3. Discovery again: the lamps, paired, reached over TLS.
    let found = hue::discover(Duration::from_secs(3), std::slice::from_ref(&bridge)).unwrap();
    let lamps: Vec<&DiscoveredDevice> = found.iter().filter(|d| d.paired).collect();
    for l in &lamps {
        println!("lamp {} {:?} {:?}", l.id, l.name, l.model);
    }
    assert!(lamps.len() >= 2, "the emulator's lamps: {found:#?}");
    assert!(
        found.iter().all(|d| !d.faster),
        "a fresh pairing has its key"
    );
    let light = |d: &DiscoveredDevice| match &d.kind {
        DeviceKind::Hue { light, .. } => light.clone(),
        _ => unreachable!(),
    };

    // 4. Over REST: identify, then a colour on each lamp.
    let first = lamps[0];
    device::identify(&endpoint(first, &bridge, true), Rgb::new(0, 0, 255)).unwrap();
    for l in &lamps {
        let mut s = device::open_stream(&endpoint(l, &bridge, false), lamps.len(), false).unwrap();
        // Twice, apart: a lamp without colours learns it from the first reply.
        s.push(colour(255, 120, 20), Effect::Sudden, 0).unwrap();
        std::thread::sleep(s.interval() + Duration::from_millis(400));
        s.push(colour(255, 100, 10), Effect::Sudden, 0).unwrap();
        std::thread::sleep(s.interval() + Duration::from_millis(400));
        println!(
            "{} {:?} after REST: {}",
            l.id,
            l.model,
            lamp_state(&bridge, &light(l))
        );
        assert_eq!(lamp_state(&bridge, &light(l))["on"], true, "{} is on", l.id);
        s.rest();
    }

    // 5. Streaming: every colour lamp of the bridge in one stream.
    let streamed: Vec<_> = lamps
        .iter()
        .filter(|l| l.model.to_ascii_lowercase().contains("color light"))
        .copied()
        .collect();
    assert!(!streamed.is_empty(), "a colour lamp to stream to");
    let siblings = streamed.len();
    let opened: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = streamed
            .iter()
            .map(|l| {
                let ep = endpoint(l, &bridge, true);
                scope.spawn(move || device::open_stream(&ep, siblings, false))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect()
    });
    let mut streams = opened;
    for s in &streams {
        assert_eq!(
            s.interval(),
            device::entertainment::INTERVAL,
            "the lamp streams rather than taking commands one by one"
        );
    }
    let started = Instant::now();
    let mut i = 0u8;
    while started.elapsed() < Duration::from_secs(3) {
        for s in &mut streams {
            s.push(colour(i, 255 - i, 128), Effect::Sudden, 0).unwrap();
        }
        i = i.wrapping_add(7);
        std::thread::sleep(Duration::from_millis(20));
    }

    // Lueur's area is there, and streaming. Read through the older API's
    // groups: diyHue's newer API lists only the areas made through the older,
    // so Lueur, finding none of its own, makes one each run.
    let groups_path = format!("/api/{}/groups", bridge.username);
    let groups = v1(&bridge, "GET", &groups_path, None);
    let (number, area) = groups
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, g)| g["name"] == device::entertainment::AREA && g["stream"]["active"] == true)
        .map(|(n, g)| (n.clone(), g.clone()))
        .unwrap_or_else(|| panic!("Lueur's area, streaming, in {groups}"));
    println!("area: {area}");

    // 6. The bridge ends the stream behind Lueur's back (as when another
    //    app takes it): the lamps notice, rather than freeze.
    let body = json!({"stream": {"active": false}});
    v1(
        &bridge,
        "PUT",
        &format!("{groups_path}/{number}"),
        Some(body),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let broke = loop {
        if streams[0].push(colour(1, 2, 3), Effect::Sudden, 0).is_err() {
            break true;
        }
        if Instant::now() > deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(broke, "a stream the bridge ended was noticed");
    drop(streams);
    println!("done");
}
