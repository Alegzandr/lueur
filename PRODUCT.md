# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

Desktop app: Svelte 5 in a Tauri v2 webview, on Windows, macOS and Linux. The
window is resizable with a 960 × 640 minimum; it is not a website and has no
mobile layout.

## Users

The heart of the audience is the PC player who games at night in a dim room:
they set the room up once, then mostly drive the lights from the tray without
leaving the game. The audience is wider than that, though: people watching
films, people listening to music (the Music effect), and people who simply
want one app to drive their smart bulbs, with effects, moods, per-light
power, brightness, white and colour, and a sleep timer.

Most are not lighting specialists. They own one or several brands of bulb,
know their phone app, and expect setup to take minutes.

## Product Purpose

Lueur turns the smart bulbs already in a room into a low-latency ambilight:
the light around you follows what's on screen. You sketch your room, place
the screen, the seat and each light where it really stands, and each light
mirrors the part of the picture that matches its place. Outside games, the
same window plays effects (Music, Rainbow, Sunset, Ocean, Candle) and moods
(Natural light, Daylight, Reading, Relax, Night light) on the whole room or
on chosen lights.

Success: the bulbs are found and placed in a few minutes, the light follows
the screen with no visible delay, and the user forgets the app is running.

## Positioning

Several things together, none ranked above the others:

- **The room plan.** Each light mirrors the slice of the picture that matches
  where it really stands (sideways position, height, distance), not a generic
  left/right zone.
- **Latency.** Colours reach the bulbs many times a second (Yeelight music
  mode, Hue Entertainment streaming), with no visible delay.
- **Local, free, many brands.** One open-source (MIT) app for Yeelight, Smart
  Life / Tuya, Philips Hue, WiZ, LIFX, Govee and Nanoleaf, driven over the
  local network with no cloud once a bulb is set up.
- **Safe next to anti-cheat.** Desktop capture only, the way OBS and Discord
  do it; no hooks, no injection, no overlay, no simulated input.

## Operating Context

- Used at night in a dim room, often while a fullscreen game or film runs;
  the window is opened briefly between sessions, the tray menu during them.
- Four pages: **Lighting** (Ambilight, effects, moods, per-light controls,
  sleep timer), **Room** (top-down floor plan to draw on), **Bulbs**
  (discovery, identify, pairing, Smart Life import), **Settings**.
- On Windows the Ambilight starts by itself when a game or video goes
  fullscreen and restores each light afterwards.
- Several computers in the house can run Lueur; a bulb driven by one shows
  that computer's name on the others, with *Take over*.
- Keyboard: Ctrl/⌘ + 1–4 switches page, Ctrl/⌘ + Enter starts or stops;
  on the plan, arrows nudge, Delete removes, Esc deselects, Shift snaps.
- Updates install themselves at launch, signed and forward-only.

## Capabilities and Constraints

- Tray app around 10 MB, installs without admin rights on Windows.
- Strict CSP: no remote scripts, styles or fonts; assets ship with the app.
- Light and dark appearance, chosen in Settings.
- Undo for removing a light and undo/redo on the room plan; changes made
  while effects play come with a one-click *Apply*.
- Configuration export/import holds no password or key.
- Hue, WiZ, LIFX, Govee and Nanoleaf are supported but not yet tested on
  hardware; only Yeelight Color and Smart Life / Tuya (v3.3) are tested.
- Roadmap, not shipped: WLED, per-light manual zone override, multi-monitor
  spanning, per-zone brightness curves.

## Brand Commitments

- Name: **Lueur**. Logo: `assets/logo.svg` (also `src/lib/ui/Logo.svelte`).
- Interface in English and French (`src/lib/i18n.ts`); brand names stay the
  same in every language.
- Voice: plain, concrete sentences that say what happens ("the light around
  you follows what's on screen"); no jargon, no superlatives, no hype. A bulb
  is called by its name or brand where its light is shown, never by its
  address.
- Tagline: "A low-latency ambilight for your smart bulbs, made for cosy night
  gaming."

## Evidence on Hand

- Screenshots of every page in light and dark: `docs/screenshots/`.
- The README's full feature, privacy and anti-cheat explanations.
- No testimonials, user counts, press, benchmarks or latency figures exist;
  do not invent any.

## Product Principles

- **Set up once, then disappear.** The window is for setup and choices; play
  happens in the game, with the tray and automatic start doing the rest.
- **The room is the model.** Every light decision follows from where things
  really stand; the plan is the source of truth, not abstract zones.
- **Say what happens, plainly.** Copy explains the effect on the lights, in
  either language, without technical vocabulary.
- **Local and honest.** Nothing leaves the network without reason, and the
  app states its limits (untested families, unencrypted credentials without
  a keyring) instead of hiding them.
- **Never get in the way of the game.** No overlay, no input, no capture
  tricks, no visible lag.

## Accessibility & Inclusion

- Photosensitivity guard on every mode's output (`crates/lueur-core/src/safety.rs`):
  white flashes can be turned off and never exceed three per second.
- `prefers-reduced-motion` is honoured by the animated backdrop and the
  animated tiles.
- Full keyboard use, including on the room plan.
- English and French with equal standing.
