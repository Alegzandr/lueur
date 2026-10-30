---
name: Lueur
description: One app for all your smart bulbs, whatever the brand, with care in every detail.
colors:
  paper: "oklch(0.972 0 0)"
  paper-sidebar: "oklch(0.948 0 0)"
  surface: "oklch(1 0 0)"
  surface-2: "oklch(0.962 0 0)"
  surface-hover: "oklch(0.935 0 0)"
  sunken: "oklch(0.955 0 0)"
  line: "oklch(0.9 0 0)"
  line-strong: "oklch(0.83 0 0)"
  text: "oklch(0.19 0 0)"
  text-2: "oklch(0.4 0 0)"
  text-3: "oklch(0.49 0 0)"
  ink: "oklch(0.27 0 0)"
  on-ink: "oklch(0.99 0 0)"
  filament-amber: "oklch(0.76 0.15 70)"
  filament-amber-text: "oklch(0.52 0.12 62)"
  active: "oklch(0.58 0.13 62)"
  active-text: "oklch(0.5 0.12 60)"
  select: "oklch(0.34 0 0)"
  focus: "oklch(0.6 0.13 64)"
  ok: "oklch(0.58 0.14 150)"
  danger: "oklch(0.55 0.19 27)"
  zone-left: "oklch(0.42 0.14 262)"
  zone-center: "oklch(0.51 0.1 175)"
  zone-right: "oklch(0.625 0.145 58)"
  night-bg: "oklch(0.205 0 0)"
  night-sidebar: "oklch(0.18 0 0)"
  night-surface: "oklch(0.24 0 0)"
  night-line: "oklch(0.31 0 0)"
  night-text: "oklch(0.95 0 0)"
  night-text-2: "oklch(0.77 0 0)"
  night-filament-amber: "oklch(0.8 0.14 72)"
typography:
  title:
    fontFamily: "system-ui, -apple-system, Segoe UI Variable Text, Segoe UI, Roboto, Ubuntu, Cantarell, Helvetica Neue, sans-serif"
    fontSize: "1.375rem"
    fontWeight: 650
    lineHeight: 1.15
    letterSpacing: "-0.018em"
  heading:
    fontFamily: "system-ui, -apple-system, Segoe UI Variable Text, Segoe UI, Roboto, Ubuntu, Cantarell, Helvetica Neue, sans-serif"
    fontSize: "0.9375rem"
    fontWeight: 600
  body:
    fontFamily: "system-ui, -apple-system, Segoe UI Variable Text, Segoe UI, Roboto, Ubuntu, Cantarell, Helvetica Neue, sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 400
    lineHeight: 1.45
    fontFeature: "\"cv11\", \"ss01\""
  label:
    fontFamily: "system-ui, -apple-system, Segoe UI Variable Text, Segoe UI, Roboto, Ubuntu, Cantarell, Helvetica Neue, sans-serif"
    fontSize: "0.75rem"
    fontWeight: 600
  caption:
    fontFamily: "system-ui, -apple-system, Segoe UI Variable Text, Segoe UI, Roboto, Ubuntu, Cantarell, Helvetica Neue, sans-serif"
    fontSize: "0.6875rem"
    fontWeight: 600
rounded:
  sm: "5px"
  md: "7px"
  lg: "10px"
  full: "999px"
spacing:
  s-1: "4px"
  s-2: "8px"
  s-3: "12px"
  s-4: "16px"
  s-5: "20px"
  s-6: "28px"
  s-7: "40px"
  s-8: "56px"
components:
  button:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
    padding: "0 12px"
    height: "28px"
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.on-ink}"
    rounded: "{rounded.md}"
    padding: "0 12px"
    height: "28px"
  button-plain:
    backgroundColor: "transparent"
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    height: "28px"
  button-plain-hover:
    backgroundColor: "{colors.surface-hover}"
    textColor: "{colors.text}"
  button-danger:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.danger}"
    rounded: "{rounded.md}"
    height: "28px"
  button-sm:
    rounded: "6px"
    padding: "0 8px"
    height: "24px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
    padding: "0 8px"
    height: "28px"
  switch-off:
    backgroundColor: "{colors.line-strong}"
    rounded: "{rounded.full}"
    width: "32px"
    height: "19px"
  switch-on:
    backgroundColor: "{colors.filament-amber}"
    rounded: "{rounded.full}"
    width: "32px"
    height: "19px"
  segmented:
    backgroundColor: "{colors.surface-hover}"
    rounded: "{rounded.md}"
    padding: "2px"
  card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.lg}"
  row:
    padding: "0 16px"
    height: "44px"
  nav-item:
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    padding: "0 8px"
    height: "36px"
  nav-item-active:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text}"
  effect-tile:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.lg}"
    padding: "8px"
  zone-badge:
    textColor: "#ffffff"
    rounded: "{rounded.full}"
    size: "15px"
