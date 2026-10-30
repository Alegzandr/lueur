# Adding a bulb family

Everything a new family of bulbs touches, in the order you will meet it,
with WLED as the worked example. The WLED code below is a sketch to start
from: WLED is not in Lueur yet, and nothing here has run against a real
controller.

Read [DEVELOPMENT.md](DEVELOPMENT.md) first for the architecture. In short:
a family lives in `crates/lueur-core/src/device/`, behind three operations
(stream, power, identify) and one search, all dispatched on `DeviceKind` in
`device/mod.rs`. The shell (`src-tauri`) only knows families through
`devices.rs`, which hands everything to the core except Tuya, whose
encrypted transport lives in the shell.

## Checklist

| Where | What |
| --- | --- |
| `device/<family>.rs` | Protocol, stream, power, discovery, tests |
| `device/mod.rs` | `pub mod`, a `DeviceKind` variant, `family()`, `is_ready()`, the `Debug` impl, `open_stream`, `interval_of`, `set_power`, `discover` |
| `lib.rs` (core) | The family in the crate doc's family list |
| `src/lib/types.ts` | The variant in the `DeviceKind` union |
| `src/lib/brands.ts` | The brand name in `FAMILY` (and the copy in `views/Bulbs.svelte`, while there is one) |
| `src/lib/i18n.ts` | `first_text`, `tip_5`, and any family-specific hint, in English and French |
| `README.md` | A row in *Supported bulbs*, "not yet tested on hardware" until it is |
| `docs/DEVELOPMENT.md` | The family in the `device/` line of the architecture |

