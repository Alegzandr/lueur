# AGENTS.md

Guidance for coding agents working on Lueur. What the app does and why is in
[README.md](README.md); the architecture, logs and release process are in
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md). Read the relevant part before
changing something.

## Project at a glance

- `crates/lueur-core/`: pure Rust, no GUI, fully unit-tested. Colour, room
  geometry, the engine, the music stage, the photosensitivity guard, and
  every bulb family behind one API.
- `src-tauri/`: the Tauri v2 desktop shell. Screen capture, the live loops,
  the IPC commands, secrets, the tray.
- `src/`: the interface, Svelte 5 (runes) and TypeScript. Views in
  `src/lib/views/`, shared controls in `src/lib/ui/`, design tokens in
  `src/styles/app.css`.
- Adding a bulb family: follow [docs/ADDING_A_FAMILY.md](docs/ADDING_A_FAMILY.md).

## Commands

```bash
npm install
npm run tauri dev                  # the app, hot-reloading
npm run dev                        # the UI alone in a browser: http://localhost:1420/?demo (or ?fresh)
npm run build                      # svelte-check + vite build
npm test                           # Vitest
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --locked --all-targets -- -D warnings
```

CI runs all of the above (and `npm audit --omit=dev`); a change is done when
they pass.

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
  out of the UI and out of exported configurations (`src-tauri/src/secrets.rs`).
- **Two languages.** Every user-facing string goes in `src/lib/i18n.ts`, in
  both `en` and `fr`, with the same standing. Brand names don't translate.
- **Both themes.** Any new design token is defined for light, OS dark and
  forced dark in `src/styles/app.css`.

## Interface and design

- Who Lueur is for: [PRODUCT.md](PRODUCT.md). The visual system and its
  rules (amber only ever means light, nothing told by colour alone, a still
  alternative to every looping animation): [DESIGN.md](DESIGN.md).
- Reuse the tokens in `src/styles/app.css` and the controls in
  `src/lib/ui/` before writing new ones.
- A bulb is called by its name or brand where its light is shown, never by
  its address (`src/lib/brands.ts`).

## Writing style

- Copy, comments and docs use plain, concrete sentences that say what
  happens, without jargon or superlatives. Match the surrounding comments:
  they explain why, in full sentences.
- Commit messages follow `type: what changes, in plain words`
  (`feat:`, `fix:`, `refactor:`, `release:`), e.g.
  `fix: the music show follows a change of track`.
- When a change affects what the user sees or does, update the README too.