---

# Design System: Lueur

## Overview

**Creative North Star: "The Dim Room"**

Lueur's window is a room with the lights off. Everything that is furniture
(the sidebar, the cards, the controls, the type) is a neutral grey, so the
only colour you see is light: the glow of a running effect, a bulb that is on,
the pools behind the Lighting view that show what each lamp is giving out.
The interface keeps quiet because what matters happens on the walls around
the screen, and it is often opened at night, between two games, in that same
dim room.

The system is plain and native in spirit: the platform's own typeface, small
type at a desktop density, hairline edges. Polish comes from the details rather
than decoration: controls with a faint lit top edge and a real pressed state,
motion that is short and physical, numbers that line up. Light and dark are
equal citizens; dark is not a filter on light but its own tuned set of values
(deeper shadows, brighter amber, a softer backdrop).

`src/styles/app.css` is the source of truth. Its header comment states the
doctrine this file expands on.

**Key Characteristics:**
- Neutral chrome (zero chroma greys) in both themes; amber is the one colour.
- Amber always means light. Neutral ink means "picked". Red means error or
  destruction. Nothing is ever told by colour alone.
- System font, 13px body, desktop density (28px controls, 44px rows).
- Depth from hairlines and soft contact shadows, never from heavy drop shadows.
- Short, eased motion (140 ms) and reduced motion honoured everywhere.
- The bulbs' real colours appear as content (backdrop, glows, effect previews),
  never as chrome.

## Colors

A greyscale room lit by one warm filament, with three plan zone hues kept for
the floor plan alone. All values are OKLCH; every chrome grey has zero chroma.

### Primary
- **Filament Amber** (`filament-amber`, dark: `night-filament-amber`): the
  colour of light itself. Switches that are on, the selected nav icon, a
  bulb's lamp dot, the selection on the plan, text selection, the caret.
  **Filament Amber Text** is its darker sibling for amber words on paper.
- **Active Amber** (`active`, `active-text`): the state ring of something
  running (an effect tile playing, a mood applied), tuned for 3:1 against
  every surface. Its soft wash (16 % alpha) backs amber text at 4.5:1.
- **Focus Amber** (`focus`): the keyboard focus outline, amber like the rest
  of the chrome, 3:1 on every surface.

### Secondary
- **Plan zones** (`zone-left` blue, `zone-center` teal, `zone-right`
  orange): which part of the screen a light mirrors. Each is a step apart in
  lightness so they stay distinct with red-green colour blindness, and each
  zone also carries a letter. No red, because red reads as an error. Used on
  the Room page and zone badges only.

### Neutral
- **Paper** (`paper`) / **Night** (`night-bg`): the page behind everything.
- **Sidebar Grey** (`paper-sidebar` / `night-sidebar`): one step darker than
  the page, so the navigation recedes.
- **Surface** (`surface` / `night-surface`): cards, rows, fields, popovers.
  `surface-2`, `surface-hover` and `sunken` are single steps around it for
  disabled fills, hover and recessed wells (effect previews, the plan).
- **Lines** (`line`, `line-strong`): row dividers and control edges.
- **Text** (`text`, `text-2`, `text-3`): primary, secondary, tertiary ink.
  Group titles and hints sit on `text-3`.
- **Ink** (`ink`, `on-ink`): the near-black primary button in light, the
  near-white one in dark. **Select** (`select`) is the neutral ink of a target
  or selection ("picked", not "running").
- **OK / Danger** (`ok`, `danger`): success and errors or destructive actions,
  nothing else.

### Named Rules
**The Amber Means Light Rule.** Amber marks only something giving light now,
or waiting on you. A choice, a selection or a target is shown in neutral ink
(`select`), so "picked" never reads as "running".

**The Never Colour Alone Rule.** A running state also carries a lamp dot or
words ("Playing"); a picked one a tick or an edge; a plan zone a letter.

