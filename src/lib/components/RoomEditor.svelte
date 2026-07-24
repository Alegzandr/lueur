<script lang="ts">
  import { config, updateConfig, notify } from "../stores";
  import * as ipc from "../ipc";
  import { describe, endpoints, facing, mapLight, EDGE_COLOR, EDGE_LABEL } from "../geometry";
  import { defaultConfig, type Light, type Vec2 } from "../types";
  import Slider from "./Slider.svelte";
  import Icon from "./Icon.svelte";

  const DEF = defaultConfig();

  const VIEW_W = 1000;
  const VIEW_H = 750;
  const M = 10; // clamp margin

  // Warm white, matching the Lights panel, so an identify flash always looks the
  // same wherever it is triggered from.
  const IDENTIFY_RGB: [number, number, number] = [255, 190, 120];

  let svgEl: SVGSVGElement;

  type Sel = { kind: "light"; id: string } | { kind: "screen" } | { kind: "corner"; index: number } | null;
  let selected = $state<Sel>(null);

  type Drag =
    | { kind: "light"; id: string; off: Vec2 }
    | { kind: "screen"; off: Vec2 }
    | { kind: "corner"; index: number; off: Vec2 }
    | null;
  let drag: Drag = null;

  function toSvg(e: PointerEvent): Vec2 {
    const ctm = svgEl.getScreenCTM();
    if (!ctm) return { x: 0, y: 0 };
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    return { x: p.x, y: p.y };
  }
  const sub = (a: Vec2, b: Vec2): Vec2 => ({ x: a.x - b.x, y: a.y - b.y });
  const clampP = (p: Vec2): Vec2 => ({
    x: Math.max(M, Math.min(VIEW_W - M, p.x)),
    y: Math.max(M, Math.min(VIEW_H - M, p.y)),
  });

  function startLight(e: PointerEvent, l: Light) {
    e.stopPropagation();
    selected = { kind: "light", id: l.id };
    drag = { kind: "light", id: l.id, off: sub(toSvg(e), l.pos) };
    svgEl.setPointerCapture(e.pointerId);
  }
  function startScreen(e: PointerEvent) {
    e.stopPropagation();
    selected = { kind: "screen" };
    drag = { kind: "screen", off: sub(toSvg(e), $config.room.screen.center) };
    svgEl.setPointerCapture(e.pointerId);
  }
  function startCorner(e: PointerEvent, index: number) {
    e.stopPropagation();
    selected = { kind: "corner", index };
    drag = { kind: "corner", index, off: sub(toSvg(e), $config.room.walls[index]) };
    svgEl.setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!drag) return;
    const p = clampP(sub(toSvg(e), drag.off));
    if (drag.kind === "light") {
      const id = drag.id;
      updateConfig((c) => {
        const l = c.room.lights.find((x) => x.id === id);
        if (l) l.pos = p;
        return c;
      });
    } else if (drag.kind === "screen") {
      updateConfig((c) => {
        c.room.screen.center = p;
        return c;
      });
    } else {
      const index = drag.index;
      updateConfig((c) => {
        if (c.room.walls[index]) c.room.walls[index] = p;
        return c;
      });
    }
  }
  function endDrag(e: PointerEvent) {
    if (drag) {
      try {
        svgEl.releasePointerCapture(e.pointerId);
      } catch {
        /* pointer may already be released */
      }
    }
    drag = null;
  }

  function deselect() {
    selected = null;
  }

  // --- Editing actions ---
  function setFacing(v: number) {
    updateConfig((c) => {
      c.room.screen.facing_deg = v;
      return c;
    });
  }
  function setWidth(v: number) {
    updateConfig((c) => {
      c.room.screen.width = v;
      return c;
    });
  }
  function addLight() {
    const n = $config.room.lights.length;
    const light: Light = {
      id: crypto.randomUUID(),
      name: `Light ${n + 1}`,
      pos: { x: 200 + ((n * 140) % 620), y: 130 },
      height: 0.5,
      zone_override: null,
      enabled: true,
    };
    updateConfig((c) => {
      c.room.lights.push(light);
      return c;
    });
    selected = { kind: "light", id: light.id };
  }
  function setHeight(id: string, v: number) {
    updateConfig((c) => {
      const l = c.room.lights.find((x) => x.id === id);
      if (l) l.height = v;
      return c;
    });
  }
  function removeSelected() {
    if (!selected) return;
    if (selected.kind === "light") {
      const id = selected.id;
      updateConfig((c) => {
        c.room.lights = c.room.lights.filter((x) => x.id !== id);
        c.bindings = c.bindings.filter((x) => x.light_id !== id);
        return c;
      });
      selected = null;
    } else if (selected.kind === "corner") {
      if ($config.room.walls.length <= 3) {
        notify("info", "A room needs at least 3 corners.");
        return;
      }
      const index = selected.index;
      updateConfig((c) => {
        c.room.walls.splice(index, 1);
        return c;
      });
      selected = null;
    }
  }
  function addCorner() {
    // Insert a point at the midpoint of the longest wall edge.
    updateConfig((c) => {
      const w = c.room.walls;
      if (w.length < 2) return c;
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
      const a = w[best];
      const b = w[(best + 1) % w.length];
      w.splice(best + 1, 0, { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });
      return c;
    });
  }
  function resetRoom() {
    updateConfig((c) => {
      c.room.walls = [
        { x: 60, y: 60 },
        { x: 940, y: 60 },
        { x: 940, y: 690 },
        { x: 60, y: 690 },
      ];
      c.room.screen = { center: { x: 500, y: 150 }, width: 300, facing_deg: 180 };
      return c;
    });
  }

  function bindingFor(id: string) {
    return $config.bindings.find((b) => b.light_id === id);
  }
  async function identify(id: string) {
    const b = bindingFor(id);
    if (!b) {
      notify("info", "This light is not bound to a bulb yet. Bind it in the Lights tab.");
      return;
    }
    try {
      await ipc.testDevice(b.ip, b.port, ...IDENTIFY_RGB);
    } catch {
      notify("error", "Could not reach the bulb.");
    }
  }

  const wallPoints = $derived($config.room.walls.map((p) => `${p.x},${p.y}`).join(" "));
  const scr = $derived($config.room.screen);
  const ends = $derived(endpoints($config.room.screen));
  const faceVec = $derived(facing($config.room.screen));
  const selectedLight = $derived.by(() => {
    const s = selected;
    if (s?.kind !== "light") return undefined;
    return $config.room.lights.find((l) => l.id === s.id);
  });

  const heightLabel = (v: number) =>
    v < 0.18 ? "floor" : v < 0.42 ? "low" : v < 0.58 ? "screen level" : v < 0.82 ? "high" : "ceiling";