A family with a credential (a token, a key) has more to do; see
[Credentials](#credentials).

The compiler finds most of the `DeviceKind` matches for you: adding the
variant breaks every `match` that has no `_` arm. The ones with a `_` arm it
cannot find are `is_ready()` in `device/mod.rs`, and in the shell `blank()`
and `restore()` in `secrets.rs` and `credential_mut()` in `vault.rs`, which
only matter for a family with a credential.

## 1. The protocol, as pure functions

Put what goes on the wire in functions that take a `BulbOutput` and return
bytes or JSON, with no socket in sight, so tests can pin every byte. WiZ has
`pilot()`, LIFX `frame()` and `set_color_payload()`, Nanoleaf `frame()` and
`panel_rgb()`, Govee `commands()`.

A `BulbOutput` is `Color { rgb, bright }` or `White { kelvin, bright }`,
brightness in percent (1 to 100). Helpers in `device/common.rs`:
`scale_bright` (percent onto a device range), `kelvin_to_rgb` (white for a
bulb without a white channel), `rgb_to_hsv`.

WLED's realtime UDP protocol (port 21324) takes one RGB triplet per LED. Its
`DRGB` mode is a protocol byte (2), a timeout in seconds after which WLED
goes back to its own effect, then up to 490 triplets. A strip mirrors one
colour, since the room plan treats it as one light, and a triplet has no
brightness, so brightness is folded in the way Nanoleaf does it:

```rust
//! WLED (ESP8266 / ESP32 LED controllers): realtime UDP on port 21324.
//!
//! No pairing and no key, like WiZ. The JSON API on port 80 says how many
//! LEDs there are; the stream then sets them all to one colour per frame.

use super::common::{LastSent, Throttle, kelvin_to_rgb, udp_to};
use super::wire::Wire;
use super::{DeviceKind, DiscoveredDevice, LightStream, REST_BRIGHT, REST_KELVIN, http, mdns};
use crate::color::{BulbOutput, Rgb};
use crate::yeelight::Effect;
use serde_json::{Value, json};
use std::io;
use std::net::UdpSocket;
use std::time::Duration;

pub const PORT: u16 = 80;
const REALTIME_PORT: u16 = 21324;
pub const ID_PREFIX: &str = "wled:";
const TIMEOUT: Duration = Duration::from_secs(2);

/// `DRGB`: one RGB triplet per LED, from the first.
const DRGB: u8 = 2;
/// The most LEDs one `DRGB` packet sets (`DNRGB`, with a start index, goes
/// past that in several packets).
const MAX_LEDS: usize = 490;
/// Seconds without a frame before WLED goes back to its own effect, so a
/// crashed Lueur never leaves the strip frozen.
const HOLD_SECONDS: u8 = 2;
/// To be measured on hardware; WLED is said to follow 40+ fps.
const FRAME_INTERVAL: Duration = Duration::from_millis(25);

/// The colour every LED shows for an output, brightness folded in.
pub fn led_rgb(output: BulbOutput) -> Rgb {
    let (rgb, bright) = match output {
        BulbOutput::Color { rgb, bright } => (rgb, bright),
        BulbOutput::White { kelvin, bright } => (kelvin_to_rgb(kelvin), bright),
    };
    let k = f32::from(bright.min(100)) / 100.0;
    let s = |v: u8| (f32::from(v) * k).round() as u8;
    Rgb::new(s(rgb.r), s(rgb.g), s(rgb.b))
}

/// A `DRGB` packet setting `leds` LEDs to `rgb`, written into `out` so the
/// stream reuses one buffer.
pub fn frame(out: &mut Vec<u8>, leds: usize, rgb: Rgb) {
    out.clear();
    out.extend_from_slice(&[DRGB, HOLD_SECONDS]);
    for _ in 0..leds.min(MAX_LEDS) {
        out.extend_from_slice(&[rgb.r, rgb.g, rgb.b]);
    }
}
```

`Rgb` and `BulbOutput` are `Copy`; nothing here allocates once `out` has
grown to size.

## 2. The stream

`open_stream` returns a `Box<dyn LightStream>`. Its `push` is called on the
frame loop, every frame, for every light, so it must:

- **never wait on the network.** A UDP send is fine (`udp_to` sets a 500 ms
  write timeout as a backstop). Anything that can block, an HTTP request,
  a TCP write to a bulb that stopped reading, an encrypted exchange, goes
  through a `common::Mailbox`: implement `common::Driver` for the slow part
  and push into the mailbox, which keeps only the newest frame (Hue and
  Yeelight do this).
- **skip what the bulb already shows**, with `common::LastSent`: it also
  resends a still scene every five seconds, so a bulb that was switched off
  at the wall comes back to the room's colour.
- **pace itself** with `common::Throttle` to what the device follows, and
  say so in `interval()`: the shows space their steps by it. Pass
  `pace.glide(effect, duration_ms)` as the transition when the protocol has
  one, so a paced bulb fades across the gap instead of stepping.
- **trace every frame** through its `Wire` (see [Logs](#logs)).
- **return `Err` when the bulb is gone**, if the transport can tell. That
  is what hands it to `Relink`.

`rest()` leaves the bulb on a warm glow (`REST_KELVIN`, `REST_BRIGHT`) when
the effect stops. It may block a little: it runs once, off the frame loop's
budget.

```rust
pub struct WledStream {
    socket: UdpSocket,
    ip: String,
    leds: usize,
    buf: Vec<u8>,
    last: LastSent,
    pace: Throttle,
    wire: Wire,
}

impl WledStream {
    pub fn open(ip: &str, wire: Wire) -> io::Result<Self> {
        let leds = led_count(&info(ip)?)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no LEDs"))?;
        Ok(WledStream {
            socket: udp_to(ip, REALTIME_PORT)?,
            ip: ip.to_string(),
            leds,
            buf: Vec::with_capacity(2 + 3 * leds.min(MAX_LEDS)),
            last: LastSent::default(),
            pace: Throttle::new(FRAME_INTERVAL),
            wire,
        })
    }
}

impl LightStream for WledStream {
    fn interval(&self) -> Duration {
        self.pace.interval()
    }

    fn push(&mut self, output: BulbOutput, _: Effect, _: u32) -> io::Result<()> {
        if !self.last.needs(output) || !self.pace.ready() {
            return Ok(());
        }
        frame(&mut self.buf, self.leds, led_rgb(output));
        self.wire.bytes(&self.buf);
        self.socket.send(&self.buf)?;
        self.last.mark(output);
        Ok(())
    }

    fn rest(&mut self) {
        // Leaving realtime is WLED's own business once frames stop; the
        // colour it goes back to is set through the JSON API.
        let rgb = kelvin_to_rgb(REST_KELVIN);
        let body = json!({
            "on": true,
            "bri": super::common::scale_bright(REST_BRIGHT, 1, 255),
            "seg": [{"col": [[rgb.r, rgb.g, rgb.b]]}],
        })
        .to_string();
        self.wire.request("POST", "/json/state", Some(&body));
        let _ = http::request("POST", &self.ip, PORT, "/json/state", Some(&body), TIMEOUT);
    }
}

pub fn set_power(ip: &str, on: bool) -> io::Result<()> {
    let body = json!({"on": on}).to_string();
    let resp = http::request("POST", ip, PORT, "/json/state", Some(&body), TIMEOUT)?;
    if resp.ok() { Ok(()) } else { Err(io::Error::other(format!("WLED answered {}", resp.status))) }
}

fn info(ip: &str) -> io::Result<Value> {
    let resp = http::request("GET", ip, PORT, "/json/info", None, TIMEOUT)?;
    serde_json::from_str(&resp.body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// The LED count in a `/json/info` reply.
pub fn led_count(info: &Value) -> Option<usize> {
    let n = info.pointer("/leds/count")?.as_u64()?;
    usize::try_from(n).ok().filter(|&n| n > 0)
}
```

Then wire it into `device/mod.rs`:

```rust
pub mod wled;

pub enum DeviceKind {
    // …
    Wled,
}

// family():         DeviceKind::Wled => "wled",
// the Debug impl:   DeviceKind::Wled => f.write_str("Wled"),
// open_stream():    DeviceKind::Wled => Box::new(wled::WledStream::open(&ep.ip, wire)?),
// interval_of():    DeviceKind::Wled => wled::FRAME_INTERVAL,
// set_power():      DeviceKind::Wled => wled::set_power(&ep.ip, on),
```

`identify()` needs nothing: its `_` arm powers the bulb on and pushes one
frame on a fresh stream. A variant with no field is serialised as
`{"family":"wled"}`; give any field `#[serde(default)]` so a config written
before it existed still reads.

## 3. Discovery

Add the family's search to the list in `device::discover`, with its name
(the name shows in the debug log, and a failing search is logged as a
warning). Every search runs on its own thread with the same timeout; a
family that fails is skipped, so a search must return what it heard, not
fail, when a device answers badly.

Ways in, from `device/common.rs` and `device/mdns.rs`:

- a broadcast to `broadcast_targets()` (the all-ones address and our
  subnet's), as WiZ and LIFX do;
- a multicast, as Govee does;
- mDNS, `mdns::browse("_service._tcp.local", timeout)`, as Hue and Nanoleaf
  do;
- a sweep of `lan_hosts()`, as the Tuya search does when broadcasts are
  filtered. It is capped to a /22 around us; knock gently (Tuya keeps a
  semaphore of open connections).

A reply that names an address of its own (Govee's scan carries an `ip`)
goes through `common::keep`, so a host claiming another device's id cannot
move it: the reply sent from the address it names wins. Ids are
family-prefixed (`wled:<mac>`), lower-case, from what the device says about
itself (a MAC, a serial), never from its address, which changes.

```rust
/// WLED answers mDNS as `_wled._tcp`, and says who it is at `/json/info`.
pub fn discover(timeout: Duration) -> io::Result<Vec<DiscoveredDevice>> {
    let mut out = Vec::new();
    for inst in mdns::browse("_wled._tcp.local", timeout)? {
        // An address that is not a LAN one is refused by `http::request`.
        if let Ok(info) = info(&inst.ip)
            && let Some(d) = parse_info(&info, &inst.ip)
        {
            out.push(d);
        }
    }
    Ok(out)
}

/// A `/json/info` reply from the controller at `ip`, if it is one.
pub fn parse_info(info: &Value, ip: &str) -> Option<DiscoveredDevice> {
    let mac = info.get("mac")?.as_str()?.to_ascii_lowercase();
    if mac.len() != 12 || !mac.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    led_count(info)?;
    let text = |k: &str| info.get(k).and_then(Value::as_str).map(str::to_string);
    Some(DiscoveredDevice {
        id: format!("{ID_PREFIX}{mac}"),
        ip: ip.to_string(),
        port: PORT,
        model: text("arch").unwrap_or_default(),
        name: text("name").unwrap_or_default(),
        support: Vec::new(),
        fw_ver: text("ver"),
        kind: DeviceKind::Wled,
        paired: true,
    })
}
```

Discovery runs once per search, not per frame, so allocating there is fine.

## 4. Private addresses only

Lueur only ever talks to private and link-local IPv4 addresses, never to
the internet, this computer's own addresses or a multicast group (beyond
the discovery probes themselves). The rule is `common::ipv4`, and every
connection goes through it: `udp_to` and `http::request`/`http::Client`
call it, and so does `YeelightControl::connect`. A new transport (raw TCP,
say) must call `common::ipv4(ip)?` before connecting, never parse the
address itself. `subnet_hosts` already leaves out anything `is_lan`
refuses. The shell's commands also check every address they get from the
interface (`lights::validate_ip`).

## 5. Tests

Follow `wiz.rs` and `lifx.rs`: test the pure functions to the byte, and
the parsers on real replies (copy one from a device or its documentation,
and say where it came from if it is not yours).

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drgb_frame_sets_every_led() {
        let mut f = Vec::new();
        frame(&mut f, 3, Rgb::new(1, 2, 3));
        assert_eq!(f, [DRGB, HOLD_SECONDS, 1, 2, 3, 1, 2, 3, 1, 2, 3]);
        frame(&mut f, 10_000, Rgb::BLACK);
        assert_eq!(f.len(), 2 + 3 * MAX_LEDS, "one packet's worth at most");
    }

    #[test]
    fn brightness_is_folded_into_rgb() {
        let c = led_rgb(BulbOutput::Color { rgb: Rgb::new(255, 100, 0), bright: 50 });
        assert_eq!(c, Rgb::new(128, 50, 0));
    }

    #[test]
    fn parses_an_info_reply() {
        let v: Value = serde_json::from_str(
            r#"{"ver":"0.14.4","leds":{"count":60},"name":"Desk strip","arch":"esp32","mac":"A8032A1B2C3D"}"#,
        )
        .unwrap();
        let d = parse_info(&v, "192.168.1.50").unwrap();
        assert_eq!(d.id, "wled:a8032a1b2c3d");
        assert_eq!((d.name.as_str(), d.kind.clone()), ("Desk strip", DeviceKind::Wled));
        assert!(parse_info(&serde_json::json!({"mac": "zz"}), "192.168.1.50").is_none());
    }
}
```

Also worth a test where it applies:

- an address outside the LAN is refused before any traffic (`udp_to("8.8.8.8", …)`
  fails; see `only_lan_addresses_are_bulbs` in `common.rs`);
- a family with a credential refuses an unpaired endpoint before any traffic
  (`unpaired_endpoints_are_refused_before_any_traffic` in `mod.rs`, with a
  TEST-NET address such as `192.0.2.1` that is never reached);
- `DeviceKind` round-trips through serde with its `family` tag
  (`kinds_round_trip_with_a_family_tag`).

Tests must not need a bulb. A test against real hardware is `#[ignore]`d
and reads its address from an environment variable (see the Tuya one in
`src-tauri/src/tuya.rs`).

`cargo test -p lueur-core` runs them; the whole check before a pull request
is at the [end of this guide](#before-the-pull-request).

## 6. Reconnection

Nothing to write, as long as the stream behaves. The runner wraps every
stream in a `device::relink::Relink` with the same `open` it was made by:
when `push` returns an error, the stream is set aside and `open` is tried
again on a thread of its own, half a second later, then doubling up to every
30 seconds. So:

- `push` returns `Err` when the bulb is unreachable, if the transport can
  tell (TCP, HTTP, a mailbox whose worker stopped). Over UDP it cannot, and
  `LastSent`'s five-second refresh brings a bulb that came back up to date.
- `open` may be called many times, from any thread: it must not depend on
  state left by an earlier stream, and must give up within a few seconds
  (`CONTROL_TIMEOUT`, or the family's own `TIMEOUT`).
- Dropping a stream must close what it opened, without blocking long: a
  stale stream is dropped on the reconnection thread, not the frame loop.

## 7. The flash limiter

Every mode's output goes through a `safety::FlashGuard` (in
`runner/ambilight.rs` and `runner/show.rs` for the ambilight and the shows,
in `manual.rs` for lights set by hand) before
it reaches `push`, after the walls' correction so it measures the light the
bulb really gives, so the photosensitivity protection holds for any family
without a line of family code. What a family must not do is undo it:
no effect, blink or animation of the device's own while streaming, no
transition longer than the one asked (beyond `glide`), and `Effect::Sudden`
means no fade. WLED's own effects stay off while realtime frames arrive.

## 8. Credentials

WLED has none. A family that is granted something to be driven (Hue's
username, Nanoleaf's token, Tuya's local key) also needs all of this, and
each part is there for a reason (see *Hue and Nanoleaf credentials* in
DEVELOPMENT.md):

- **The credential in the kind** (`DeviceKind::Nanoleaf { token }`), with
  `#[serde(default)]`, and `is_ready()` false while it is empty:
  `require_ready` then refuses the endpoint before any traffic.
- **A pairing** in `device::Pairings` (`NanoleafPairing` is the model:
  the device's own id, the address it was granted at, the credential), and
  a `pair()` function that fails with `io::ErrorKind::WouldBlock` until the
  user has pressed the button, which `commands::pair_device` turns into
  `pair_waiting`. Add the family to the `Granted` enum there.
- **Where the credential may go**: discovery hands it out according to
  `device::approach`, never on the id alone. The credential goes to the
  address it was granted at, or over TLS to a pinned certificate; anything
  else is listed unpaired and kept in `identity.rs` for the user to confirm.
- **Kept from the interface and from exports**: `blank()` and `restore()`
  in `src-tauri/src/secrets.rs`.
- **Sealed on disk**: `credential_mut()`, `holds_credentials()` and `fill()`
  in `src-tauri/src/vault.rs`.
- **Never in a log**: mask it in the hand-written `Debug` impls in
  `device/mod.rs` (the kind, and the pairing type) with `Masked`, give the
  stream's wire `.hiding(credential)` if the credential travels with
  requests, and never name it in a log call. The test
  `no_log_call_names_a_credential` in `src-tauri/src/logging.rs` fails on a
  log call that formats a variable called `token`, `key`, `username` and
  the like; add the credential's field name to its list if it is a new
  one.
- **The interface**: the field in `types.ts`, a `pair_<family>` hint in
  `i18n.ts`, and the pairing branch in `views/Bulbs.svelte`.

## Logs

Each stream holds a `device::wire::Wire`, made by `open_stream` from the
endpoint (`Wire::of(ep)`), and hands it every frame it sends:
`wire.bytes(&frame)` for binary protocols, `wire.text(&json)` for text,
`wire.request(method, path, body)` for HTTP. They log at trace level under
`lueur_core::device::wire`, with the family and the device id, and check the
level before formatting anything, so the frame loop pays one atomic load
when tracing is off. Do not build a string only for the trace outside an
`if Wire::enabled()`. How to turn tracing on is under *Logs* in
DEVELOPMENT.md.

Other messages use the `log` macros with fields:
`log::debug!(family = "wled", id = ep.id.as_str(), ip = ep.ip.as_str(); "…")`.
Addresses only from `debug` down; `info` and above are what every user's
log file holds.

## Before the pull request

- The checks under *Commands* in [AGENTS.md](../AGENTS.md) pass (the CI
  runs the same).
- The family is in the README's *Supported bulbs*, marked untested until
  someone has run it on real hardware, and in `first_text` and `tip_5`.
- A trace log of a few seconds of ambilight on the real device, attached to
  the pull request, if you have one.