**The No Pure Black on Paper Rule.** Light-theme shadows stay faint and
nothing is drawn in pure black: deep shadows and near-black parts belong to the
dark theme and give it away.

## Typography

**Body Font:** the platform's own UI face (`system-ui`, Segoe UI Variable on
Windows, SF on macOS, Cantarell/Ubuntu on Linux), with `cv11` and `ss01`
features on.

**Character:** no brand face. Lueur should feel like it belongs to the OS it
runs on; hierarchy comes from size and weight steps, not a second family.

### Hierarchy
- **Title** (650, 22px, line-height 1.15, -0.018em): the one page title per
  view ("Lighting", "Room").
- **Heading** (600, 15px): the app name in the sidebar, dialog titles.
- **Body** (400, 13px, 1.45): rows, buttons (500), fields, nav.
- **Label** (600, 12px, `text-3`): group titles above cards ("Effects",
  "Moods"), small buttons, tooltips at 400.
- **Caption** (600, 11px): count badges and the tiniest metadata.

### Named Rules
**The Lined-Up Numbers Rule.** Every changing number (percentages, minutes,
degrees, addresses) uses tabular figures (`.num`), so values don't jitter as
they change.

## Layout

A fixed 200px sidebar on the left, the page on the right in a single scroll
column, max 820px wide, padded 28px top and bottom and 40px on the sides
(same room below as above, so a page that fits never scrolls a few pixels).
The Room page breaks the column: the plan takes the width and an inspector
column sits on the right.

Pages stack **groups**: a small `text-3` group title, then a card. Groups are
28px apart. Inside a card, rows are 44px minimum, padded 16px, split by 1px
hairlines; the control sits on the right in a column of at most 320px or half
the row. The Lighting page is the one exception to the 820px column: up to
1240px wide, its lights and scenes sit on the left and the settings of the
lights picked on the right once the window is 1200px wide, one column below
that. Its mode grids count their columns from the room they have: still
modes in 3 or 5, animated ones in 3 or 6, at 8px gaps.

Spacing comes from an 8-step scale (4, 8, 12, 16, 20, 28, 40, 56). The window
is never narrower than 960 × 640, so there are no mobile breakpoints.

## Elevation & Depth

A hybrid of tonal layering and hairline-plus-contact shadows. Surfaces are
nearly flat; what lifts them is a 1px ring (0.5px in dark) and a short, soft
shadow, plus a 1px lit top edge on raised controls. Only floating things
(tooltips, menus, toasts) get a longer shadow.

### Shadow Vocabulary
- **Control edge** (`--control-edge`): hairline ring and 1–2px contact shadow
  on buttons, fields, selects, the segmented thumb.
- **Control highlight** (`--control-hi`, `inset 0 1px 0`): the lit top edge of
  a raised control.
- **Card edge** (`--card-edge`): ring plus a soft 4/12px lift for cards, tiles
  and the active nav item.
- **Float** (`--float`): tooltips, popovers and toasts.
- **Thumb / knob** (`--thumb-shadow`, `--knob-shadow`): slider thumbs and
  switch knobs.

### Named Rules
**The Half-Pixel Rule.** In dark, edges are 0.5px rings of black or 9 % white;
on paper a half-pixel melts into the grey, so light-theme edges are a full
pixel, fainter.

**The One Flourish Rule.** The lamp dot's glow (`box-shadow` scaled to its
size) is Lueur's one decorative effect in the chrome. Real glows belong to
content: the Lighting backdrop and the plan's light pools.

## Shapes

Gently rounded and consistent: 5px for small inner parts (segmented thumbs),
7px for controls and nav items, 10px for cards and tiles, full round for
switches, lamp dots, zone badges and slider thumbs. The logo is a squircle
tile (112/480 radius). No sharp corners and no pill-shaped buttons.

## Components

### Buttons
Tactile and discreet: physical to the touch, never asking for attention.
- **Shape:** 7px radius (6px for `sm`), 28px high (24px `sm`), 12px padding.
- **Default:** a two-stop near-white gradient, control edge and lit top edge,
  500 weight.
- **Primary:** ink gradient with `on-ink` text; one per view at most.
- **Plain:** transparent, `text-2`; hover fills `surface-hover`.
- **Danger:** default button with `danger` text and a `danger-soft` hover.
- **Hover / Active:** brightness 0.975 on hover; on press 0.94 and a
  half-pixel drop (`translateY(0.5px)`), 80 ms. Disabled drops to `text-3`.
