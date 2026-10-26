<script lang="ts">
  import {
    app,
    edit,
    addLight,
    bindingFor,
    identifyLight,
    lightById,
    notify,
    markRoomArranged,
    removeLight,
    renameLight,
    setLightEnabled,
  } from "../state.svelte";
  import { tick } from "svelte";
  import { history, observe, planKey, redo, track, undo } from "../history.svelte";
  import {
    closestOnSegment,
    describe,
    endpoints,
    facing,
    mapLight,
    EDGE_COLOR,
    EDGE_KIND,
    EDGE_LETTER,
    type Edge,
  } from "../geometry";
  import { t, pct } from "../i18n";
  import { bulbLabel } from "../brands";
  import { MOD_KEY } from "../platform";
  import { WALLS, rgbCss, rgbOfHex } from "../scenes";
  import { defaultConfig, type Rgb, type Vec2 } from "../types";
  import Icon from "../ui/Icon.svelte";
  import Slider from "../ui/Slider.svelte";
  import Switch from "../ui/Switch.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";
  import ZoneBadge from "../ui/ZoneBadge.svelte";

  const DEF = defaultConfig();
  // Extent of the plan in plan units. Everything placed is kept inside it.
  const VIEW_W = 1000;
  const VIEW_H = 750;
  const MARGIN = 12;
  /** How far in front of the screen the seat handle sits, in plan units. */
  const SEAT = 120;
  const SCREEN_MIN = 120;
  const SCREEN_MAX = 520;

  let svgEl = $state<SVGSVGElement>();

  const sameWall = (a: Rgb, b: Rgb) => a.r === b.r && a.g === b.g && a.b === b.b;
  /** The paint picked, or -1 for one of the user's own. */
  const wallIndex = $derived(WALLS.findIndex((w) => sameWall(app.config.room.wall, w.rgb)));
  const wallHex = $derived.by(() => {
    const { r, g, b } = app.config.room.wall;
    return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
  });

  function setWall(rgb: Rgb) {
    edit((c) => (c.room.wall = { ...rgb }));
  }

  /** The wall swatches are a radio group: the arrows move the choice. */
  function onWallKey(e: KeyboardEvent & { currentTarget: HTMLElement }) {
    const last = WALLS.length - 1;
    const i = Math.max(0, wallIndex);
    const to =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? (i === last ? 0 : i + 1)
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? (i === 0 ? last : i - 1)
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? last
              : -1;
    if (to < 0) return;
    e.preventDefault();
    setWall(WALLS[to].rgb);
    e.currentTarget.querySelectorAll<HTMLButtonElement>("button")[to]?.focus();
  }

  // Selection: a light lives in shared state (other views link to it); the
  // screen and corners are only selectable here.
  type Other = { kind: "screen" } | { kind: "corner"; index: number } | null;
  let other = $state<Other>(null);

  const selLight = $derived(app.selectedLight ? lightById(app.selectedLight) : undefined);

  // --- Focus ---
  // Picking from the list, or adding, moves the focus to the inspector; going
  // back returns it to where it came from, or to the object's row in the list.
  // A pick on the plan with the pointer leaves the focus alone.

  let inspectorEl = $state<HTMLElement>();
  /** The control the inspector was opened from, when it outlives it. */
  let origin: HTMLElement | null = null;

  /** The list row of what is selected now, e.g. "light:<id>" or "corner:2". */
  function selectedKey(): string | null {
    if (selLight) return `light:${selLight.id}`;
    if (other?.kind === "screen") return "screen";
    if (other?.kind === "corner") return `corner:${other.index}`;
    return null;
  }

  async function focusInspector() {
    await tick();
    inspectorEl?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }

  /** Only take the focus back from the inspector or from nowhere, never
   *  from a control the user has since moved to. */
  function focusIsOurs(): boolean {
    const a = document.activeElement;
    return !a || a === document.body || !!inspectorEl?.contains(a);
  }

  /** After the inspector closes: back to the origin, else to the row `key`,
   *  else to the row now at `index` (what took a removed object's place). */
  async function focusList(key: string | null, index = 0) {
    const from = origin;
    origin = null;
    await tick();
    if (from?.isConnected) return from.focus();
    const rows = [...(inspectorEl?.querySelectorAll<HTMLElement>("[data-key]") ?? [])];
    const row = rows.find((r) => r.dataset.key === key) ?? rows[Math.min(Math.max(index, 0), rows.length - 1)];
    row?.focus();
  }

  function selectLight(id: string, focus = false) {
    app.selectedLight = id;
    other = null;
    if (focus) focusInspector();
  }
  function selectOther(o: Other, focus = false) {
    app.selectedLight = null;
    other = o;
    if (focus) focusInspector();
  }
  /** A pick from the list or an add button: the focus follows it. */
  function pick(e: Event, o: { light: string } | Other) {
    const el = e.currentTarget as HTMLElement;
    // List rows go away with the list; they are found again by their key.
    origin = el.dataset.key ? null : el;
    if (o && "light" in o) selectLight(o.light, true);
    else selectOther(o, true);
  }
  function clearSelection(restore = true) {
    const key = selectedKey();
    const ours = restore && key !== null && focusIsOurs();
    app.selectedLight = null;
    other = null;
    if (ours) focusList(key);
    else origin = null;
  }

  // --- Dragging ---

  type Drag =
    | { kind: "light"; id: string; off: Vec2 }
    | { kind: "screen"; off: Vec2 }
    | { kind: "corner"; index: number; off: Vec2 }
    | { kind: "seat" }
    | { kind: "width" }
    | null;
  let drag: Drag = null;
  let dragging = $state(false);

  function toPlan(e: PointerEvent | MouseEvent): Vec2 {
    const ctm = svgEl?.getScreenCTM();
    if (!ctm) return { x: 0, y: 0 };
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    return { x: p.x, y: p.y };
  }
  const sub = (a: Vec2, b: Vec2): Vec2 => ({ x: a.x - b.x, y: a.y - b.y });
  const clampP = (p: Vec2): Vec2 => ({
    x: Math.round(Math.max(MARGIN, Math.min(VIEW_W - MARGIN, p.x))),
    y: Math.round(Math.max(MARGIN, Math.min(VIEW_H - MARGIN, p.y))),
  });

  function begin(e: PointerEvent, d: NonNullable<Drag>) {
    if (e.button !== 0) return;
    e.stopPropagation();
    origin = null;
    drag = d;
    dragging = true;
    held = fitted;
    svgEl?.setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!drag) return;
    const p = toPlan(e);
    const d = drag;
    edit((c) => {
      const s = c.room.screen;
      switch (d.kind) {
        case "light": {
          const l = c.room.lights.find((x) => x.id === d.id);
          if (l) l.pos = clampP(sub(p, d.off));
          break;
        }
        case "screen":
          s.center = clampP(sub(p, d.off));
          break;
        case "corner":
          if (c.room.walls[d.index]) c.room.walls[d.index] = clampP(sub(p, d.off));
          break;
        case "seat": {
          // facing = (sin θ, cos θ), so θ = atan2(dx, dy).
          let deg = (Math.atan2(p.x - s.center.x, p.y - s.center.y) * 180) / Math.PI;
          deg = e.shiftKey ? Math.round(deg / 15) * 15 : Math.round(deg);
          if (deg <= -180) deg += 360;
          s.facing_deg = deg;
          break;
        }
        case "width": {
          const half = Math.hypot(p.x - s.center.x, p.y - s.center.y);
          s.width = Math.round(Math.max(SCREEN_MIN, Math.min(SCREEN_MAX, half * 2)) / 5) * 5;
          break;
        }
      }
    });
  }

  function endDrag(e: PointerEvent) {
    if (drag) {
      try {
        svgEl?.releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
    }
    // Moving a light or the seat is arranging the room.
    if (drag && (drag.kind === "light" || drag.kind === "seat" || drag.kind === "screen")) markRoomArranged();
    drag = null;
    dragging = false;
    held = null;
  }

  // --- Editing ---

  function newLight(e: Event) {
    const l = addLight();
    pick(e, { light: l.id });
  }

  /** Double-click on a wall drops a corner exactly there. */
  function insertCornerAt(e: MouseEvent) {
    const p = toPlan(e);
    const w = app.config.room.walls;
    let best = -1;
    let bestDist = Infinity;
    let bestPoint = p;
    for (let i = 0; i < w.length; i++) {
      const q = closestOnSegment(p, w[i], w[(i + 1) % w.length]);
      const dist = Math.hypot(p.x - q.x, p.y - q.y);
      if (dist < bestDist) {
        bestDist = dist;
        best = i;
        bestPoint = q;
      }
    }
    if (best < 0) return;
    edit((c) => {
      c.room.walls.splice(best + 1, 0, clampP(bestPoint));
    });
    selectOther({ kind: "corner", index: best + 1 });
  }

  /** Toolbar version: split the longest wall in two. */
  function addCorner(e: Event) {
    const w = app.config.room.walls;
    let best = 0;
    let bestLen = -1;
    for (let i = 0; i < w.length; i++) {
      const a = w[i];
      const b = w[(i + 1) % w.length];
      const len = Math.hypot(b.x - a.x, b.y - a.y);
      if (len > bestLen) {
        bestLen = len;
        best = i;
      }
    }
    edit((c) => {
      const a = c.room.walls[best];
      const b = c.room.walls[(best + 1) % c.room.walls.length];
      c.room.walls.splice(best + 1, 0, { x: Math.round((a.x + b.x) / 2), y: Math.round((a.y + b.y) / 2) });
    });
    pick(e, { kind: "corner", index: best + 1 });
  }

  function removeCorner(index: number) {
    if (app.config.room.walls.length <= 3) {
      notify("info", t("min_corners"));
      return;
    }
    const ours = focusIsOurs();
    const row = rowIndex(`corner:${index}`);
    edit((c) => {
      c.room.walls.splice(index, 1);
    });
    other = null;
    if (ours) focusList(null, row);
  }

  function removeSelectedLight(id: string) {
    const ours = focusIsOurs();
    const row = rowIndex(`light:${id}`);
    removeLight(id);
    if (ours) focusList(null, row);
  }

  function resetShape() {
    edit((c) => {
      c.room.walls = structuredClone(DEF.room.walls);
      c.room.screen = structuredClone(DEF.room.screen);
    });
    clearSelection(false);
    notify("info", t("reset_done"));
  }

  function removeSelected() {
    if (selLight) removeSelectedLight(selLight.id);
    else if (other?.kind === "corner") removeCorner(other.index);
  }

  function nudge(dx: number, dy: number) {
    edit((c) => {
      if (selLight) {
        const l = c.room.lights.find((x) => x.id === selLight.id);
        if (l) l.pos = clampP({ x: l.pos.x + dx, y: l.pos.y + dy });
      } else if (other?.kind === "screen") {
        c.room.screen.center = clampP({ x: c.room.screen.center.x + dx, y: c.room.screen.center.y + dy });
      } else if (other?.kind === "corner") {
        const i = other.index;
        const p = c.room.walls[i];
        if (p) c.room.walls[i] = clampP({ x: p.x + dx, y: p.y + dy });
      }
    });
    announceSoon();
  }

  // --- Typed positions ---
  // The keyboard's way to place things: X and Y fields, kept in the plan's
  // bounds. A value is taken while typing only once it is in bounds, so
  // typing "420" does not stop at 12 on the way; leaving the field clamps it.

  const X_MAX = VIEW_W - MARGIN;
  const Y_MAX = VIEW_H - MARGIN;

  function selectedPos(): Vec2 | undefined {
    if (selLight) return lightById(selLight.id)?.pos;
    if (other?.kind === "screen") return app.config.room.screen.center;
    if (other?.kind === "corner") return app.config.room.walls[other.index];
  }

  function setPos(axis: "x" | "y", v: number) {
    edit((c) => {
      const put = (p: Vec2): Vec2 => clampP({ ...p, [axis]: v });
      if (selLight) {
        const l = c.room.lights.find((x) => x.id === selLight.id);
        if (l) l.pos = put(l.pos);
      } else if (other?.kind === "screen") {
        c.room.screen.center = put(c.room.screen.center);
      } else if (other?.kind === "corner") {
        const p = c.room.walls[other.index];
        if (p) c.room.walls[other.index] = put(p);
      }
    });
    // Placing a light or the screen is arranging the room, as a drag is.
    if (selLight || other?.kind === "screen") markRoomArranged();
  }

  function onPosInput(axis: "x" | "y", e: Event & { currentTarget: HTMLInputElement }) {
    const v = e.currentTarget.valueAsNumber;
    if (Number.isFinite(v) && v >= MARGIN && v <= (axis === "x" ? X_MAX : Y_MAX)) setPos(axis, v);
  }

  function onPosChange(axis: "x" | "y", e: Event & { currentTarget: HTMLInputElement }) {
    const v = e.currentTarget.valueAsNumber;
    if (Number.isFinite(v)) setPos(axis, v);
    // Show what was kept: the clamped value, or the old one for no number.
    const kept = selectedPos();
    if (kept) e.currentTarget.value = String(kept[axis]);
  }

  // --- Announcements ---
  // Moves made with the arrows are read out once the keys rest, so holding
  // an arrow gives one message, not one per step.

  let announcement = $state("");
  let announceTimer: ReturnType<typeof setTimeout> | null = null;

  function announceSoon() {
    if (announceTimer) clearTimeout(announceTimer);
    announceTimer = setTimeout(() => {
      announceTimer = null;
      announcement = whereNow();
    }, 600);
  }
  $effect(() => () => {
    if (announceTimer) clearTimeout(announceTimer);
  });

  function whereNow(): string {
    const p = selectedPos();
    if (!p) return "";
    if (selLight) {
      const m = mapLight(app.config.room.screen, selLight);
      return t("moved_zone", {
        name: selLight.name,
        x: p.x,
        y: p.y,
        letter: t(EDGE_LETTER[m.edge]),
        zone: t(EDGE_KIND[m.edge]),
      });
    }
    const name = other?.kind === "corner" ? cornerName(other.index) : t("plan_screen");
    return t("moved", { name, x: p.x, y: p.y });
  }

  // --- Naming corners ---

  const WHERE = [
    ["where_top_left", "where_top", "where_top_right"],
    ["where_left", "where_middle", "where_right"],
    ["where_bottom_left", "where_bottom", "where_bottom_right"],
  ] as const;

  /** "Corner 2, top right": where it sits in the room as drawn. */
  function cornerName(i: number): string {
    const w = app.config.room.walls;
    const p = w[i];
    if (!p) return t("corner_n", { n: i + 1 });
    const third = (v: number, all: number[]) => {
      const lo = Math.min(...all);
      const f = (v - lo) / Math.max(Math.max(...all) - lo, 1);
      return f < 1 / 3 ? 0 : f > 2 / 3 ? 2 : 1;
    };
    const row = third(p.y, w.map((q) => q.y));
    const col = third(p.x, w.map((q) => q.x));
    return t("corner_at", { n: i + 1, where: t(WHERE[row][col]) });
  }

  /** Where an object's row sits in the list, to land on its neighbour. */
  function rowIndex(key: string): number {
    const { lights, walls } = app.config.room;
    return ["screen", ...lights.map((l) => `light:${l.id}`), ...walls.map((_, i) => `corner:${i}`)].indexOf(key);
  }

  // --- History ---

  track();
  // Every change to the plan feeds the history; a drag is one step.
  $effect(() => observe(planKey(), dragging));

  /** Undo or redo, then let go of what the step took away. */
  function travel(dir: "undo" | "redo") {
    if (dir === "undo") undo();
    else redo();
    if (app.selectedLight && !lightById(app.selectedLight)) app.selectedLight = null;
    if (other?.kind === "corner" && other.index >= app.config.room.walls.length) other = null;
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    // Text fields keep their own undo and arrows; Escape still closes the
    // inspector they sit in.
    if (t.closest("input, select, textarea")) {
      if (e.key === "Escape" && inspectorEl?.contains(t) && (selLight || other)) {
        e.preventDefault();
        clearSelection();
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !drag) {
      const key = e.key.toLowerCase();
      if (key === "z" || key === "y") {
        e.preventDefault();
        travel(key === "y" || e.shiftKey ? "redo" : "undo");
      }
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const step = e.shiftKey ? 25 : 5;
    switch (e.key) {
      case "Escape":
        clearSelection();
        break;
      case "Delete":
      case "Backspace":
        if (!selLight && other?.kind !== "corner") return;
        removeSelected();
        break;
      case "ArrowLeft":
        nudge(-step, 0);
        break;
      case "ArrowRight":
        nudge(step, 0);
        break;
      case "ArrowUp":
        nudge(0, -step);
        break;
      case "ArrowDown":
        nudge(0, step);
        break;
      default:
        return;
    }
    if (selLight || other) e.preventDefault();
  }

  // --- Drawing ---

  const room = $derived(app.config.room);
  const scr = $derived(room.screen);
  const wallPoints = $derived(room.walls.map((p) => `${p.x},${p.y}`).join(" "));
  const ends = $derived(endpoints(scr));
  const face = $derived(facing(scr));
  const seat = $derived({ x: scr.center.x + face.x * SEAT, y: scr.center.y + face.y * SEAT });
  /** Your view of the picture: from the seat to both ends of the screen. */
  const cone = $derived(`${seat.x},${seat.y} ${ends.left.x},${ends.left.y} ${ends.right.x},${ends.right.y}`);

  // --- Framing ---
  // The view fits whatever is drawn, so a small room fills the stage. It is
  // held still during a drag, or the plan would slide under the pointer.
  let stageW = $state(0);
  let stageH = $state(0);

  const fitted = $derived.by(() => {
    const pts = [...room.walls, ...room.lights.map((l) => l.pos), ends.left, ends.right, seat];
    const xs = pts.map((p) => p.x);
    const ys = pts.map((p) => p.y);
    const pad = 70;
    let x = Math.min(...xs) - pad;
    let y = Math.min(...ys) - pad;
    let w = Math.max(Math.max(...xs) + pad - x, 400);
    let h = Math.max(Math.max(...ys) + pad - y, 300);
    // Match the stage's shape so the plan is centred, not letterboxed.
    if (stageW > 0 && stageH > 0) {
      const aspect = stageW / stageH;
      if (w / h < aspect) {
        const nw = h * aspect;
        x -= (nw - w) / 2;
        w = nw;
      } else {
        const nh = w / aspect;
        y -= (nh - h) / 2;
        h = nh;
      }
    }
    return { x, y, w, h };
  });

  let held = $state<typeof fitted | null>(null);
  const box = $derived(held ?? fitted);
  /** Plan units per screen pixel: keeps text and handles a constant size. */
  const k = $derived(stageW > 0 ? Math.max(box.w / stageW, box.h / stageH) : 1);

  function anchorOnScreen(u: number): Vec2 {
    const t = Math.max(0, Math.min(1, (u + 1) / 2));
    return { x: ends.left.x + (ends.right.x - ends.left.x) * t, y: ends.left.y + (ends.right.y - ends.left.y) * t };
  }

  /** A light's name sits below it, or above when the light is just behind
   *  the screen, so names never sit across the screen. */
  function labelPos(p: Vec2): Vec2 {
    const q = closestOnSegment(p, ends.left, ends.right);
    const behind = Math.abs(p.x - q.x) < 70 * k && p.y < q.y && q.y - p.y < 120 * k;
    return behind ? { x: p.x, y: p.y - 16 * k } : { x: p.x, y: p.y + 24 * k };
  }

  const heightLabel = (v: number) =>
    t(v < 0.18 ? "h_floor" : v < 0.42 ? "h_low" : v < 0.58 ? "h_level" : v < 0.82 ? "h_high" : "h_ceiling");

  const EDGES: Edge[] = ["left", "center", "right"];
</script>

<svelte:window onkeydown={onKey} />

<div class="room">
  <section class="stage-col">
    <header class="bar">
      <h1 class="page-title">{t("nav_room")}</h1>
      <div class="tools">
        <div class="btn-group">
          <button
            class="btn icon"
            title="{t('undo')} ({MOD_KEY}Z)"
            aria-label={t("undo")}
            disabled={!history.canUndo}
            onclick={() => travel("undo")}><Icon name="undo" size={15} /></button
          >
          <button
            class="btn icon"
            title="{t('redo')} ({MOD_KEY === '⌘' ? '⇧⌘Z' : 'Ctrl+Y'})"
            aria-label={t("redo")}
            disabled={!history.canRedo}
            onclick={() => travel("redo")}><Icon name="redo" size={15} /></button
          >
        </div>
        <button class="btn plain" onclick={resetShape}>{t("reset_shape")}</button>
        <div class="btn-group">
          <button class="btn" onclick={addCorner}>{t("add_corner")}</button>
          <button class="btn" onclick={newLight}>{t("add_light")}</button>
        </div>
      </div>
    </header>

    {#if !app.config.interface.room_arranged && room.lights.length > 0}
      <div class="coach" role="status">
        <span class="lamp" aria-hidden="true"></span>
        <p>{t("coach")}</p>
        <button class="btn primary" onclick={markRoomArranged}>{t("coach_done")}</button>
      </div>
    {/if}
    <div class="banner-slot"><RestartBanner /></div>

    <div class="stage" class:dragging bind:clientWidth={stageW} bind:clientHeight={stageH}>
      <!-- The plan is a picture to screen readers: everything on it can be
           reached, selected and placed from the list in the inspector. -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <svg
        bind:this={svgEl}
        viewBox="{box.x} {box.y} {box.w} {box.h}"
        style="--k:{k}"
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label={t("plan_label")}
        onpointerdown={() => clearSelection(false)}
        onpointermove={onMove}
        onpointerup={endDrag}
        onpointercancel={endDrag}
      >
        <defs>
          <pattern id="dots" width="25" height="25" patternUnits="userSpaceOnUse">
            <circle cx="12.5" cy="12.5" r={1.1 * k} class="dot" />
          </pattern>
          {#each EDGES as edge}
            <radialGradient id="halo-{edge}">
              <stop offset="0" stop-color={EDGE_COLOR[edge]} stop-opacity="0.55" />
              <stop offset="0.55" stop-color={EDGE_COLOR[edge]} stop-opacity="0.16" />
              <stop offset="1" stop-color={EDGE_COLOR[edge]} stop-opacity="0" />
            </radialGradient>
          {/each}
        </defs>

        <rect x={box.x} y={box.y} width={box.w} height={box.h} fill="url(#dots)" />

        <!-- Floor, then the light falling on it, then the walls on top -->
        <polygon points={wallPoints} class="floor" />
        {#each room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          {#if l.enabled}
            <circle
              cx={l.pos.x}
              cy={l.pos.y}
              r={70 + 70 * m.spread}
              fill="url(#halo-{m.edge})"
              opacity={0.5 + 0.5 * m.gain}
              class="halo"
            />
          {/if}
        {/each}
        <polygon points={cone} class="cone" />
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <polygon points={wallPoints} class="walls" ondblclick={insertCornerAt} />

        <!-- Which part of the screen each light looks at -->
        {#each room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          {@const a = anchorOnScreen(m.u)}
          <line
            x1={l.pos.x}
            y1={l.pos.y}
            x2={a.x}
            y2={a.y}
            class="link"
            stroke={EDGE_COLOR[m.edge]}
            opacity={l.enabled ? 0.25 + 0.45 * m.gain : 0.12}
          />
        {/each}

        <!-- Seat: drag it around to turn the screen -->
        <line x1={scr.center.x} y1={scr.center.y} x2={seat.x} y2={seat.y} class="sightline" />
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <g class="seat" onpointerdown={(e) => begin(e, { kind: "seat" })}>
          <title>{t("seat_title")}</title>
          <circle cx={seat.x} cy={seat.y} r={14 * k} class="seat-hit" />
          <circle cx={seat.x} cy={seat.y} r={7 * k} class="seat-dot" />
        </g>
        <text x={seat.x} y={seat.y + 22 * k} class="caption" text-anchor="middle">{t("plan_you")}</text>

        <!-- Screen -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <g
          class="screen"
          class:sel={other?.kind === "screen"}
          onpointerdown={(e) => {
            selectOther({ kind: "screen" });
            begin(e, { kind: "screen", off: sub(toPlan(e), scr.center) });
          }}
        >
          <line x1={ends.left.x} y1={ends.left.y} x2={ends.right.x} y2={ends.right.y} class="screen-hit" />
          <line x1={ends.left.x} y1={ends.left.y} x2={ends.right.x} y2={ends.right.y} class="screen-body" />
        </g>
        {#if other?.kind === "screen"}
          {#each [ends.left, ends.right] as p}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <circle
              cx={p.x}
              cy={p.y}
              r={6 * k}
              class="handle"
              onpointerdown={(e) => begin(e, { kind: "width" })}
            >
              <title>{t("resize_title")}</title>
            </circle>
          {/each}
        {/if}
        <text
          x={scr.center.x + face.x * 20 * k}
          y={scr.center.y + face.y * 20 * k + 4 * k}
          class="caption"
          text-anchor="middle">{t("plan_screen")}</text
        >

        <!-- Corners -->
        {#each room.walls as w, i}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <rect
            class="corner"
            class:sel={other?.kind === "corner" && other.index === i}
            x={w.x - 5 * k}
            y={w.y - 5 * k}
            width={10 * k}
            height={10 * k}
            rx={2 * k}
            onpointerdown={(e) => {
              selectOther({ kind: "corner", index: i });
              begin(e, { kind: "corner", index: i, off: sub(toPlan(e), w) });
            }}
          />
        {/each}

        <!-- Lights on top -->
        {#each room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          {@const sel = app.selectedLight === l.id}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g
            class="light"
            class:sel
            class:off={!l.enabled}
            onpointerdown={(e) => {
              selectLight(l.id);
              begin(e, { kind: "light", id: l.id, off: sub(toPlan(e), l.pos) });
            }}
          >
            <circle cx={l.pos.x} cy={l.pos.y} r={15 * k} class="ring" />
            <circle cx={l.pos.x} cy={l.pos.y} r={9.5 * k} class="bulb" fill={EDGE_COLOR[m.edge]} />
            <text x={l.pos.x} y={l.pos.y} class="bulb-letter" text-anchor="middle" dominant-baseline="central"
              >{t(EDGE_LETTER[m.edge])}</text
            >
            <text x={labelPos(l.pos).x} y={labelPos(l.pos).y} class="label" text-anchor="middle">{l.name}</text>
          </g>
        {/each}
      </svg>

      <p class="help">{t("plan_help")}</p>
    </div>
  </section>

  {#snippet place(p: Vec2)}
    <fieldset class="place">
      <legend class="field-label">{t("position")}</legend>
      <div class="xy">
        {#each ["x", "y"] as const as axis}
          <label class="axis">
            <span aria-hidden="true">{axis.toUpperCase()}</span>
            <input
              type="number"
              class="num"
              min={MARGIN}
              max={axis === "x" ? X_MAX : Y_MAX}
              step="1"
              value={p[axis]}
              aria-label={t(axis === "x" ? "pos_x" : "pos_y")}
              aria-describedby="pos-tip"
              oninput={(e) => onPosInput(axis, e)}
              onchange={(e) => onPosChange(axis, e)}
            />
          </label>
        {/each}
      </div>
      <p class="hint" id="pos-tip">{t("pos_tip", { min: MARGIN, maxX: X_MAX, maxY: Y_MAX, w: scr.width })}</p>
    </fieldset>
  {/snippet}

  <aside class="inspector" bind:this={inspectorEl}>
    {#if selLight}
      {@const l = selLight}
      {@const m = mapLight(scr, l)}
      {@const b = bindingFor(l.id)}
      <button class="back" onclick={() => clearSelection()}><Icon name="back" size={13} /> {t("all_objects")}</button>

      <input
        class="name"
        type="text"
        value={l.name}
        aria-label={t("light_name")}
        spellcheck="false"
        data-autofocus
        oninput={(e) => renameLight(l.id, e.currentTarget.value)}
      />

      <figure class="preview" style="--c:{EDGE_COLOR[m.edge]}">
        <svg viewBox="0 0 160 90" role="img" aria-label={t("mirrors")}>
          <rect x="0.5" y="0.5" width="159" height="89" rx="5" class="preview-bg" />
          <rect
            x={m.zone.x * 160 + 3}
            y={m.zone.y * 90 + 3}
            width={Math.max(m.zone.w * 160 - 6, 4)}
            height={Math.max(m.zone.h * 90 - 6, 4)}
            rx="3"
            class="preview-zone"
            fill-opacity={0.2 + 0.25 * (1 - m.spread)}
          />
        </svg>
        <figcaption><ZoneBadge edge={m.edge} off={!l.enabled} labelled />{t(describe(m))}</figcaption>
      </figure>

      <div class="fields">
        {@render place(l.pos)}
        <div class="field">
          <span class="field-label">{t("height")}</span>
          <Slider
            label={t("height")}
            value={l.height}
            min={0}
            max={1}
            step={0.01}
            def={0.5}
            format={heightLabel}
            onchange={(v) =>
              edit((c) => {
                const x = c.room.lights.find((y) => y.id === l.id);
                if (x) x.height = v;
              })}
          />
        </div>
        <div class="field inline">
          <span class="field-label">{t("follows")}</span>
          <Switch checked={l.enabled} label={t("follows")} onchange={(v) => setLightEnabled(l.id, v)} />
        </div>
        <div class="field inline" title={t("facts_tip")}>
          <span class="field-label">{t("fact_brightness")}</span>
          <span class="fact num">{pct(m.gain)}</span>
        </div>
        <div class="field inline" title={t("facts_tip")}>
          <span class="field-label">{t("fact_response")}</span>
          <span class="fact num">{pct(m.responsiveness)}</span>
        </div>
      </div>

      <div class="bulb-box">
        <Icon name="bulb" size={15} />
        {#if b}
          <!-- The bulb by name; its address only in the tooltip, for whoever needs it. -->
          <span class="bulb-info" title={b.ip}>{bulbLabel(b, l.name)}</span>
          <button class="btn sm" title={t("identify_tip")} onclick={() => identifyLight(l.id)}>{t("identify")}</button>
        {:else}
          <span class="bulb-info warn">{t("no_bulb")}</span>
          <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
        {/if}
      </div>

      <button class="btn plain danger remove" onclick={() => removeSelectedLight(l.id)}>{t("remove_light")}</button>
    {:else if other?.kind === "screen"}
      <button class="back" onclick={() => clearSelection()}><Icon name="back" size={13} /> {t("all_objects")}</button>
      <h2 class="name static" tabindex="-1" data-autofocus>{t("plan_screen")}</h2>
      <div class="fields">
        {@render place(scr.center)}
        <div class="field">
          <span class="field-label">{t("facing")}</span>
          <Slider
            label={t("facing")}
            value={scr.facing_deg}
            min={-180}
            max={180}
            step={1}
            def={DEF.room.screen.facing_deg}
            format={(v) => `${v}°`}
            onchange={(v) => edit((c) => (c.room.screen.facing_deg = v))}
          />
        </div>
        <div class="field">
          <span class="field-label">{t("width")}</span>
          <Slider
            label={t("width")}
            value={scr.width}
            min={SCREEN_MIN}
            max={SCREEN_MAX}
            step={5}
            def={DEF.room.screen.width}
            format={(v) => String(Math.round(v))}
            onchange={(v) => edit((c) => (c.room.screen.width = v))}
          />
        </div>
      </div>
    {:else if other?.kind === "corner"}
      {@const index = other.index}
      {@const p = room.walls[index]}
      <button class="back" onclick={() => clearSelection()}><Icon name="back" size={13} /> {t("all_objects")}</button>
      <h2 class="name static" tabindex="-1" data-autofocus>{cornerName(index)}</h2>
      {#if p}
        <div class="fields">
          {@render place(p)}
        </div>
      {/if}
      <button class="btn plain danger remove" onclick={() => removeCorner(index)}>{t("remove_corner")}</button>
    {:else}
      <h2 class="side-title" id="objects-title">{t("objects")}</h2>
      <ul class="objects" aria-labelledby="objects-title">
        <li>
          <button class="obj" data-key="screen" onclick={(e) => pick(e, { kind: "screen" })}>
            <Icon name="screen" size={15} />
            <span class="obj-name">{t("plan_screen")}</span>
            <span class="obj-meta num">{scr.facing_deg}°</span>
          </button>
        </li>
        {#each room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          <li>
            <button class="obj" class:off={!l.enabled} data-key="light:{l.id}" onclick={(e) => pick(e, { light: l.id })}>
              <ZoneBadge edge={m.edge} off={!l.enabled} />
              <span class="obj-name"
                >{l.name}<span class="sr-only"
                  >, {t("zone_of", { letter: t(EDGE_LETTER[m.edge]), zone: t(EDGE_KIND[m.edge]) })}{#if !l.enabled}, {t(
                      "not_following",
                    )}{/if}</span
                ></span
              >
              {#if !bindingFor(l.id)}<span class="obj-warn">{t("no_bulb")}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
      {#if room.lights.length === 0}
        <p class="empty">{t("no_lights")}</p>
      {/if}

      <h2 class="side-title corners-title" id="corners-title">{t("corners")}</h2>
      <ul class="objects" aria-labelledby="corners-title">
        {#each room.walls as w, i}
          <li>
            <button class="obj" data-key="corner:{i}" onclick={(e) => pick(e, { kind: "corner", index: i })}>
              <span class="obj-corner" aria-hidden="true"></span>
              <span class="obj-name">{cornerName(i)}</span>
              <span class="obj-meta num" aria-hidden="true">{w.x}, {w.y}</span>
            </button>
          </li>
        {/each}
      </ul>

      <div class="paint">
        <h2 class="side-title" id="walls-title">{t("walls")}</h2>
        <div class="walls" role="radiogroup" aria-labelledby="walls-title" tabindex="-1" onkeydown={onWallKey}>
          {#each WALLS as w, i (w.id)}
            {@const picked = i === wallIndex}
            <button
              class="wall"
              class:picked
              role="radio"
              aria-checked={picked}
              tabindex={i === Math.max(0, wallIndex) ? 0 : -1}
              title={t(`wall_${w.id}`)}
              aria-label={t(`wall_${w.id}`)}
              style="--c:{rgbCss([w.rgb.r, w.rgb.g, w.rgb.b])}"
              onclick={() => setWall(w.rgb)}
            ></button>
          {/each}
          <label
            class="wall any"
            class:picked={wallIndex < 0}
            title={t("wall_custom")}
            style={wallIndex < 0 ? `--c:${wallHex}` : undefined}
          >
            <input
              type="color"
              value={wallHex}
              aria-label={t("wall_custom")}
              oninput={(e) => {
                const c = rgbOfHex(e.currentTarget.value);
                if (c) setWall({ r: c[0], g: c[1], b: c[2] });
              }}
            />
          </label>
        </div>
        <p class="paint-tip">{t("walls_tip")}</p>
      </div>

      <div class="legend">
        {#each EDGES as edge}
          <span><ZoneBadge {edge} />{t(EDGE_KIND[edge])}</span>
        {/each}
      </div>
    {/if}
  </aside>

  <!-- Where a light moved with the arrows went, for screen readers. -->
  <p class="sr-only" aria-live="polite">{announcement}</p>
</div>

<style>
  .room {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 288px;
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
  }

  /* Stage */
  .stage-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    padding: var(--s-5) var(--s-5) var(--s-5) var(--s-6);
    gap: var(--s-3);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-4);
  }
  .coach {
    display: flex;
    align-items: center;
    gap: var(--s-4);
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-4);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
  }
  .coach p {
    flex: 1;
  }
  .tools {
    display: flex;
    gap: var(--s-2);
  }
  .banner-slot:empty {
    display: none;
  }
  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    border-radius: var(--r-lg);
    background: var(--plan-bg);
    box-shadow: var(--card-edge);
    overflow: hidden;
  }
  .stage svg {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
    touch-action: none;
    font-family: var(--font);
  }
  .help {
    position: absolute;
    left: 50%;
    bottom: var(--s-3);
    transform: translateX(-50%);
    padding: 3px 10px;
    border-radius: var(--r-full);
    background: color-mix(in oklab, var(--surface) 80%, transparent);
    backdrop-filter: blur(8px);
    box-shadow: var(--card-edge);
    font-size: var(--t-xs);
    color: var(--text-3);
    white-space: nowrap;
    pointer-events: none;
  }

  .dot {
    fill: var(--plan-grid);
  }
  .floor {
    fill: var(--plan-floor);
    filter: drop-shadow(0 calc(3px * var(--k)) calc(10px * var(--k)) oklch(0 0 0 / 0.1));
  }
  .walls {
    fill: none;
    stroke: var(--plan-wall);
    stroke-width: calc(5px * var(--k));
    stroke-linejoin: miter;
    pointer-events: stroke;
    cursor: copy;
  }
  .cone {
    fill: var(--text);
    opacity: 0.04;
    pointer-events: none;
  }
  .halo {
    pointer-events: none;
    transition: opacity 300ms var(--ease);
  }
  .link {
    stroke-width: calc(1.5px * var(--k));
    stroke-dasharray: calc(2px * var(--k)) calc(5px * var(--k));
    stroke-linecap: round;
    pointer-events: none;
  }
  .sightline {
    stroke: var(--text-3);
    stroke-width: calc(1.25px * var(--k));
    stroke-dasharray: calc(4px * var(--k)) calc(5px * var(--k));
    pointer-events: none;
  }
  .seat {
    cursor: grab;
  }
  .seat-hit {
    fill: transparent;
  }
  .seat-dot {
    fill: var(--plan-floor);
    stroke: var(--text-2);
    stroke-width: calc(2px * var(--k));
    transition: stroke var(--dur) var(--ease);
  }
  .seat:hover .seat-dot {
    stroke: var(--text);
  }
  .caption {
    fill: var(--text-3);
    font-size: calc(12px * var(--k));
    font-weight: 500;
    pointer-events: none;
    paint-order: stroke;
    stroke: var(--plan-floor);
    stroke-width: calc(4px * var(--k));
    stroke-linejoin: round;
  }

  .screen {
    cursor: grab;
  }
  .screen-hit {
    stroke: transparent;
    stroke-width: calc(22px * var(--k));
    stroke-linecap: round;
  }
  .screen-body {
    stroke: var(--plan-wall);
    stroke-width: calc(7px * var(--k));
    stroke-linecap: round;
    transition: stroke var(--dur) var(--ease);
  }
  .screen.sel .screen-body {
    stroke: var(--active);
  }
  .handle {
    fill: var(--surface);
    stroke: var(--active);
    stroke-width: calc(2px * var(--k));
    cursor: ew-resize;
  }

  .corner {
    fill: var(--plan-floor);
    stroke: var(--plan-wall);
    stroke-width: calc(1.5px * var(--k));
    cursor: move;
  }
  .corner:hover {
    fill: var(--surface-hover);
  }
  .corner.sel {
    fill: var(--active);
    stroke: var(--active);
  }

  .light {
    cursor: grab;
  }
  .ring {
    fill: none;
    stroke: var(--active);
    stroke-width: calc(2px * var(--k));
    opacity: 0;
    transition: opacity var(--dur) var(--ease);
  }
  .light:hover .ring {
    opacity: 0.35;
  }
  .light.sel .ring {
    opacity: 1;
  }
  .bulb {
    filter: drop-shadow(0 calc(1px * var(--k)) calc(2px * var(--k)) var(--pin-shadow));
    stroke: var(--plan-floor);
    stroke-width: calc(2.5px * var(--k));
  }
  .light.off .bulb {
    fill: var(--line-strong);
  }
  /* The zone's letter, so a bulb's zone reads without its colour. */
  .bulb-letter {
    fill: var(--zone-ink);
    font-size: calc(10.5px * var(--k));
    font-weight: 700;
    pointer-events: none;
  }
  .light.off .bulb-letter {
    fill: var(--text);
  }
  .label {
    fill: var(--text);
    font-size: calc(12.5px * var(--k));
    font-weight: 500;
    pointer-events: none;
    paint-order: stroke;
    stroke: var(--plan-floor);
    stroke-width: calc(4px * var(--k));
    stroke-linejoin: round;
  }
  .light.off .label {
    fill: var(--text-3);
  }
  .dragging,
  .dragging * {
    cursor: grabbing !important;
  }

  /* Inspector */
  .inspector {
    border-left: 1px solid var(--line);
    background: var(--sidebar);
    padding: var(--s-4);
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: stretch;
  }
  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin: 0 0 var(--s-3) -6px;
    padding: 2px 6px 2px 2px;
    border-radius: var(--r-sm);
    color: var(--text-3);
    font-size: var(--t-sm);
  }
  .back:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .name {
    font-size: var(--t-lg);
    font-weight: 600;
    height: 32px;
    margin-left: -8px;
    width: calc(100% + 8px);
    background: transparent;
    box-shadow: none;
  }
  .name:hover {
    box-shadow: var(--control-edge);
  }
  .name.static {
    margin: 0 0 var(--s-4);
    height: auto;
    width: auto;
  }

  .preview {
    margin: var(--s-3) 0 var(--s-5);
  }
  .preview svg {
    display: block;
    width: 100%;
    height: auto;
    border-radius: 6px;
    box-shadow: 0 0 0 3px var(--plan-wall);
  }
  .preview-bg {
    fill: var(--sunken);
    stroke: none;
  }
  .preview-zone {
    fill: var(--c);
    stroke: var(--c);
    stroke-width: 1.25;
    transition:
      x 200ms var(--ease),
      y 200ms var(--ease),
      width 200ms var(--ease),
      height 200ms var(--ease);
  }
  figcaption {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin-top: var(--s-3);
    font-size: var(--t-sm);
    color: var(--text-2);
  }

  .place {
    margin: 0;
    padding: 0;
    border: none;
    min-width: 0;
  }
  .place legend {
    padding: 0;
  }
  .xy {
    display: flex;
    gap: var(--s-2);
  }
  .axis {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text-2);
  }
  .axis input {
    flex: 1;
    min-width: 0;
    width: 100%;
  }
  .hint {
    margin-top: var(--s-2);
    font-size: var(--t-xs);
    color: var(--text-3);
    line-height: 1.4;
    user-select: text;
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
    padding: var(--s-4) 0;
    border-top: 1px solid var(--line);
  }
  .field-label {
    display: block;
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--text-2);
    margin-bottom: 2px;
  }
  .field.inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .field.inline .field-label {
    margin: 0;
  }
  .fact {
    font-size: var(--t-sm);
    color: var(--text-2);
  }

  .bulb-box {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-3);
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--card-edge);
    color: var(--text-3);
  }
  .bulb-info {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
    font-size: var(--t-sm);
  }
  .warn {
    color: var(--glow-text);
  }
  .remove {
    align-self: flex-start;
    margin-top: var(--s-3);
    margin-left: -8px;
  }

  .side-title {
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text-3);
    margin: 0 0 var(--s-2) 2px;
  }
  .objects {
    list-style: none;
    margin: 0 -6px;
    padding: 0;
  }
  .obj {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--r-sm);
    text-align: left;
    color: var(--text-3);
  }
  .obj:hover {
    background: var(--surface-hover);
  }
  .obj-name {
    flex: 1;
    min-width: 0;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .obj.off .obj-name {
    color: var(--text-3);
  }
  .obj :global(.zone-badge) {
    margin: 0 0.5px;
  }
  /* A corner as the plan draws it. */
  .obj-corner {
    width: 9px;
    height: 9px;
    margin: 0 3px;
    flex: none;
    border-radius: 2px;
    background: var(--plan-floor);
    box-shadow: inset 0 0 0 1.5px var(--plan-wall);
  }
  .corners-title {
    margin-top: var(--s-4);
  }
  .obj-meta {
    font-size: var(--t-sm);
  }
  .obj-warn {
    font-size: var(--t-xs);
    color: var(--glow-text);
  }
  .empty {
    font-size: var(--t-sm);
    color: var(--text-3);
    margin: var(--s-2) 2px;
  }
  .paint {
    margin-top: var(--s-5);
    padding-top: var(--s-4);
    border-top: 1px solid var(--line);
  }
  .walls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
    padding: 2px;
  }
  .wall {
    position: relative;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.18);
    transition: transform var(--dur) var(--ease);
  }
  .wall:hover {
    transform: scale(1.08);
  }
  .wall.picked {
    box-shadow:
      inset 0 0 0 1px oklch(0 0 0 / 0.18),
      0 0 0 2px var(--surface),
      0 0 0 3.5px var(--text);
  }
  /* The user's own paint: a spectrum until one is picked, then that paint. */
  .wall.any {
    background: var(--c, conic-gradient(red, yellow, lime, cyan, blue, magenta, red));
    cursor: pointer;
  }
  .wall.any input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
  }
  .wall.any:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .paint-tip {
    font-size: var(--t-xs);
    color: var(--text-3);
    margin: var(--s-2) 2px 0;
    line-height: 1.4;
  }
  .legend {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: auto;
    padding-top: var(--s-5);
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
</style>
