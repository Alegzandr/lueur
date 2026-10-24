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
- 🖥️ **Exclusive fullscreen friendly.** On Windows the desktop is read through
  DXGI Desktop Duplication and shrunk on the GPU, so a frame costs well under a
  millisecond and fullscreen games are captured. macOS and Linux use their
  native APIs (ScreenCaptureKit / X11·PipeWire).
- 🪶 **Lightweight.** Built on Tauri (system webview, ~10 MB) rather than
  Electron.
- 📦 **Installs and updates itself.** On Windows the download is a single
  executable that installs itself silently for the current user, no admin
  rights, no wizard. Every launch checks GitHub for a new version and applies
  it before the app starts.
- 🤖 **Automatic.** On Windows, the ambilight starts by itself when a game or
  a video goes fullscreen and stops when it ends, handing each light back to
  what it showed before. Fullscreen is read from the shell's notification
  state (what Windows uses to hold notifications during a game): no window
  or process is ever inspected.
- 🎵 **Effects.** Music (colours change on the beat, brightness follows the
  loudness: on Windows Lueur listens to the computer's own sound, elsewhere
  the microphone), Rainbow, Sunset, Ocean and Candle. One click, nothing to
  tune.
- 💡 **Everyday lighting too.** Brightness, warm-to-cool white and colour for
  each light, and four moods (Daylight, Reading, Relax, Night light), from
  the window or the tray menu, so the phone app can stay in your pocket.
- 🔌 **Tray, autostart, start-minimised.** Close to tray, launch at login, and
  optionally auto-start the effect.
- 🔒 **Minimal attack surface.** Network traffic is LAN bulb control and
  discovery, the update check against GitHub (whose downloads are
  signature-checked) and, only while you import Smart Life bulbs, Tuya's
  sign-in API; bulbs are only ever reached at private LAN addresses; strict
  Content-Security-Policy; every IPC input is validated; bridge and bulb
  credentials never leave the backend, nor go into an exported config.
