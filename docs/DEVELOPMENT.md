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
├─ crates/lueur-shell/    The desktop service, no GUI: what the window calls
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
│   ├─ update.rs          Updates from GitHub releases, checked as the Tauri updater checks them
│   ├─ service.rs         The commands a window calls, validated
│   ├─ tray_model.rs      What the tray menu shows and what its items do
│   ├─ secrets.rs         Credentials kept from the UI and from exports
│   ├─ identity.rs        Paired bridges heard where nothing vouches for them
│   ├─ peers.rs           Talks with other computers running Lueur (LAN)
│   ├─ vault.rs           Credentials sealed on disk, the key in the OS keyring
│   ├─ autostart.rs       Launch at login, the same entry the Tauri plugin wrote
│   ├─ error.rs           Errors sent as codes the UI words in its language
│   ├─ logging.rs         Logs to stderr and a capped file, set by LUEUR_LOG
│   ├─ install.rs         Windows: silent per-user install and uninstall
│   ├─ state.rs, paths.rs Shared app state; where the data folder is
│   └─ lib.rs             Shell, the events it raises and the Host that hears them
├─ crates/lueur-slint/    The window and the tray, drawn with Slint over
│   │                     lueur-shell; builds the `lueur` executable
│   ├─ ui/                The interface, in .slint files compiled by build.rs:
│   │  ├─ app.slint      The window: the sidebar, the page shown, the toasts,
│   │  │                 Ctrl+1 to 4 and Ctrl+Enter
│   │  ├─ shell.slint    The sidebar, the toasts, the backdrops, a page's column
│   │  ├─ controls.slint Shared controls (button, switch, slider, segmented,
│   │  │                 select, text field, row, card, tip)
│   │  ├─ theme.slint    The tokens' names, spaces, type sizes, motion
│   │  ├─ tr.slint       The words of locales/, through src/i18n.rs
│   │  ├─ splash.slint   The update splash
│   │  └─ pages/         Lighting, Room, Bulbs, Settings: each a global the
│   │                    Rust fills, and the page that draws it
│   ├─ src/main.rs, app.rs Start-up, the shell's events, the tray's picks,
│   │                     what closing the window does, restarting
│   ├─ src/rt.rs          The window's thread: the state, jobs sent off it,
│   │                     one refresh per turn of the loop
│   ├─ src/state.rs       What the window shows, the actions every page shares
│   ├─ src/window.rs      The sidebar, the update button, the toasts
│   ├─ src/pages/         A module per page: fills its global, answers its
│   │                     callbacks, keeps what only it needs
│   ├─ src/models.rs      Lists changed in place, so rows keep focus and hover
│   ├─ src/tray.rs        The tray (tray-icon), drawn from lueur_shell::tray_model
│   ├─ src/single.rs      One instance at a time, under the Tauri plugin's names
│   ├─ src/update.rs      Startup splash and the hourly check, over lueur_shell::update
│   ├─ src/host.rs        The shell's events, carried to the window's thread
│   ├─ src/theme.rs       Design tokens in OKLCH, light and dark
│   ├─ src/i18n.rs        The strings of locales/, by key, in the current language
│   ├─ src/images.rs      The room's photographs (WebP), decoded and blurred once
│   └─ assets/            Inter, the Phosphor icons, the mode photographs,
│                         built into the binary by build.rs
└─ locales/               en.json and fr.json: every string the window shows
```

The security- and colour-critical logic lives in `lueur-core`, which has **no
GUI dependencies** and is covered by unit tests you can run without a display:

```bash
cargo test -p lueur-core
```

The device layer is a small trait, so new bulb families slot in without
touching the engine. [ADDING_A_FAMILY.md](ADDING_A_FAMILY.md) walks through
everything a new family touches, with WLED as the example.

The window crate keeps what its pages work out (what each light does, the
plan's arithmetic, the choices Settings offers) in plain functions apart
from drawing them, tested without a window:

```bash
cargo test -p lueur-slint
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
cargo run --release -p lueur-shell --example capture_bench -- --help
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
bans the APIs that would break them, and `lueur-shell` and `lueur-slint` deny
those lints.

### Smart Life sign-in

The QR sign-in uses the partner id Home Assistant ships with Tuya's SDK
(`CLIENT_ID` in `crates/lueur-shell/src/tuya_cloud.rs`). A public release should
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

