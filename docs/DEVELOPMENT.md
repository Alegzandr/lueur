# Developing Lueur

Everything needed to build, change and release Lueur. For what the app does
and how to use it, see the [README](../README.md).

## Architecture

```
lueur/
├─ crates/lueur-core/     Pure, GUI-free, fully unit-tested Rust
│   ├─ color.rs           Colour types, linear-space averaging, smoothing
│   ├─ geometry.rs        Room model + light→screen-zone mapping
│   ├─ engine.rs          Frame → one colour per light
│   ├─ yeelight/          LAN protocol, SSDP discovery, music-mode sockets
│   ├─ device/            Every family behind one stream/power/identify API:
│   │                     WiZ, LIFX, Govee, Hue, Nanoleaf, Tuya DP encoding
│   └─ config.rs          Persisted settings
├─ src-tauri/             Desktop shell (Tauri v2)
│   ├─ capture.rs         Screen capture behind a FrameSource trait
│   │  └─ dxgi.rs        Windows: Desktop Duplication, GPU-downscaled
│   ├─ runner.rs          The live capture→compute→push loop
│   ├─ devices.rs         Dispatch between the core families and Tuya
│   ├─ tuya.rs            Tuya LAN control (rustuya), discovery by key
│   ├─ tuya_cloud.rs      Smart Life QR sign-in that imports local keys
│   ├─ commands.rs        Validated IPC surface
│   ├─ tray.rs            Tray menu: start/stop, open, quit
│   ├─ error.rs           Errors sent as codes the UI words in its language
│   ├─ install.rs         Windows: silent per-user install and uninstall
│   ├─ updater.rs         Startup splash and updates from GitHub releases
│   └─ lib.rs             Tray, autostart, single-instance, window handling
└─ src/                   Svelte 5 (runes) + TypeScript frontend
    ├─ lib/state.svelte.ts  App state and every action the UI can take
    ├─ lib/i18n.ts          English and French strings
    ├─ lib/views/           Lighting, Room, Bulbs, Settings
    ├─ lib/ui/              Shared controls (Slider, Switch, Segmented, Row…)
    └─ styles/app.css       Design tokens, light and dark themes
```

The security- and colour-critical logic lives in `lueur-core`, which has **no
GUI dependencies** and is covered by unit tests you can run without a display:

```bash
cargo test -p lueur-core
```

The device layer is a small trait, so new bulb families slot in without
touching the engine.

### Capture

On Windows the desktop is read through DXGI Desktop Duplication and shrunk on
the GPU, so a frame costs well under a millisecond and exclusive-fullscreen
games are captured; GDI is the fallback. macOS uses ScreenCaptureKit, Linux
X11 or PipeWire.

The anti-cheat rules in the README are enforced at build time: `clippy.toml`
bans the APIs that would break them, and the app crate denies those lints.

### Smart Life sign-in

The QR sign-in uses the partner id Home Assistant ships with Tuya's SDK
(`CLIENT_ID` in `src-tauri/src/tuya_cloud.rs`). A public release should
register its own with Tuya.

## Building & running

### Prerequisites

- [Rust](https://rustup.rs/) 1.90+ and [Node.js](https://nodejs.org/) 20.19+ or 22.12+.
- Tauri CLI: `npm install` pulls it in as a dev dependency (`npm run tauri`).

**Linux** also needs the usual Tauri + capture system libraries, e.g. on
Debian/Ubuntu:

```bash
sudo apt-get install \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
  libsoup-3.0-dev libssl-dev libxcb1-dev libxrandr-dev libdbus-1-dev \
  libpipewire-0.3-dev pkg-config build-essential
```

### Develop

```bash
npm install
npm run tauri dev      # hot-reloading app window
```

The UI also runs in a plain browser for design work: `npm run dev`, then open
`http://localhost:1420/?demo` for a furnished sample room, or `?fresh` for a
first launch with bulbs waiting on the network. The README screenshots are
taken from `?demo` at the app's default window size (1180×800, 2× scale).

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
While Lueur runs it checks again every six hours. Development builds never
update.

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
