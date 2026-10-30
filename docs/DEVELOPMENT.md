# Developing Lueur

Everything needed to build, change and release Lueur. For what the app does
and how to use it, see the [README](../README.md).

## Architecture

```
lueur/
├─ crates/lueur-core/     Pure, GUI-free, fully unit-tested Rust
│   ├─ color.rs           Colour types, linear-space averaging, smoothing
│   ├─ geometry.rs        Room model + light→screen-zone mapping
│   ├─ bars.rs            Black bars around a film, left out of the zones
│   ├─ engine.rs          Frame → one colour per light
│   ├─ scene.rs           Lights held steady: one light's white or colour, moods
│   ├─ show/              Shows with no screen: Rainbow, Sunset, Ocean, Candle
│   ├─ stage/             The music show: listening, the beat clock, the stage
│   ├─ sync.rs            Lights of different speeds kept together
│   ├─ safety.rs          Photosensitivity guard on every mode's output
│   ├─ yeelight/          LAN protocol, SSDP discovery, music-mode sockets
│   ├─ device/            Every family behind one stream/power/identify API:
│   │                     WiZ, LIFX, Govee, Hue, Nanoleaf, Tuya DP encoding;
│   │                     entertainment.rs streams to a Hue bridge over
│   │                     dtls/, a DTLS 1.2 PSK client on ring;
│   │                     wire.rs traces what each stream sends,
│   │                     relink.rs brings dropped bulbs back, lease.rs
│   │                     shares bulbs with other computers running Lueur
│   └─ config.rs          Persisted settings
├─ src-tauri/             Desktop shell (Tauri v2)
│   ├─ capture.rs         Screen capture behind a FrameSource trait
│   │  ├─ dxgi.rs        Windows: Desktop Duplication, GPU-downscaled
│   │  ├─ gdi.rs         Windows fallback: GDI, shrunk while it copies
│   │  ├─ clip.rs        Windows: HDR highlights down to the SDR white, on the GPU
│   │  ├─ hdr.rs         Windows: an HDR desktop back to SDR colours
│   │  ├─ screencast.rs  Wayland: ScreenCast portal + PipeWire stream
│   │  ├─ scale.rs       CPU shrink of a full-size frame (PipeWire)
│   │  └─ bench.rs       What examples/capture_bench.rs measures
│   ├─ runner/            The live loops: ambilight.rs, show.rs, connect.rs
│   ├─ audio.rs           What the computer plays, for the music show
│   │  └─ apps.rs        Windows: one application's sound
│   ├─ lights.rs          Everyday control shared by window and tray
│   ├─ manual.rs          Lights set by hand, through the flash guard
│   ├─ auto.rs            Windows: Screen on while a game or video is fullscreen
│   ├─ natural.rs         The natural light, following the time of day
│   ├─ sleep.rs           The sleep timer: fade out, then everything off
│   ├─ devices.rs         Dispatch between the core families and Tuya
│   ├─ tuya.rs            Tuya LAN control (rustuya), discovery by key
│   ├─ tuya_cloud.rs      Smart Life QR sign-in that imports local keys
│   ├─ commands.rs        Validated IPC surface
│   ├─ secrets.rs         Credentials kept from the UI and from exports
│   ├─ identity.rs        Paired bridges heard where nothing vouches for them
│   ├─ peers.rs           Talks with other computers running Lueur (LAN)
│   ├─ vault.rs           Credentials sealed on disk, the key in the OS keyring
│   ├─ tray.rs            Tray menu: start/stop, open, quit
│   ├─ titlebar.rs        Windows: the title bar drawn by the page
│   ├─ error.rs           Errors sent as codes the UI words in its language
│   ├─ logging.rs         Logs to stderr and a capped file, set by LUEUR_LOG
│   ├─ install.rs         Windows: silent per-user install and uninstall
│   ├─ updater.rs         Startup splash and updates from GitHub releases
│   ├─ state.rs, paths.rs Shared app state; where the data folder is
│   └─ lib.rs             Tray, autostart, single-instance, window handling
└─ src/                   Svelte 5 (runes) + TypeScript frontend
    ├─ lib/state.svelte.ts  App state and every action the UI can take
    ├─ lib/state/           …split by concern: app, config, devices, effect,
    │                       lights, pairing, toasts, derived
    ├─ lib/i18n.ts          English and French strings
    ├─ lib/views/           Lighting, Room, Bulbs, Settings
    │                       (lighting/ and room/ hold their pieces)
    ├─ lib/ui/              Shared controls (Slider, Switch, Segmented, Row…)
    └─ styles/app.css       Design tokens, light and dark themes
```