- [Rust](https://rustup.rs/) 1.92+. Lueur is Rust only: there is no npm step
  to build, run, sign or release it.

**Linux** also needs the Slint, audio and capture system libraries. One
command installs them on Debian/Ubuntu (24.04 or later), Fedora and Arch, and
is what the CI runs:

```bash
./scripts/linux-deps.sh
```

It installs exactly these:

```bash
# Debian / Ubuntu
sudo apt-get install build-essential file pkg-config clang libclang-dev \
  libssl-dev libxcb1-dev libxrandr-dev libdbus-1-dev libasound2-dev \
  libpipewire-0.3-dev libwayland-dev libegl-dev libgbm-dev \
  libfontconfig-dev libxkbcommon-x11-dev libx11-xcb-dev
# Fedora
sudo dnf install gcc gcc-c++ make file pkgconf-pkg-config clang clang-devel \
  openssl-devel libxcb-devel libXrandr-devel dbus-devel alsa-lib-devel \
  pipewire-devel wayland-devel mesa-libEGL-devel mesa-libgbm-devel \
  fontconfig-devel libxkbcommon-x11-devel libX11-devel
# Arch
sudo pacman -S --needed base-devel file pkgconf clang \
  openssl libxcb libxrandr dbus alsa-lib pipewire wayland libglvnd mesa \
  fontconfig libxkbcommon-x11 libx11
```

At run time, Wayland capture needs `xdg-desktop-portal` and the desktop's
portal backend (`-gnome`, `-kde`, `-wlr`…), which desktops install already.

Linux has no tray yet: `tray-icon` needs a GTK loop of its own, which the
app does not run. There, closing the window quits, and Lueur never starts
hidden. One instance at a time still holds, over the session bus, under the
D-Bus name the Tauri plugin used (`com.lueur.app.SingleInstance`, in
`crates/lueur-slint/src/single.rs`).

### Develop

```bash
cargo run -p lueur-slint
LUEUR_DATA_DIR=/tmp/lueur-try cargo run -p lueur-slint
```

The first reads your settings folder and drives your bulbs. The second
keeps its settings in a folder of its own, so a change can be tried without
touching them; copy `scripts/dev/sample-config.json` there as `config.json`
for a furnished sample room. A development build started with
`LUEUR_DATA_DIR` skips the single-instance lock, so it runs beside the
installed Lueur and beside other test runs. Without it, a second launch
only brings the running Lueur's window up, the Tauri versions' included.

Two more variables work in development builds only:

- `LUEUR_START_VIEW` opens the window on a page: `lighting`, `room`,
  `bulbs` or `settings`.
- `LUEUR_SPLASH_DEMO` shows the startup splash going through a pretend
  update. It downloads and installs nothing.

On Windows, `scripts/dev/snap.ps1` starts a development build on a settings
folder of its own, saves a picture of its window and stops it, to check a
change by eye. It draws that window on the CPU (`SLINT_BACKEND=winit-software`):
the pixels of an OpenGL window read back white.

```powershell
cargo build -p lueur-slint
./scripts/dev/snap.ps1 -Exe target/debug/lueur.exe -DataDir $env:TEMP\lueur-snap -Out shot.png -View room
```

The README screenshots are taken at the app's default window size
(1180×800, 2× scale).

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
LUEUR_LOG=lueur=debug cargo run -p lueur-slint                 # Lueur in detail: searches, streams, power
LUEUR_LOG=lueur_core::device=trace cargo run -p lueur-slint    # and every frame sent to a bulb
LUEUR_LOG=lueur_core::device::wire=trace cargo run -p lueur-slint  # the frames, the rest as usual
LUEUR_LOG=debug cargo run -p lueur-slint                       # everything, dependencies included
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
cargo build --release --locked -p lueur-slint  # target/release/lueur(.exe)
./scripts/package-linux.sh <version> <out dir>  # Linux: the AppImage, from that build
./scripts/package-macos.sh <version> <out dir>  # macOS: the universal Lueur.app and its archives
```

On Windows the executable is the release file as it is: it installs itself
on first launch. `package-linux.sh` makes the AppImage with linuxdeploy,
pinned to one release and checked against its checksum, and the desktop
file and icon in `packaging/linux/`. `package-macos.sh` builds for Apple
silicon and for Intel itself (`rustup target add aarch64-apple-darwin
x86_64-apple-darwin` first), joins the two into one `Lueur.app` with
`packaging/macos/Info.plist` and its icon, and makes the `.app.tar.gz` (the
update file) and the `.zip` (the first download).

Set `LUEUR_DATA_DIR` to keep settings somewhere other than the user config
dir while testing.

## Install and updates, under the hood

**Windows.** Started from anywhere, the executable copies itself to
`%LOCALAPPDATA%\Lueur`, adds a Start menu shortcut and an entry in *Installed
apps*, then relaunches from there. An older download never replaces a newer
install. The installed copy is `lueur-app.exe`, not `lueur.exe`: Overwolf
lists a game under that name and matches it by name alone, so Outplayed took
Lueur for a game. An install from 0.18 or earlier, still `lueur.exe`, copies
itself to the new name on its first launch of a later version, which then
remakes the Start menu shortcut and deletes the old file. Development builds
keep Cargo's `lueur.exe`.

**Updates.** Each launch shows a small window while Lueur checks the latest
GitHub release; a new version is downloaded, its signature checked, installed,
and the app restarts into it. Without network it gives up after a few seconds.
While Lueur runs it checks again every hour; a version found then is
installed and waits behind a *Restart* button at the top of the window and
an entry in the tray menu. Development builds never update. The splash and
the hourly check are `crates/lueur-slint/src/update.rs`; the check, the
download, the signature and the install per platform are
`crates/lueur-shell/src/update.rs`.

**No going back.** The minisign signature of the downloaded file is checked
against the public key the release signs with (`PUBKEY` in
`crates/lueur-shell/src/update.rs`), and only a version newer than the
running one is installed. But the version compared is the one `latest.json`
announces, and `latest.json` is not signed: alone, a doctored manifest could
pair `version: 99.0.0` with the URL and signature of an old, genuine
release. So each signature carries the version it was made for in its
trusted comment, which the signature covers (`lueur-sign
--app-version`, checked by the release workflow), and a file whose signed
version differs from the announced one is refused. The announced version
must be strictly newer (`is_upgrade` in `update.rs`), so the signed one is
too. Releases signed before v0.8.0 carry no version and are refused by
v0.8.0 and later, which is what we want: an update only ever goes forward.

**From the Tauri versions.** Up to 0.18, Lueur was a Tauri app that
updated itself with the Tauri updater plugin, which checks the same things
(`requireSignedVersion` is its name for the signed version). `update.rs`
reads the same `latest.json` at the same address, picks the same platform
entry, checks the same key and installs the same files the same way, and
the release keeps the names and shapes those versions expect: the Windows
executable, the AppImage, a `Lueur.app` with the identifier
`com.lueur.app`. So an installed Tauri version updates into this one, and
this one keeps updating from releases made the same way. Its tests check
signatures made by `tauri signer sign` and by `lueur-sign`, which writes
the very same thing. The single-instance lock
(`crates/lueur-slint/src/single.rs`) and the login entry
(`crates/lueur-shell/src/autostart.rs`) also keep the Tauri plugins' names:
the two versions recognise each other as one app, and the login entry an
older version wrote still starts the new one.

## Releasing

Bump `version` in `Cargo.toml` (`[workspace.package]`) and merge to `main`.
When the version on `main` has no `v<version>` tag yet,
`.github/workflows/release.yml` runs the CI, builds Windows, Linux and
macOS as under [Build a release](#build-a-release), signs the update files
in a job that compiles nothing, and publishes the GitHub release, which
creates the tag, with the `latest.json` the updater reads
(`scripts/latest-json.sh`) and a `SHA256SUMS.txt` to check a first download
against. A failed release is retried from the workflow's "Run workflow"
button. The CI also makes the AppImage and the Mac app on every pull
request, neither signed nor published, so a change to their packaging shows
before a release depends on it.

The update files are signed by `crates/lueur-sign`, which writes exactly
what the Tauri CLI's signer wrote (a prehashed minisign signature, base64
encoded, whose trusted comment names the version), so every installed copy,
Tauri ones included, checks them as before. The signing job builds it from
the lockfile before the key is in reach; only its signing step is handed
the key. It reads two repository secrets, still named
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the
key the Tauri CLI generated, whose public key is `PUBKEY` in
`crates/lueur-shell/src/update.rs`. Changing that key would strand every
installed copy.

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
hardened runtime and `packaging/macos/Entitlements.plist` (the microphone, for
the Music effect); with all six it also notarizes and staples the ticket,
then makes the two Mac archives again from the signed app, before the
`sign` job signs the update files. Without them the release is built
exactly as before, with a warning. This path has not run on a real Mac
yet: check the first signed release's log (`codesign --verify`,
`stapler validate` and `spctl` run in it) and the app itself.
