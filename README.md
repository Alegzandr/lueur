<div align="center">

# Lueur

**A low-latency ambilight for your smart bulbs — made for cosy night gaming.**

Draw your room, place your screen and lights, pick your bulbs, and the light
around you follows what's on screen.

*Lightweight · Portable · Windows / macOS / Linux*

</div>

---

## What it does

- 🎮 **Real ambilight, low latency.** Screen capture → per-light colour → bulbs,
  many times a second. Yeelight bulbs are driven in **music mode** so there's no
  command rate limit and no visible lag.
- 🛋️ **Room-aware.** Instead of assuming lights are stuck to your monitor, you
  sketch your room, place the screen (with the direction you sit) and drop the
  lights where they actually are. Lueur works out which slice of the screen each
  light should mirror.
- 🖥️ **Exclusive fullscreen friendly.** Uses each platform's native capture API
  (Windows Graphics Capture / ScreenCaptureKit / X11·PipeWire), which captures
  fullscreen games.
- 🪶 **Lightweight & portable.** Built on Tauri (system webview, ~10 MB) rather
  than Electron. Settings live in a `lueur-data` folder next to the executable —
  run it from a USB stick and leave no trace.
- 🔌 **Tray, autostart, start-minimised.** Close to tray, launch at login, and
  optionally auto-start the effect.
- 🔒 **Minimal attack surface.** The only network traffic is LAN bulb control and
  SSDP discovery; strict Content-Security-Policy; every IPC input is validated.

> **Supported bulbs today:** Yeelight Color (v2). More models will be added as
> they can be tested — the device layer is a small trait, so new bulbs slot in
> without touching the engine.

## Screens

| Room editor | Lights | Settings |
|---|---|---|
| Draw walls, place the screen & lights | Discover and bind bulbs | Tune latency, look & startup |

## How the room mapping works

The room is a top-down floor plan. The screen is a segment with a *facing*
direction (where you sit). For each light Lueur computes, from its position:

- **Left of the screen** → mirrors the screen's **left edge**.
- **Right of the screen** → mirrors the **right edge**.
- **Behind the screen** → mirrors the **vertical slice** directly behind it.

Colours are averaged in linear light (so reds and blues stay vivid, not muddy),
smoothed over time (a slider trades latency for calm), then shaped for
brightness/saturation before being pushed to the bulb.

## Architecture

```
lueur/
├─ crates/lueur-core/     Pure, GUI-free, fully unit-tested Rust
│   ├─ color.rs           Colour types, linear-space averaging, smoothing
│   ├─ geometry.rs        Room model + light→screen-zone mapping
│   ├─ engine.rs          Frame → one colour per light
│   ├─ yeelight/          LAN protocol, SSDP discovery, music-mode sockets
│   └─ config.rs          Persisted settings + portable data-dir resolution
├─ src-tauri/             Desktop shell (Tauri v2)
│   ├─ capture.rs         xcap screen capture behind a FrameSource trait
│   ├─ runner.rs          The live capture→compute→push loop
│   ├─ commands.rs        Validated IPC surface
│   └─ lib.rs             Tray, autostart, single-instance, window handling
└─ src/                   Svelte 5 + TypeScript frontend (cosy dark UI)
```

The security- and colour-critical logic lives in `lueur-core`, which has **no
GUI dependencies** and is covered by unit tests you can run without a display:

```bash
cargo test -p lueur-core
```

## Building & running

### Prerequisites

- [Rust](https://rustup.rs/) (stable) and [Node.js](https://nodejs.org/) 18+.
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

### Build a release / portable binary

```bash
npm run tauri build
```

The bundle appears under `src-tauri/target/release/` (and installers under
`.../bundle/`). The plain executable is portable: copy it anywhere and it keeps
its settings in a `lueur-data` folder beside it (falling back to your user config
dir if that location isn't writable).

## First-time setup in the app

1. **Lights** tab → *Scan network*. (Enable **LAN Control** for each bulb in the
   official Yeelight app first.) Add the bulbs you want.
2. **Room** tab → drag the **screen** to where your monitor is and rotate it to
   face your seat; drag each **light** to its real position. The dashed lines
   show what each light will mirror.
3. Pick your monitor and tune latency/brightness in **Settings**.
4. Hit **Start ambilight**. 🌙

## Roadmap

- More bulb families (Philips Hue Entertainment, WLED, LIFX, Govee LAN…).
- Optional per-light manual zone override in the UI.
- Multi-monitor spanning and per-zone brightness curves.

## License

MIT — see [LICENSE](LICENSE).