The security- and colour-critical logic lives in `lueur-core`, which has **no
GUI dependencies** and is covered by unit tests you can run without a display:

```bash
cargo test -p lueur-core
```

The device layer is a small trait, so new bulb families slot in without
touching the engine. [ADDING_A_FAMILY.md](ADDING_A_FAMILY.md) walks through
everything a new family touches, with WLED as the example.

The interface has its own tests (Vitest), run without the shell:

```bash
npm test
```

### Capture

On Windows the desktop is read through DXGI Desktop Duplication and shrunk on
the GPU, so a frame costs well under a millisecond and exclusive-fullscreen
games are captured. Where duplication refuses (a rotated display, a remote
session), GDI takes over, sampling the desktop straight into a small bitmap.

On a display with HDR on, duplication is asked for the 16-bit float scRGB
desktop Windows composes (`IDXGIOutput5::DuplicateOutput1`, which refuses a
thread that is not per-monitor DPI aware: the call sets that on its thread).
A pixel shader (`capture/clip.rs`, compiled at run time with `D3DCompile`)
draws it into the top of the mip chain divided by the display's SDR white
(`DisplayConfigGetDeviceInfo`, read again every two seconds so the
brightness slider is followed), with negative channels cut and anything
brighter brought down by its brightest channel. The floats are then shrunk,
which averages them in linear light, and encoded as sRGB on the CPU
(`capture/hdr.rs`). Bringing highlights down before the shrink keeps a dark
scene with a few bright lights as dark as it looks in SDR. The 8-bit image
the older call returns ignores the SDR white, which is what makes HDR
captures look washed out; GDI only ever sees that image. The tests in
`clip.rs` run the shader on WARP and check it gives the bytes the CPU
conversion does.

Turning HDR on or off, like any display mode change, interrupts duplication
and refuses it for a few seconds. The capture tries again every quarter of
a second for eight seconds before GDI takes over (`DUPLICATION_PATIENCE` in
`capture.rs`), the bulbs holding their colour meanwhile. Every two seconds a session also asks its
DXGI factory whether its view of the displays is still current, and opens
again if not: one opened while Windows was still switching would otherwise
keep reading the old format.

On a Linux Wayland session the screen mode opens one xdg-desktop-portal
ScreenCast session and reads its PipeWire stream, shrunk on arrival. The
desktop asks once which screen to share; where the portal can remember it
(ScreenCast version 4 and up: recent GNOME and KDE), the restore token it
returns is kept in `screencast-token.json` in the data folder and later
launches start without asking. Without a portal it falls back to `xcap`'s
one-shot screenshots, slow and on some desktops asking every time, and says
so on stderr. X11 and macOS use `xcap`'s one-shot capture (XCB and
CoreGraphics), at full resolution.

What it costs is measured by an example that runs capture and colour
computation at the target rate without any bulb:

```bash
cargo run --release -p lueur --example capture_bench -- --help
```

On the reference machine (Ryzen 7 9850X3D, RTX 4080 SUPER, Windows 11, one
2560×1440 display, a video playing), 300 frames at 45 fps, 8 lights; CPU is
the whole process's, in percent of one core, lowest and highest of three
runs:

| Path | Frame | Capture avg / p95 | Colours avg / p95 | CPU |
| --- | --- | --- | --- | --- |
| DXGI (what Lueur uses) | 320×180 | 0.5 / 1.4 ms | 0.06 / 0.09 ms | 3.7–4.2 % |
| GDI fallback | 640×360 | 13 / 15 ms | 0.19 / 0.26 ms | 22–30 % |
| `xcap` full frame (the old fallback) | 2560×1440 | 19.5 / 22 ms | 0.23 / 0.32 ms | 50–57 % |

