<script lang="ts">
  import {
    app,
    edit,
    addLight,
    bindingFor,
    identifyLight,
    lightById,
    notify,
    removeLight,
    renameLight,
    setLightEnabled,
  } from "../state.svelte";
  import { describe, endpoints, facing, mapLight, EDGE_COLOR, EDGE_LABEL, type Edge } from "../geometry";
  import { defaultConfig, type Vec2 } from "../types";
  import Icon from "../ui/Icon.svelte";
  import Slider from "../ui/Slider.svelte";
  import Switch from "../ui/Switch.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

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

  // Selection: a light lives in shared state (other views link to it); the
  // screen and corners are only selectable here.
  type Other = { kind: "screen" } | { kind: "corner"; index: number } | null;
  let other = $state<Other>(null);

  const selLight = $derived(app.selectedLight ? lightById(app.selectedLight) : undefined);

  function selectLight(id: string) {
    app.selectedLight = id;
    other = null;
  }
  function selectOther(o: Other) {
    app.selectedLight = null;
    other = o;
  }
  function clearSelection() {
    app.selectedLight = null;
    other = null;
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
    drag = null;
    dragging = false;
    held = null;
  }

  // --- Editing ---

  function newLight() {
    const l = addLight();
    selectLight(l.id);
  }

  /** Double-click on a wall drops a corner exactly there. */
  function insertCornerAt(e: MouseEvent) {
    const p = toPlan(e);
    const w = app.config.room.walls;
    let best = -1;
    let bestDist = Infinity;
    let bestPoint = p;
    for (let i = 0; i < w.length; i++) {
      const a = w[i];
      const b = w[(i + 1) % w.length];
      const ab = sub(b, a);
      const len2 = ab.x * ab.x + ab.y * ab.y || 1;
      const t = Math.max(0, Math.min(1, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2));
      const q = { x: a.x + ab.x * t, y: a.y + ab.y * t };
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
  function addCorner() {
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
    selectOther({ kind: "corner", index: best + 1 });
  }

  function removeCorner(index: number) {
    if (app.config.room.walls.length <= 3) {
      notify("info", "A room needs at least three corners.");
      return;
    }
    edit((c) => {
      c.room.walls.splice(index, 1);
    });
    other = null;
  }

  function resetShape() {
    edit((c) => {
      c.room.walls = structuredClone(DEF.room.walls);
      c.room.screen = structuredClone(DEF.room.screen);
    });
    clearSelection();
    notify("info", "Walls and screen reset. Your lights stayed where they were.");
  }

  function removeSelected() {
    if (selLight) removeLight(selLight.id);
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
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t.closest("input, select, textarea")) return;
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

  /** A light's name sits below it, or above when below would run into the
   *  screen. */
  function labelPos(p: Vec2): Vec2 {
    const below = { x: p.x, y: p.y + 24 * k };
    return distToScreen(below) < 18 * k ? { x: p.x, y: p.y - 16 * k } : below;
  }

  function distToScreen(p: Vec2): number {
    const a = ends.left;
    const ab = sub(ends.right, a);
    const len2 = ab.x * ab.x + ab.y * ab.y || 1;
    const t = Math.max(0, Math.min(1, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2));
    return Math.hypot(p.x - (a.x + ab.x * t), p.y - (a.y + ab.y * t));
  }

  const heightLabel = (v: number) =>
    v < 0.18 ? "Floor" : v < 0.42 ? "Low" : v < 0.58 ? "Screen level" : v < 0.82 ? "High" : "Ceiling";

  const EDGES: Edge[] = ["left", "center", "right"];
</script>

<svelte:window onkeydown={onKey} />

<div class="room">
  <section class="stage-col">
    <header class="bar">
      <h1 class="title">Room</h1>
      <div class="tools">
        <button class="btn sm" onclick={newLight}><Icon name="bulb" size={14} /> Add light</button>
        <button class="btn sm" onclick={addCorner}><Icon name="corner" size={14} /> Add corner</button>
        <button class="btn plain sm" onclick={resetShape}>Reset shape</button>
      </div>
    </header>

    <div class="banner-slot"><RestartBanner /></div>

    <div class="stage" class:dragging bind:clientWidth={stageW} bind:clientHeight={stageH}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <svg
        bind:this={svgEl}
        viewBox="{box.x} {box.y} {box.w} {box.h}"
        style="--k:{k}"
        preserveAspectRatio="xMidYMid meet"
        onpointerdown={clearSelection}
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
          <title>Where you sit. Drag to turn the screen.</title>
          <circle cx={seat.x} cy={seat.y} r={14 * k} class="seat-hit" />
          <circle cx={seat.x} cy={seat.y} r={7 * k} class="seat-dot" />
        </g>
        <text x={seat.x} y={seat.y + 22 * k} class="caption" text-anchor="middle">You</text>

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
              <title>Drag to resize the screen</title>
            </circle>
          {/each}
        {/if}
        <text
          x={scr.center.x - face.x * 18 * k}
          y={scr.center.y - face.y * 18 * k + 4 * k}
          class="caption"
          text-anchor="middle">Screen</text
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
            <circle cx={l.pos.x} cy={l.pos.y} r={8 * k} class="bulb" fill={EDGE_COLOR[m.edge]} />
            <text x={labelPos(l.pos).x} y={labelPos(l.pos).y} class="label" text-anchor="middle">{l.name}</text>
          </g>
        {/each}
      </svg>

      <p class="help">
        Drag to arrange. Double-click a wall to add a corner. <kbd>Del</kbd> removes,
        arrows nudge.
      </p>
    </div>
  </section>

  <aside class="inspector">
    {#if selLight}
      {@const l = selLight}
      {@const m = mapLight(scr, l)}
      {@const b = bindingFor(l.id)}
      <button class="back" onclick={clearSelection}><Icon name="back" size={14} /> All objects</button>

      <input
        class="name"
        type="text"
        value={l.name}
        aria-label="Light name"
        spellcheck="false"
        oninput={(e) => renameLight(l.id, e.currentTarget.value)}
      />
      <p class="what" style="--c:{EDGE_COLOR[m.edge]}">
        <span class="swatch"></span>{describe(m)}
      </p>

      <figure class="preview">
        <svg viewBox="0 0 160 90" role="img" aria-label="The part of the screen this light mirrors">
          <rect x="0.5" y="0.5" width="159" height="89" rx="4" class="preview-bg" />
          <rect
            x={m.zone.x * 160}
            y={m.zone.y * 90}
            width={m.zone.w * 160}
            height={m.zone.h * 90}
            rx="3"
            fill={EDGE_COLOR[m.edge]}
            fill-opacity={0.18 + 0.22 * (1 - m.spread)}
            stroke={EDGE_COLOR[m.edge]}
            stroke-width="1.5"
          />
        </svg>
        <figcaption>What it watches on screen</figcaption>
      </figure>

      <div class="field">
        <div class="field-label">Height in the room</div>
        <Slider
          label="Height in the room"
          value={l.height}
          min={0}
          max={1}
          step={0.01}
          def={0.5}
          format={heightLabel}
          ends={["Floor", "Ceiling"]}
          onchange={(v) => edit((c) => {
            const x = c.room.lights.find((y) => y.id === l.id);
            if (x) x.height = v;
          })}
        />
      </div>

      <div class="field inline">
        <div>
          <div class="field-label">Follows the picture</div>
          <p class="field-hint">Off leaves this bulb alone.</p>
        </div>
        <Switch checked={l.enabled} label="Follows the picture" onchange={(v) => setLightEnabled(l.id, v)} />
      </div>

      <dl class="facts">
        <div><dt>Brightness</dt><dd class="num">{Math.round(m.gain * 100)}%</dd></div>
        <div><dt>Response</dt><dd class="num">{Math.round(m.responsiveness * 100)}%</dd></div>
      </dl>
      <p class="field-hint">Lights further from the screen glow softer and move slower, like real bounced light.</p>

      <div class="bulb-box">
        {#if b}
          <div class="bulb-info">
            <div class="field-label">{b.name || "Yeelight"}</div>
            <div class="field-hint num">{b.ip}</div>
          </div>
          <button class="btn sm" onclick={() => identifyLight(l.id)}><Icon name="flash" size={14} /> Identify</button>
        {:else}
          <div class="bulb-info">
            <div class="field-label">No bulb assigned</div>
            <div class="field-hint">Pick which bulb this is.</div>
          </div>
          <button class="btn sm" onclick={() => (app.view = "bulbs")}>Assign</button>
        {/if}
      </div>

      <button class="btn plain danger remove" onclick={() => removeLight(l.id)}>
        <Icon name="trash" size={14} /> Remove light
      </button>
    {:else if other?.kind === "screen"}
      <button class="back" onclick={clearSelection}><Icon name="back" size={14} /> All objects</button>
      <h2 class="name static">Screen</h2>
      <p class="field-hint">Drag the screen to move it, its ends to resize it, and the seat to turn it.</p>
      <div class="field">
        <div class="field-label">Facing</div>
        <Slider
          label="Facing"
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
        <div class="field-label">Width</div>
        <Slider
          label="Width"
          value={scr.width}
          min={SCREEN_MIN}
          max={SCREEN_MAX}
          step={5}
          def={DEF.room.screen.width}
          format={(v) => String(Math.round(v))}
          onchange={(v) => edit((c) => (c.room.screen.width = v))}
        />
      </div>
    {:else if other?.kind === "corner"}
      {@const index = other.index}
      <button class="back" onclick={clearSelection}><Icon name="back" size={14} /> All objects</button>
      <h2 class="name static">Corner {index + 1}</h2>
      <p class="field-hint">Drag it to reshape the room. Double-click any wall to add another.</p>
      <button class="btn plain danger remove" onclick={() => removeCorner(index)}>
        <Icon name="trash" size={14} /> Remove corner
      </button>
    {:else}
      <h2 class="side-title">Objects</h2>
      <ul class="objects">
        <li>
          <button class="obj" onclick={() => selectOther({ kind: "screen" })}>
            <Icon name="screen" size={16} />
            <span class="obj-name">Screen</span>
            <span class="obj-meta num">{scr.facing_deg}°</span>
          </button>
        </li>
        {#each room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          <li>
            <button class="obj" class:off={!l.enabled} onclick={() => selectLight(l.id)}>
              <span class="obj-dot" style="background:{EDGE_COLOR[m.edge]}"></span>
              <span class="obj-name">{l.name}</span>
              {#if !bindingFor(l.id)}<span class="obj-warn" title="No bulb assigned">No bulb</span>{/if}
              <Icon name="chevron" size={14} />
            </button>
          </li>
        {/each}
      </ul>
      {#if room.lights.length === 0}
        <p class="empty">No lights yet. Add them from Bulbs, or place one by hand.</p>
      {/if}
      <button class="btn sm add" onclick={newLight}><Icon name="plus" size={14} /> Add light</button>

      <h2 class="side-title">How it maps</h2>
      <ul class="rules">
        <li><b>Across</b> the screen picks the slice of the picture.</li>
        <li><b>Height</b> picks the band, floor to ceiling.</li>
        <li><b>Distance</b> widens the view, then softens and slows it.</li>
      </ul>
      <div class="legend">
        {#each EDGES as edge}
          <span><i style="background:{EDGE_COLOR[edge]}"></i>{EDGE_LABEL[edge]}</span>
        {/each}
      </div>
    {/if}
  </aside>
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
  .title {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 500;
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
    box-shadow: inset 0 0 0 1px var(--line);
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
    left: var(--s-3);
    bottom: var(--s-3);
    padding: 4px var(--s-2);
    border-radius: var(--r-sm);
    background: color-mix(in oklab, var(--surface) 85%, transparent);
    font-size: var(--t-xs);
    color: var(--text-3);
    pointer-events: none;
  }

  .dot {
    fill: var(--plan-grid);
  }
  .floor {
    fill: var(--plan-floor);
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
    stroke: var(--glow);
  }
  .handle {
    fill: var(--surface);
    stroke: var(--glow);
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
    fill: var(--glow);
    stroke: var(--glow);
  }

  .light {
    cursor: grab;
  }
  .ring {
    fill: none;
    stroke: var(--glow);
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
    stroke: var(--plan-floor);
    stroke-width: calc(2.5px * var(--k));
  }
  .light.off .bulb {
    fill: var(--line-strong);
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
    padding: var(--s-5);
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: stretch;
  }
  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin: 0 0 var(--s-4) -4px;
    padding: 2px 6px 2px 2px;
    border-radius: var(--r-sm);
    color: var(--text-2);
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .back:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .name {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 500;
    height: 38px;
    margin-left: -9px;
    width: calc(100% + 9px);
    border-color: transparent;
    background: transparent;
  }
  .name:hover {
    border-color: var(--line-strong);
  }
  .name.static {
    margin: 0 0 var(--s-2);
    height: auto;
    width: auto;
  }
  .what {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    margin-top: 2px;
    color: var(--text-2);
  }
  .swatch {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
  }
  .preview {
    margin: var(--s-4) 0 var(--s-5);
  }
  .preview svg {
    display: block;
    width: 100%;
    height: auto;
  }
  .preview-bg {
    fill: var(--surface);
    stroke: var(--line-strong);
  }
  figcaption {
    margin-top: var(--s-1);
    font-size: var(--t-xs);
    color: var(--text-3);
    text-align: center;
  }
  .field {
    margin-bottom: var(--s-5);
  }
  .field.inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-3);
  }
  .field :global(.slider) {
    min-width: 0;
  }
  .field-label {
    font-weight: 500;
    margin-bottom: 2px;
  }
  .field-hint {
    font-size: var(--t-sm);
    color: var(--text-3);
    text-wrap: pretty;
  }
  .facts {
    display: flex;
    gap: var(--s-6);
    margin: 0 0 var(--s-1);
  }
  .facts dt {
    font-size: var(--t-xs);
    color: var(--text-3);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .facts dd {
    margin: 0;
    font-size: var(--t-lg);
    font-weight: 500;
  }
  .bulb-box {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    margin-top: var(--s-5);
    padding: var(--s-3);
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--shadow-1);
  }
  .bulb-info {
    flex: 1;
    min-width: 0;
  }
  .remove {
    align-self: flex-start;
    margin-top: var(--s-5);
    margin-left: -8px;
  }

  .side-title {
    font-size: var(--t-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
    margin-bottom: var(--s-2);
  }
  .side-title:not(:first-child) {
    margin-top: var(--s-6);
  }
  .objects {
    list-style: none;
    margin: 0 -8px;
    padding: 0;
  }
  .obj {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
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
  .obj-dot {
    width: 10px;
    height: 10px;
    margin: 0 3px;
    border-radius: 50%;
    flex: none;
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
    margin: var(--s-2) 0;
  }
  .add {
    align-self: flex-start;
    margin-top: var(--s-3);
  }
  .rules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .rules b {
    color: var(--text);
    font-weight: 600;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1) var(--s-3);
    margin-top: var(--s-4);
    font-size: var(--t-xs);
    color: var(--text-3);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }
</style>