</script>

<div class="editor">
  <div class="stage-col">
    <div class="toolbar">
      <button class="btn small" onclick={addLight}><Icon name="plus" size={14} /> Light</button>
      <button class="btn small" onclick={addCorner}><Icon name="plus" size={14} /> Corner</button>
      <button class="btn ghost small" onclick={resetRoom}>Reset room</button>
      <span class="grow"></span>
      <span class="hint faint">Drag the screen and lights to match your room</span>
    </div>

    <div class="stage">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <svg
        bind:this={svgEl}
        viewBox="0 0 {VIEW_W} {VIEW_H}"
        preserveAspectRatio="xMidYMid meet"
        onpointerdown={deselect}
        onpointermove={onMove}
        onpointerup={endDrag}
        onpointercancel={endDrag}
      >
        <defs>
          <pattern id="grid" width="50" height="50" patternUnits="userSpaceOnUse">
            <path d="M50 0H0V50" fill="none" stroke="var(--border)" stroke-width="1" />
          </pattern>
        </defs>

        <rect x="0" y="0" width={VIEW_W} height={VIEW_H} fill="url(#grid)" />

        <!-- Room outline -->
        <polygon
          points={wallPoints}
          fill="var(--surface)"
          fill-opacity="0.55"
          stroke="var(--border-strong)"
          stroke-width="2"
          stroke-linejoin="round"
        />

        {#each $config.room.walls as w, i}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <rect
            class="corner"
            class:sel={selected?.kind === "corner" && selected.index === i}
            x={w.x - 8}
            y={w.y - 8}
            width="16"
            height="16"
            rx="3"
            onpointerdown={(e) => startCorner(e, i)}
          />
        {/each}

        <!-- Connectors: opacity tracks the distance falloff the engine applies -->
        {#each $config.room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          {@const t = Math.max(0, Math.min(1, (m.u + 1) / 2))}
          {@const target = {
            x: ends.left.x + (ends.right.x - ends.left.x) * t,
            y: ends.left.y + (ends.right.y - ends.left.y) * t,
          }}
          <line
            x1={l.pos.x}
            y1={l.pos.y}
            x2={target.x}
            y2={target.y}
            stroke={EDGE_COLOR[m.edge]}
            stroke-width="1.5"
            stroke-dasharray="3 6"
            stroke-linecap="round"
            opacity={l.enabled ? 0.15 + 0.4 * m.gain : 0.1}
          />
        {/each}

        <!-- Screen, and the direction you sit in -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <g class="screen" class:sel={selected?.kind === "screen"} onpointerdown={startScreen}>
          <line
            x1={scr.center.x}
            y1={scr.center.y}
            x2={scr.center.x + faceVec.x * 72}
            y2={scr.center.y + faceVec.y * 72}
            stroke="var(--text-3)"
            stroke-width="1.5"
            stroke-dasharray="3 5"
          />
          <circle
            cx={scr.center.x + faceVec.x * 72}
            cy={scr.center.y + faceVec.y * 72}
            r="5"
            fill="none"
            stroke="var(--text-3)"
            stroke-width="1.5"
          />
          <line
            class="screen-face"
            x1={ends.left.x}
            y1={ends.left.y}
            x2={ends.right.x}
            y2={ends.right.y}
            stroke-width="12"
            stroke-linecap="round"
          />
          <circle class="knob" cx={scr.center.x} cy={scr.center.y} r="7" />
        </g>
        <text x={scr.center.x} y={scr.center.y - 20} class="screen-label" text-anchor="middle">
          Screen
        </text>

        <!-- Lights on top -->
        {#each $config.room.lights as l (l.id)}
          {@const m = mapLight(scr, l)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g
            class="light"
            class:sel={selected?.kind === "light" && selected.id === l.id}
            onpointerdown={(e) => startLight(e, l)}
          >
            <circle
              class="ring"
              cx={l.pos.x}
              cy={l.pos.y}
              r="17"
              fill="none"
              stroke={EDGE_COLOR[m.edge]}
              stroke-width="2"
            />
            <circle
              cx={l.pos.x}
              cy={l.pos.y}
              r="10"
              fill={EDGE_COLOR[m.edge]}
              opacity={l.enabled ? 0.45 + 0.55 * m.gain : 0.25}
            />
            <text x={l.pos.x} y={l.pos.y + 32} class="light-label" text-anchor="middle">
              {l.name}
            </text>
          </g>
        {/each}
      </svg>
    </div>
  </div>

  <aside class="inspector panel">
    <section class="block">
      <h2 class="section-title">Screen</h2>
      <div class="fields">
        <Slider
          label="Facing"
          value={$config.room.screen.facing_deg}
          min={-180}
          max={180}
          step={1}
          def={DEF.room.screen.facing_deg}
          format={(v) => `${v}°`}
          tip="The dashed line points at where you sit. Drag the centre knob to move the screen."
          onchange={setFacing}
        />
        <Slider
          label="Width"
          value={$config.room.screen.width}
          min={120}
          max={520}
          step={5}
          def={DEF.room.screen.width}
          format={(v) => String(Math.round(v))}
          onchange={setWidth}
        />
      </div>
    </section>

    <section class="block">
      <h2 class="section-title">Selection</h2>
      {#if selectedLight}
        {@const m = mapLight(scr, selectedLight)}
        <div class="sel-name">{selectedLight.name}</div>
        <div class="chip" style="--c:{EDGE_COLOR[m.edge]}">{describe(m)}</div>

        <!-- What this light actually samples, drawn on a 16:9 screen. -->
        <svg class="preview" viewBox="0 0 160 90" role="img" aria-label="Sampled region of the screen">
          <rect x="0.5" y="0.5" width="159" height="89" rx="3" class="preview-bg" />
          <rect
            x={m.zone.x * 160}
            y={m.zone.y * 90}
            width={m.zone.w * 160}
            height={m.zone.h * 90}
            rx="2"
            fill={EDGE_COLOR[m.edge]}
            fill-opacity={0.12 + 0.2 * (1 - m.spread)}
            stroke={EDGE_COLOR[m.edge]}
            stroke-width="1.5"
          />
        </svg>

        <div class="fields">
          <Slider
            label="Height in the room"
            value={selectedLight.height}
            min={0}
            max={1}
            step={0.01}
            def={0.5}
            format={heightLabel}
            tip="Picks the horizontal band of the picture this light mirrors."
            onchange={(v) => setHeight(selectedLight.id, v)}
          />
        </div>

        <dl class="stats">
          <div><dt>Brightness</dt><dd>×{m.gain.toFixed(2)}</dd></div>
          <div><dt>Response</dt><dd>×{m.responsiveness.toFixed(2)}</dd></div>
        </dl>

        <div class="actions">
          <button class="btn small" onclick={() => identify(selectedLight.id)}>Identify</button>
          <button class="btn ghost small danger" onclick={removeSelected}>
            <Icon name="trash" size={14} /> Remove
          </button>
        </div>
      {:else if selected?.kind === "corner"}
        <p class="note muted">Room corner selected.</p>
        <div class="actions">
          <button class="btn ghost small danger" onclick={removeSelected}>
            <Icon name="trash" size={14} /> Remove corner
          </button>
        </div>
      {:else if selected?.kind === "screen"}
        <p class="note muted">Screen selected. Use the sliders above to rotate and resize it.</p>
      {:else}
        <p class="note faint">Click a light, the screen, or a corner to edit it.</p>
      {/if}
    </section>

    <section class="block">
      <h2 class="section-title">Mapping</h2>
      <ul class="rules">
        <li>Sideways position picks the slice of the picture.</li>
        <li>Height picks the band, floor to ceiling.</li>
        <li>Distance widens the slice, then dims and slows it.</li>
      </ul>
      <p class="note faint">
        Close behind the screen a light mirrors a tight slice. Out across the room it settles into
        the whole picture, softer and slower, the way indirect light actually behaves.
      </p>
      <div class="key">
        {#each ["left", "center", "right"] as const as edge}
          <span class="key-item">
            <span class="d" style="background:{EDGE_COLOR[edge]}"></span>
            {EDGE_LABEL[edge]}
          </span>
        {/each}
      </div>
    </section>
  </aside>
</div>

<style>
  .editor {
    display: grid;
    grid-template-columns: 1fr 296px;
    gap: var(--s-5);
    height: 100%;
  }

  .stage-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: var(--s-3);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex: none;
  }
  .btn.small {
    min-height: 28px;
    padding: 0 var(--s-2);
    font-size: var(--t-2);
  }
  .hint {
    font-size: var(--t-2);
  }

  .stage {
    flex: 1;
    min-height: 0;
    border-radius: var(--r-2);
    overflow: hidden;
    background: var(--surface-sunken);
    border: 1px solid var(--border);
  }
  svg {
    width: 100%;
    height: 100%;
    display: block;
    touch-action: none;
  }

  .corner {
    fill: var(--surface-raised);
    stroke: var(--border-strong);
    stroke-width: 1.5;
    cursor: grab;
    transition:
      fill var(--dur) var(--ease),
      stroke var(--dur) var(--ease);
  }
  .corner:hover {
    fill: var(--surface-hover);
    stroke: var(--text-3);
  }
  .corner.sel {
    fill: var(--accent);
    stroke: var(--accent);
  }

  .screen {
    cursor: grab;
  }
  .screen-face {
    stroke: var(--text-2);
    transition: stroke var(--dur) var(--ease);
  }
  .knob {
    fill: var(--surface-sunken);
    stroke: var(--text-2);
    stroke-width: 2;
    transition:
      fill var(--dur) var(--ease),
      stroke var(--dur) var(--ease);
  }
  .screen:hover .screen-face,
  .screen:hover .knob {
    stroke: var(--text);
  }
  /* Selection fills the knob rather than brightening the stroke, so it stays
     distinct from hover now that both resolve to white. */
  .screen.sel .screen-face,
  .screen.sel .knob {
    stroke: var(--text);
  }
  .screen.sel .knob {
    fill: var(--text);
  }
  .screen-label {
    fill: var(--text-3);
    font-size: 13px;
    font-weight: 500;
    pointer-events: none;
  }

  .light {
    cursor: grab;
  }
  /* The ring is the selection affordance; it stays out of the way until needed. */
  .ring {
    opacity: 0;
    transition: opacity var(--dur) var(--ease);
  }
  .light:hover .ring {
    opacity: 0.45;
  }
  .light.sel .ring {
    opacity: 1;
    stroke: var(--text);
  }
  .light-label {
    fill: var(--text-3);
    font-size: 13px;
    pointer-events: none;
  }

  .inspector {
    display: flex;
    flex-direction: column;
    overflow: auto;
    padding: var(--s-4);
    gap: var(--s-5);
  }
  .block + .block {
    border-top: 1px solid var(--border);
    padding-top: var(--s-5);
  }
  .block .section-title {
    display: block;
    margin-bottom: var(--s-3);
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
  }

  .sel-name {
    font-size: var(--t-4);
    font-weight: 600;
    margin-bottom: var(--s-2);
  }
  .chip {
    display: inline-block;
    font-size: var(--t-1);
    padding: 3px var(--s-2);
    border-radius: var(--r-full);
    color: var(--c);
    background: color-mix(in oklab, var(--c) 18%, transparent);
  }
  .preview {
    display: block;
    width: 100%;
    height: auto;
    margin: var(--s-4) 0;
  }
  .preview-bg {
    fill: var(--surface-sunken);
    stroke: var(--border);
    stroke-width: 1;
  }

  .stats {
    display: flex;
    gap: var(--s-5);
    margin: var(--s-4) 0 0;
    font-size: var(--t-1);
  }
  .stats dt {
    color: var(--text-3);
  }
  .stats dd {
    margin: 2px 0 0;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .note {
    font-size: var(--t-2);
    line-height: 1.5;
    margin: 0;
  }
  .actions {
    display: flex;
    gap: var(--s-2);
    margin-top: var(--s-4);
  }

  .rules {
    list-style: none;
    margin: 0 0 var(--s-3);
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
    font-size: var(--t-2);
    color: var(--text-2);
    line-height: 1.4;
  }
  .rules li {
    position: relative;
    padding-left: var(--s-3);
  }
  .rules li::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0.55em;
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: var(--text-3);
  }

  .key {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1) var(--s-3);
    margin-top: var(--s-3);
    padding-top: var(--s-3);
    border-top: 1px solid var(--border);
  }
  .key-item {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    font-size: var(--t-1);
    color: var(--text-3);
  }
  .key .d {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
  }
</style>