GDI's floor is reading the composed desktop back from the GPU, whatever the
shrink. `HALFTONE` stretching was measured at about the CPU of the full copy
it replaces, so the fallback samples with `COLORONCOLOR` at twice the DXGI
size instead. GPU time is not in these figures, and the effect on a game's
frame rate has not been measured directly: DXGI's share is one small copy
and mip chain per presented frame, but only a run with a frame-time overlay
in a GPU-bound game would put a number on it.

The anti-cheat rules in the README are enforced at build time: `clippy.toml`
bans the APIs that would break them, and the app crate denies those lints.

### Smart Life sign-in

The QR sign-in uses the partner id Home Assistant ships with Tuya's SDK
(`CLIENT_ID` in `src-tauri/src/tuya_cloud.rs`). A public release should
register its own with Tuya.

### Hue streaming

Pairing asks a bridge for its streaming key too (`generateclientkey`), kept
beside the username and as secret: sealed, blanked for the interface and
left out of exports. The lamps of a bridge with one take a seat each in a
single stream per bridge (`device/entertainment.rs`): Lueur's own
entertainment area, named *Lueur*, set up over the CLIP v2 API with every
lamp asking, then HueStream frames over DTLS 1.2 PSK (`device/dtls/`, on
`ring`, no new crate). The PSK identity is the `hue-application-id` the
bridge gives in a header of `GET /auth/v1`, which is what an area set up
over CLIP v2 streams as; the username, the older API's identity, is tried
next. A bridge that will not stream (another app streams, a lamp it cannot
stream to, a round bridge without HTTPS) leaves its lamps on the REST API,
and is tried again after half a minute.

UDP never says when the far end stopped listening, so a running stream is
watched two ways: the sender looks each second for a close_notify or the
OS's "port unreachable" (`DtlsClient::check`), and a watcher thread asks
the bridge every five seconds whether the area is still active and ours
(another app took it, the bridge restarted). Either way the stream is
marked broken, its lamps' streams fail, and `Relink` reopens them on the
REST API. Lamps identified or set by hand always use the REST API
(`device::unstreamed`): a still colour is no reason to stream, and when a
stream stops the bridge puts its lamps back as they were before it.

Over REST, a lamp that refuses `xy` (error 6: a White Ambiance lamp) is
sent the white nearest to each colour from then on, and one that also
refuses `ct` (a lamp that only dims) its brightness alone.

The DTLS client is checked against OpenSSL's server by tests that need
`openssl` on the `PATH`:

```bash
cargo test -p lueur-core dtls -- --include-ignored
```

