# AGENTS.md

Guidance for coding agents working on Lueur. What the app does and why is in
[README.md](README.md); the architecture, logs and release process are in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Read the relevant part before
changing something.

## Project at a glance

Lueur is Rust only: four crates in one Cargo workspace (the fourth,
`crates/lueur-sign`, signs the update files at release time), no web view
and no npm step.

- `crates/lueur-core/`: pure Rust, no GUI, fully unit-tested. Colour, room
  geometry, the engine, the music stage, the photosensitivity guard, and
  every bulb family behind one API.
- `crates/lueur-shell/`: the desktop service, no GUI. Screen capture, the
  live loops, the bulbs, secrets, the background workers, the updates and
  the commands a window calls (`service.rs`), behind a `Host` the window
  implements.
- `crates/lueur-gpui/`: the window and the tray, drawn with GPUI over
  `lueur-shell`; it builds the `lueur` executable. Pages in
  `src/views/`, shared controls in `src/ui/`, design tokens in
  `src/theme.rs`, the splash and the hourly update check in `src/update.rs`.
- `locales/en.json` and `locales/fr.json`: every string the window shows.
- Adding a bulb family: follow [docs/ADDING_A_FAMILY.md](docs/ADDING_A_FAMILY.md).

## Commands

```bash
cargo run -p lueur-gpui                                 # the app
LUEUR_DATA_DIR=/tmp/lueur-try cargo run -p lueur-gpui   # the app on a settings folder of its own
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets -- -D warnings
cargo audit
```

A development build started with `LUEUR_DATA_DIR` never touches your
settings and skips the single-instance lock, so it runs beside the
installed Lueur. `LUEUR_START_VIEW` (`lighting`, `room`, `bulbs`,
`settings`) opens it on a page, and on Windows `scripts/dev/snap.ps1` saves
a picture of its window; see *Develop* in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

CI runs all of the checks above; a change is done when they pass.

## Rules that must hold

- **Anti-cheat.** Lueur only reads whole monitors through the OS capture
  APIs. Never open, read or hook another process, never capture a window,
  never synthesise input, never register global hotkeys, never draw over a
  game. `clippy.toml` bans the APIs that would break this; don't lift a ban
  without reading "Competitive games and anti-cheat" in the README.
- **Photosensitivity.** Every mode's output goes through the `FlashGuard` in
  `crates/lueur-core/src/safety.rs` (WCAG 2.3.1, at most three flashes a
  second). Never route light around it.
- **Privacy.** Bulbs are reached only at private addresses. The only outside
  traffic is the update check and the Smart Life sign-in. Credentials stay
  out of the UI and out of exported configurations (`crates/lueur-shell/src/secrets.rs`).
- **Two languages.** Every user-facing string goes in `locales/en.json` and
  `locales/fr.json`, under the same key, with the same standing. Brand
  names don't translate.
- **Both themes.** Any new design token is defined in both `Theme::light()`
  and `Theme::dark()` in `crates/lueur-gpui/src/theme.rs`; the system, or
  the appearance forced in Settings, picks between them. The room plan's
  own colours follow the same rule in
  `crates/lueur-gpui/src/views/room/colors.rs`.

## Interface and design

- Who Lueur is for: [PRODUCT.md](PRODUCT.md). The visual system and its
  rules (amber only ever means light, nothing told by colour alone, a still
  alternative to every looping animation): [DESIGN.md](DESIGN.md).
- Reuse the tokens in `crates/lueur-gpui/src/theme.rs` and the controls in
  `crates/lueur-gpui/src/ui/` before writing new ones.
- A bulb is called by its name or brand where its light is shown, never by
  its address (`crates/lueur-gpui/src/views/bulbs/brands.rs`).
- Every control is reachable with Tab and tells assistive technology what
  it is, what it is called and what state it is in, through GPUI's
  AccessKit integration (`.role()`, `.aria_label()` and the like). The
  controls in `crates/lueur-gpui/src/ui/` already do; a new one must too.

## Writing style

- Copy, comments and docs use plain, concrete sentences that say what
  happens, without jargon or superlatives. Match the surrounding comments:
  they explain why, in full sentences.
- Commit messages follow `type: what changes, in plain words`
  (`feat:`, `fix:`, `refactor:`, `release:`), e.g.
  `fix: the music show follows a change of track`.
- When a change affects what the user sees or does, update the README too.
