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
  lights where they actually are. Lueur works out which part of the screen each
  light should mirror, and how tightly — a lamp behind the panel tracks a slice,
  one across the room glows with the whole scene.
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

## The app

A plain, native-looking desktop layout in English or French (it follows the
system language, or pick one in Settings), with light and dark themes. A
sidebar holds four sections and a start/stop control that is always in
reach; the tray menu can start and stop the ambilight too, so you never have
to leave a game.

| Lighting | Room | Bulbs | Settings |
|---|---|---|---|
| Every light with its power and "follows the screen" switch, a response preset (Instant, Balanced, Calm), brightness and colour intensity | A floor plan you draw on: drag lights, corners and the screen, drag *You* to turn the screen | Network search, *Add all*, assign a bulb to an existing light, rename | Capture, colour, transitions, language, appearance and startup |

The first launch walks you through three steps (add bulbs, place them, start).
New bulbs land in a row behind the screen, the usual setup. Bulbs that get a
new address from the router are found again at launch. Removing a light can
be undone, and changes to the room while the ambilight runs come with a
one-click *Apply now*.

Keyboard: <kbd>Ctrl</kbd>/<kbd>⌘</kbd> + <kbd>1</kbd>–<kbd>4</kbd> switches
section, <kbd>Ctrl</kbd>/<kbd>⌘</kbd> + <kbd>Enter</kbd> starts or stops. On the
plan, arrows nudge the selection (<kbd>Shift</kbd> for bigger steps),
<kbd>Delete</kbd> removes it, <kbd>Esc</kbd> deselects, and holding
<kbd>Shift</kbd> while turning the screen snaps to 15°.

## How the room mapping works

The room is a top-down floor plan. The screen is a segment with a *facing*
direction (where you sit). For each light Lueur reads three things off the plan:

- **Sideways position** picks the horizontal slice of the picture it mirrors.
- **Height** (a per-light floor→ceiling slider) picks the vertical band.
- **Distance** widens that region continuously: tucked behind the panel a light
  mirrors a tight slice; out across the room it settles into the whole picture.
  Distance also dims it and slows its response, because indirect light that
  snaps at 60 Hz from the far wall reads as a glitch, not as atmosphere.

Zone edges are feathered rather than hard, so two neighbouring lights hand over
gradually when something bright crosses between them.

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
│   ├─ tray.rs            Tray menu: start/stop, open, quit
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
first launch with bulbs waiting on the network.

### Build a release / portable binary

```bash
npm run tauri build
```

The bundle appears under `src-tauri/target/release/` (and installers under
`.../bundle/`). The plain executable is portable: copy it anywhere and it keeps
its settings in a `lueur-data` folder beside it (falling back to your user config
dir if that location isn't writable).

## First-time setup in the app

1. **Bulbs**: Lueur searches the network on first visit. (Enable **LAN
   Control** for each bulb in the official Yeelight app first.) Add the bulbs
   you want; *Identify* flashes one so you know which is which.
2. **Room**: drag the **screen** to where your monitor is and drag the **You**
   marker to your seat to turn it; drag each **light** to its real position. The
   dotted lines and glows show what each light will mirror.
3. **Lighting**: pick a response and hit **Start ambilight**. Finer tuning lives
   in **Settings**.

## Roadmap

- More bulb families (Philips Hue Entertainment, WLED, LIFX, Govee LAN…).
- Optional per-light manual zone override in the UI.
- Multi-monitor spanning and per-zone brightness curves.

## License

MIT — see [LICENSE](LICENSE).