The whole path, from mDNS to pairing, the REST API, the stream and the
bridge ending it, runs against [diyHue](https://github.com/diyhue/diyHue),
an emulated bridge, in `crates/lueur-core/tests/hue_emulator.rs`. Lueur
talks to no address of its own computer, so the emulator and the test each
run in a container on one Docker network (the file says how). diyHue
serves the stream with `openssl s_server`, so it accepts any PSK identity:
which one a real bridge wants is settled by what Hyperion and LedFx do, not
by this test. None of it has met a real bridge yet.

### Finding bridges and controllers

Hue bridges and Nanoleaf controllers are found over mDNS (`device/mdns.rs`):
a PTR query from an ephemeral port, which responders answer by unicast
("legacy unicast"), sent on every LAN interface in turn, since a multicast
leaves from one interface only and the default one is often a VPN's or a
virtual machine's. A paired bridge is also asked at its last address, in
case mDNS is filtered. To see what answers on a network:

```bash
cargo test -p lueur-core browses_the_real_network -- --ignored --nocapture
LUEUR_MDNS_SERVICES=_hue._tcp.local cargo test -p lueur-core browses_the_real_network -- --ignored --nocapture
```

### Nanoleaf streaming

A layout plays in External Control v2: one `effects` write, then UDP
frames to port 60222 (Light Panels take v2 too on a recent firmware). The
pieces without LEDs (the Rhythm module, controllers, connectors, caps) are
left out of the frame and the zones, by `shapeType`. The zones follow the
layout as it hangs: its positions (y going up) are turned back by the
angle the user gave it in the Nanoleaf app (`panelLayout/globalOrientation`,
anticlockwise, as openHAB reads it), then grouped along the longer side,
left to right or bottom to top (`LightStream::upright`). A controller leaves
External Control by itself when its app picks an effect, when the panels
are touched, or after a power cut, and frames sent then are lost; so a
keeper thread asks every five seconds what it plays (`effects/select`) and
writes External Control again when it is something else. When the keeper
gets no answer three times running, the stream fails and `Relink` takes
over. **Identify** sets the colour over REST, which answers, rather than
sending one datagram that could arrive before the controller listens.

### Hue and Nanoleaf credentials

A Hue username or a Nanoleaf token is all it takes to drive the lights, and
anything on the network can claim a bridge's or controller's id, in mDNS or
in the bridge's public `/api/config`. So the id alone opens nothing
(`Approach` in `device/mod.rs`):

- **Hue bridges are pinned, trust on first use.** `pair` records the
  SHA-256 of the certificate the bridge shows on port 443, provided what
  answers there over TLS reports the bridge's id. Nothing vouches for that
  certificate (Lueur ships no Signify root, and anyone can make one), so it
  is taken on the word of whatever answers the first time; from then on
  every request goes over TLS to that certificate only (`device/http.rs`, a
  rustls verifier that checks the fingerprint, and the handshake signature
  against it), never in the clear again. A bridge that does not answer
  over HTTPS is not paired, unless its `/api/config` says it is the round
  bridge (`BSB001`), which has no HTTPS. Pairing again a bridge whose
  certificate is pinned needs that same certificate (`pairing_cert` in
  `hue.rs`). A pairing made before pinning learns its certificate the
  first time discovery reaches the bridge over TLS at its paired address,
  where its username always went. A pinned bridge that moves is followed
  only if it shows its certificate at the new address.
- **The round bridge and Nanoleaf controllers** speak HTTP only. Their
  credential goes to the address it was granted at, nowhere else.
- Anything else, a paired id heard at another address without proof, or a
  pinned bridge showing another certificate, gets nothing: discovery lists
  it unpaired, `identity.rs` keeps it and emits `device-unverified`, and
  the Bulbs page asks the user, offering first to pair it again with its
  button. For a device with nothing pinned, `confirm_device` (which only
  accepts a device the last search listed, at the address it was heard at)
  moves the pairing on the user's word. A pinned bridge showing another
  certificate, or none, can only be an impostor or a reset bridge: it is
  never confirmed, and `forget_device` drops its pairing so it can be
  paired again.
- A binding only ever holds its device's credential at its pairing's
  address (`follow_pairings`, `restore_bindings` in `secrets.rs`).

Validity dates are not checked: the certificate is known by its
fingerprint. A bridge whose certificate changes (a factory reset, a
firmware that reissues it) has to be paired again.

### Sharing bulbs between computers

Two computers playing an effect on one bulb would fight over it: a
Yeelight has one music channel, which each would take back from the other
in turn (`Relink` reconnects within half a second), and every other family
would show both streams at once. Two things keep them apart.

- **Leases** (`device/lease.rs`, `peers.rs`). While an instance drives a
  bulb it broadcasts a claim on it to the LAN, UDP port 47474: at once,
  then every second, as JSON (`{"lueur":1,"id":…,"host":…,"claims":[{"device":…,"round":…,"since":…}]}`).
  An instance that drives nothing stays quiet. Each light's `Relink` goes
  through a `Gate` every frame; a bulb someone else holds is let go at once
  without settling it, and its light waits (`Link::Held`) until the claim
  is gone. Who holds a bulb is decided from the claims alone, the same way
  on every computer: highest `round`, then oldest `since`, then smallest
  instance id. *Take over* (`take_light`) claims a round above the holder.
  Claims of an instance not heard for 3.5 s lapse; a released claim stands
  1.5 s, so the effect that replaces it keeps the bulb; an instance that
  slept drops its claims and listens for 1.2 s before claiming again, as it
  does at launch. Announcements are not authenticated: a forged one can
  make a light wait, never drive a bulb, and *Take over* always wins.
- **Eviction** (`yeelight/connection.rs`). A reconnecting Yeelight first
  asks the bulb whether it is in music mode (`get_prop music_on`, asked
  again for most of a second, so a channel this instance just closed is not
  mistaken for someone else's). If it is, the open fails with
  `lease::taken`, the light waits (`Link::Taken`) and asks again every 4 s.
  This covers another app, or a Lueur older than the leases. Other families
  cannot tell.

Screen starting by itself, resuming at launch and moving lights between
effects never take a bulb: only the user's *Take over* does, and it then
opens the stream impolitely. Restoring hand-set colours and the sleep
timer skip the bulbs another computer holds.

## Building & running

### Prerequisites

- [Rust](https://rustup.rs/) 1.90+ and [Node.js](https://nodejs.org/) 20.19+ or 22.12+.
- Tauri CLI: `npm install` pulls it in as a dev dependency (`npm run tauri`).

**Linux** also needs the Tauri, audio and capture system libraries. One
command installs them on Debian/Ubuntu (24.04 or later), Fedora and Arch, and
is what the CI runs:

```bash
./scripts/linux-deps.sh
```

It installs exactly these:

```bash
# Debian / Ubuntu
sudo apt-get install build-essential file pkg-config clang libclang-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  libsoup-3.0-dev libssl-dev libxdo-dev libxcb1-dev libxrandr-dev libdbus-1-dev \
  libasound2-dev libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev
# Fedora
sudo dnf install gcc gcc-c++ make file pkgconf-pkg-config clang clang-devel \
  webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel \
  libsoup3-devel openssl-devel libxdo-devel libxcb-devel libXrandr-devel dbus-devel \
  alsa-lib-devel pipewire-devel wayland-devel mesa-libEGL-devel mesa-libgbm-devel
# Arch
sudo pacman -S --needed base-devel file pkgconf clang \
  webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg libsoup3 openssl xdotool \
  libxcb libxrandr dbus alsa-lib pipewire wayland libglvnd mesa
```

At run time, Wayland capture needs `xdg-desktop-portal` and the desktop's
portal backend (`-gnome`, `-kde`, `-wlr`…), which desktops install already.

### Develop

```bash
npm install
npm run tauri dev      # hot-reloading app window
```

The UI also runs in a plain browser for design work: `npm run dev`, then open
`http://localhost:1420/?demo` for a furnished sample room (add `&shared` to
see lights another computer drives), or `?fresh` for a
first launch with bulbs waiting on the network. The README screenshots are
taken from `?demo` at the app's default window size (1180×800, 2× scale).

### Logs

Lueur logs to stderr and to `logs/lueur.log` in its data folder (Settings,
*Open logs folder*). The file is capped at 2 MB: it then becomes
`lueur.old.log`, replacing the one before. `LUEUR_LOG` sets what is logged,
in `RUST_LOG`'s syntax (comma-separated `level`, `target` or
`target=level`; the `/regex` suffix is not supported). Without it, Lueur
logs from `info` up and its dependencies from `warn` up
(`warn,lueur=info`). Targets named alone are added to that default; a bare
level replaces it. Addresses appear from `debug` down, never a credential.

```bash
LUEUR_LOG=lueur=debug npm run tauri dev                  # Lueur in detail: searches, streams, power
LUEUR_LOG=lueur_core::device=trace npm run tauri dev     # and every frame sent to a bulb
LUEUR_LOG=lueur_core::device::wire=trace npm run tauri dev   # the frames, the rest as usual
LUEUR_LOG=debug npm run tauri dev                        # everything, dependencies included
```

At trace level each frame a stream sends is logged with its family, the
device id and its content: hex for binary protocols (LIFX, Nanoleaf), the
JSON or text as sent for the others (WiZ, Govee, Yeelight, Hue, the Tuya
DPs before encryption). The Hue username in a request path shows as
`***`. Off, tracing costs the frame loop one level check per frame sent,
and formats nothing. On PowerShell, set the variable first:
`$env:LUEUR_LOG = "lueur_core::device=trace"`.

### Build a release

```bash
npm run tauri build -- --no-bundle   # Windows: the self-installing .exe
npm run tauri build                  # Linux: AppImage, macOS: .app
```

The output lands in `target/release/`. Set `LUEUR_DATA_DIR` to keep settings
somewhere other than the user config dir while testing.

## Install and updates, under the hood

**Windows.** Started from anywhere, the executable copies itself to
`%LOCALAPPDATA%\Lueur`, adds a Start menu shortcut and an entry in *Installed
apps*, then relaunches from there. An older download never replaces a newer
install.

**Updates.** Each launch shows a small window while Lueur checks the latest
GitHub release; a new version is downloaded, its signature checked, installed,
and the app restarts into it. Without network it gives up after a few seconds.
While Lueur runs it checks again every hour, and a version found then waits
behind a *Restart* button in the title bar and an entry in the tray menu.
Development builds never update.

**No going back.** The Tauri updater verifies the minisign signature of the
downloaded file against the public key in `tauri.conf.json`, and installs
only a version newer than the running one. But the version it compares is
the one `latest.json` announces, and `latest.json` is not signed: alone, a
doctored manifest could pair `version: 99.0.0` with the URL and signature
of an old, genuine release. So each signature carries the version it was
made for in its trusted comment, which the signature covers
(`tauri signer sign --app-version`, checked by the release workflow), and
`plugins.updater.requireSignedVersion` makes the plugin refuse a file whose
signed version differs from the announced one. The announced version must be
strictly newer (`is_upgrade` in `updater.rs`), so the signed one is too.
Releases signed before v0.8.0 carry no version and are refused by v0.8.0 and
later, which is what we want: an update only ever goes forward.

## Releasing

Bump `version` in `Cargo.toml` (`[workspace.package]`) and `package.json` and
merge to `main`. When the version on `main` has no `v<version>` tag yet,
`.github/workflows/release.yml` checks both versions agree, runs the CI, builds
Windows, Linux and macOS, signs the update files in a job that compiles
nothing, and publishes the GitHub release, which creates the tag, with the
`latest.json` the updater reads and a `SHA256SUMS.txt` to check a first
download against. A failed release is retried from the workflow's "Run
workflow" button.

It needs two repository secrets, `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, from `npx tauri signer generate`, whose
public key goes in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`.

### Signing the Mac app

The published Mac app is not signed yet: Gatekeeper asks for the user's
go-ahead at first launch, and since each update is a different unsigned
binary, macOS may ask again for the Keychain item that holds the
credentials' key and for the microphone. The workflow signs and notarizes
it as soon as these repository secrets exist:

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | The *Developer ID Application* certificate and its key, exported as `.p12` and base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | The password of that `.p12` |
| `APPLE_SIGNING_IDENTITY` | Its name, e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID` | The Apple account that notarizes |
| `APPLE_PASSWORD` | An app-specific password of that account |
| `APPLE_TEAM_ID` | The team id |

The build job makes the app unsigned, as without them. With the first
three, a `macos-sign` job then signs it on a fresh Mac, the only job that
sees these secrets: it runs nothing from npm or the build, only Apple's
`security`, `codesign`, `notarytool` and `stapler`. It signs with the
hardened runtime and `src-tauri/Entitlements.plist` (the microphone, for
the Music effect); with all six it also notarizes and staples the ticket,
then makes the two Mac archives again from the signed app, before the
`sign` job signs the update files. Without them the release is built
exactly as before, with a warning. This path has not run on a real Mac
yet: check the first signed release's log (`codesign --verify`,
`stapler validate` and `spctl` run in it) and the app itself.
