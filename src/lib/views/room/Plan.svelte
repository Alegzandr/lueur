<script lang="ts">
  import { app, edit, markRoomArranged } from "../../state.svelte";
  import { closestOnSegment, endpoints, facing, mapLight, EDGE_COLOR, EDGE_LETTER } from "../../geometry";
  import { t } from "../../i18n";
  import type { Vec2 } from "../../types";
  import { EDGES, SCREEN_MAX, SCREEN_MIN, SEAT, clampP, sub, type Other } from "./plan";

  // The room drawn from above: walls, screen, seat and lights, each dragged
  // into place. Selecting is the Room view's; this reports what was grabbed.
  let {
    other,
    dragging = $bindable(false),
    onselectlight,
    onselectother,
    onclear,
    ongrab,
  }: {
    /** The screen or corner selected, if any. */
    other: Other;
    /** True while something is being dragged. */
    dragging?: boolean;
    onselectlight: (id: string) => void;
    onselectother: (o: Other) => void;
    /** A press on the empty plan. */
    onclear: () => void;
    /** A drag began. */
    ongrab: () => void;
  } = $props();

  let svgEl = $state<SVGSVGElement>();

  // --- Dragging ---

  type Drag =
    | { kind: "light"; id: string; off: Vec2 }
    | { kind: "screen"; off: Vec2 }
    | { kind: "corner"; index: number; off: Vec2 }
    | { kind: "seat" }
    | { kind: "width" }
    | null;
  let drag: Drag = null;

  function toPlan(e: PointerEvent | MouseEvent): Vec2 {
    const ctm = svgEl?.getScreenCTM();
    if (!ctm) return { x: 0, y: 0 };
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    return { x: p.x, y: p.y };
  }

  function begin(e: PointerEvent, d: NonNullable<Drag>) {
    if (e.button !== 0) return;
    e.stopPropagation();
    ongrab();
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
    onselectother({ kind: "corner", index: best + 1 });
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
</script>

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
    onpointerdown={onclear}
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
        onselectother({ kind: "screen" });
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
          onselectother({ kind: "corner", index: i });
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
          onselectlight(l.id);
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

<style>
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
</style>