- 🛡️ **Anti-cheat safe.** Lueur reads the screen the way OBS or Discord screen
  share do, and nothing else. See [Competitive games and anti-cheat](#competitive-games-and-anti-cheat).

> **Supported bulbs:** Yeelight Color (tested), Smart Life / Tuya colour bulbs
> (tested on a v3.3 bulb), Philips Hue (through the bridge), WiZ, LIFX, Govee
> (LAN Control on) and Nanoleaf. Everything is driven over the LAN; the device
> layer is a small trait, so new families slot in without touching the engine.

### Smart Life / Tuya bulbs

Tuya bulbs only obey clients that know their *local key*, which the Smart Life
app keeps to itself. **Smart Life** on the Bulbs page asks for your user code
(Smart Life: Me → ⚙ → Account and Security → User Code) and shows a QR code;
scan it from the Smart Life app and confirm. Lueur then lists your homes'
colour bulbs, keeps each one's id and local key, and ends the session: no
account token is stored, and everything afterwards is local.

- Bulbs someone shared with you one by one are not visible to this sign-in.
  Ask their owner to add you to their home instead.
- Lueur finds the bulbs by their UDP announcements where the firewall lets
  them through, and otherwise by knocking on TCP 6668 across your /24 and
  trying the imported keys.
- The sign-in uses the partner id Home Assistant ships with Tuya's SDK
  (`CLIENT_ID` in `src-tauri/src/tuya_cloud.rs`). A public release should
  register its own with Tuya.

## The app

A plain, native-looking desktop layout in English or French (it follows the
system language, or pick one in Settings), with light and dark themes. A
sidebar holds four sections and a start/stop control that is always in
reach; the tray menu can start and stop the ambilight too, so you never have
to leave a game.

| Lighting | Room | Bulbs | Settings |
|---|---|---|---|
| The screen ambilight (brightness, response, wall colour), the effects and moods, and every light with its power, brightness and white or colour | A floor plan you draw on: drag lights, corners and the screen, drag *You* to turn the screen | Network search, *Add all*, assign a bulb to an existing light, rename | Ambilight in fullscreen, display, language, appearance and startup |

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

## Competitive games and anti-cheat

Lueur is built to run next to Vanguard, Easy Anti-Cheat, BattlEye, FACEIT and
the like without looking like a cheat:

- **Desktop capture only.** It reads whole monitors through DXGI Desktop
  Duplication (or GDI as a fallback), the same OS APIs OBS and Discord use. It
  never captures a game window directly.
- **Hands off the game.** No handle on the game process, no memory reads, no
  DLL injection, no hooks, no driver.
- **Output only.** No synthetic keyboard or mouse input and no global hotkeys:
  screen reading plus input is what pixel bots do. The only output is colour
  sent to bulbs on your LAN.
- **No overlay.** Nothing is ever drawn over the game.

These rules are enforced at build time: `clippy.toml` bans the APIs that would
break them, and the app crate denies those lints.

If a game hides itself from capture, the bulbs simply see a black screen;
switching the game to borderless-windowed usually fixes it. Lueur never tries
to get around that.

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

### Build a release

```bash
npm run tauri build -- --no-bundle   # Windows: the self-installing .exe
npm run tauri build                  # Linux: AppImage, macOS: .app
```

The output lands in `target/release/`. Settings live in a `lueur` folder in
your user config dir (`%APPDATA%`, `~/.config`, `~/Library/Application
Support`); set `LUEUR_DATA_DIR` to put them elsewhere. **Settings →
Configuration** exports them to a file and imports one back, to keep a copy
or move to another computer. The export holds no credential, so it is safe
to share: on another computer, pair the bridges and import Smart Life again.

## Install, updates and releases

**Windows.** The release is one executable. Started from anywhere, it copies
itself to `%LOCALAPPDATA%\Lueur`, adds a Start menu shortcut and an entry in
*Installed apps*, then relaunches from there, without asking anything. An
older download never replaces a newer install. Uninstalling from *Installed
apps* removes everything but your settings.

**Linux and macOS.** Download the AppImage or the `.app` (zipped) and run it;
updates replace that file or bundle in place.

**Updates.** Each launch shows a small window while Lueur checks the latest
GitHub release; a new version is downloaded, its signature checked, installed,
and the app restarts into it. Without network it gives up after a few seconds.
While Lueur runs it checks again every six hours, installs what it finds, and
offers a restart in the sidebar. Apart from the Smart Life
sign-in, this is the only traffic Lueur sends beyond your LAN, over HTTPS to
github.com. Development builds never update.

**Releasing.** Bump `version` in `Cargo.toml` (`[workspace.package]`) and
`package.json` and merge to `main`. When the version on `main` has no
`v<version>` tag yet, `.github/workflows/release.yml` checks both versions
agree, runs the CI, builds Windows, Linux and macOS, signs the update files in a job that
compiles nothing, and publishes the GitHub release, which creates the tag, with the
`latest.json` the updater reads and a `SHA256SUMS.txt` to check a first download
against. A failed release is retried from the
workflow's "Run workflow" button. It needs two repository secrets, `TAURI_SIGNING_PRIVATE_KEY`
and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, from `npx tauri signer generate`,
whose public key goes in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`.

## First-time setup in the app

1. **Bulbs**: Lueur searches the network on first visit. (Yeelight and
   Govee need **LAN Control** on in their app first; a Hue bridge or Nanoleaf
   asks you to press its button once; Smart Life bulbs need the sign-in
   above.) Add the bulbs you want; *Identify* flashes one so you know which is
   which.
2. **Room**: drag the **screen** to where your monitor is and drag the **You**
   marker to your seat to turn it; drag each **light** to its real position. The
   dotted lines and glows show what each light will mirror.
3. **Lighting**: pick a response and hit **Start ambilight**. Finer tuning lives
   in **Settings**.

## Roadmap

- Hue Entertainment streaming (DTLS) instead of the REST API, and WLED.
- Testing WiZ, LIFX, Govee, Hue and Nanoleaf on real hardware.
- Optional per-light manual zone override in the UI.
- Multi-monitor spanning and per-zone brightness curves.

## License

MIT — see [LICENSE](LICENSE).