- **Group:** joined buttons share one edge with 0.5px dividers.

### Switch
32 × 19px track, 15px white knob. Off is `line-strong` with a visible inner
edge; on is Filament Amber. Pressing widens the knob to 18px, as a real
toggle squashes. 220 ms knob travel.

### Segmented control
A `surface-hover` well with 2px padding and a raised thumb (control edge, lit
top) that slides between equal columns in 220 ms. Labels `text-2`, active
`text`.

### Slider
4px track filled with `text-2` up to the value, `line` beyond; 18px round
thumb that grows 6 % on hover and 12 % while dragged. Focus draws a 2px
surface gap and a 2px amber ring around the thumb.

### Cards / Rows
- **Corner Style:** 10px.
- **Background:** `surface`, with the card edge shadow.
- **Rows:** 44px min, 16px horizontal padding, 20px gap, hairline between
  rows; optional ⓘ tip that opens a floating bubble (max 280px).

### Inputs / Fields
28px, 7px radius, `surface` fill with the control edge. Focus outline sits
on the field's own edge (offset 0). Selects take the raised control look with
a drawn double chevron.

### Navigation
Sidebar items 36px high, 7px radius, icon plus label. Hover fills
`surface-hover`; the active page becomes a small card (surface + card edge)
with its icon in Filament Amber. Count badges are grey pills.

### Effect tile (signature)
A card holding a colour preview well (`--p` gradient, 38px) and a name, and
nothing else. Playing on every light picked draws a 2px inset ring of Active
Amber and a lamp before the name; on some of them, a dashed ring, with their
names on hover. A running
preview drifts slowly (6s); a pending one breathes. The Screen tile shows a
small monitor whose glow spills onto the wall behind it, desaturated when off.

Still modes (the moods) take the same tile: their well is the pool of their
white on a dark wall (`well-night`), as strong as the mood is bright; the
natural light's is its day, and it spans two columns.

### Light card
Each light on the Lighting page: a disc of the light it gives (its white or
colour, fainter as it dims, the gradient of its effect, a hollow ring while
off, dashed with no bulb), its name, a line saying what it does and how
bright, and a quiet power button (the disc already says whether it is on).
Picked, the card takes a neutral ink edge and
a tick on the disc, never amber. A click anywhere on the card picks it. Over
the settings beside them, the lights picked are named once, with their discs;
the sections under that heading carry no title of their own.

### Lamp dot (signature)
A 7px Filament Amber dot with a glow of its own size: something giving
light now, or waiting on you. The status vocabulary across the app.

### Backdrop (signature)
Behind the Lighting view, one soft pool of colour per light, placed where the
light stands relative to the screen and tinted with the frames its bulb is
receiving. Stronger and purer on paper (opacity 0.6, saturation 1.5), softer
at night (0.7, 1.15). Still under reduced motion.

### Room plan (signature)
A dotted-grid well with a white floor and dark walls; draggable corners, a
screen bar with its viewing cone toward "You", and light pins with a pool in
the colour of the zone they mirror and a dashed line to the screen edge
they follow.

## Do's and Don'ts

### Do:
- **Do** take every value from the tokens in `src/styles/app.css`, and define
  any new one there for both themes (OS dark, forced dark, forced light).
- **Do** keep chrome greys at zero chroma; colour enters only as light.
- **Do** pair every state colour with a lamp, a word, a tick, an edge or a
  letter.
- **Do** use `var(--dur)` (140 ms) and `var(--ease)` for state changes;
  220 ms for travelling parts (knobs, thumbs); 200 ms for a view entering.
- **Do** give every looping animation a still alternative under
  `prefers-reduced-motion`.
- **Do** use tabular figures for any number that changes.

### Don't:
- **Don't** use amber for a selection, a target or a primary button: amber
  means light.
- **Don't** introduce a second accent colour, a brand typeface or an icon
  font.
- **Don't** use red for anything but errors and destructive actions,
  including on the plan.
- **Don't** draw heavy or pure-black shadows on paper.
- **Don't** let a bulb's real colour tint the chrome; it belongs in previews,
  pools and dots.
- **Don't** flash: anything that blinks in the UI respects the same
  photosensitivity limits as the lights.
