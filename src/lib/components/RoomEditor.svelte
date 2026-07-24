<script lang="ts">
  import { config, updateConfig, notify } from "../stores";
  import * as ipc from "../ipc";
  import { classify, endpoints, facing, EDGE_COLOR, EDGE_LABEL } from "../geometry";
  import type { Light, Vec2 } from "../types";

  const VIEW_W = 1000;
  const VIEW_H = 750;
  const M = 10; // clamp margin

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
      zone_override: null,
      enabled: true,
    };
    updateConfig((c) => {
      c.room.lights.push(light);
      return c;
    });
    selected = { kind: "light", id: light.id };
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
      notify("info", "This light isn't bound to a bulb yet (Lights tab).");
      return;
    }
    try {
      await ipc.testDevice(b.ip, b.port, 155, 108, 255);
    } catch {
      notify("error", "Could not reach the bulb.");
    }
  }

  const wallPoints = $derived($config.room.walls.map((p) => `${p.x},${p.y}`).join(" "));
  const scr = $derived($config.room.screen);
  const ends = $derived(endpoints($config.room.screen));
  const faceVec = $derived(facing($config.room.screen));
</script>

<div class="editor">
  <div class="canvas-wrap card">
    <div class="toolbar">
      <button class="btn small" onclick={addLight}>+ Light</button>
      <button class="btn small" onclick={addCorner}>+ Corner</button>
      <button class="btn small ghost" onclick={resetRoom}>Reset room</button>
      <div class="grow"></div>
      <div class="hint faint">Drag the screen and lights to match your room</div>
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
          <linearGradient id="screenGrad" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" stop-color="#ff5c9d" />
            <stop offset="0.5" stop-color="#9b6cff" />
            <stop offset="1" stop-color="#38e8ff" />
          </linearGradient>
          <pattern id="grid" width="50" height="50" patternUnits="userSpaceOnUse">
            <path d="M50 0H0V50" fill="none" stroke="rgba(140,130,210,0.06)" stroke-width="1" />
          </pattern>
        </defs>

        <rect x="0" y="0" width={VIEW_W} height={VIEW_H} fill="url(#grid)" />

        <!-- Room outline -->
        <polygon
          points={wallPoints}
          fill="rgba(56,232,255,0.03)"
          stroke="rgba(150,140,230,0.4)"
          stroke-width="3"
          stroke-linejoin="round"
        />

        {#each $config.room.walls as w, i}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <rect
            class="corner"
            class:sel={selected?.kind === "corner" && selected.index === i}
            x={w.x - 9}
            y={w.y - 9}
            width="18"
            height="18"
            rx="4"
            onpointerdown={(e) => startCorner(e, i)}
          />
        {/each}

        <!-- Connectors + lights -->
        {#each $config.room.lights as l (l.id)}
          {@const cls = classify(scr, l)}
          {@const t = Math.max(0, Math.min(1, (cls.u + 1) / 2))}
          {@const target = {
            x: ends.left.x + (ends.right.x - ends.left.x) * t,
            y: ends.left.y + (ends.right.y - ends.left.y) * t,
          }}
          <line
            x1={l.pos.x}
            y1={l.pos.y}
            x2={target.x}
            y2={target.y}
            stroke={EDGE_COLOR[cls.edge]}
            stroke-width="1.5"
            stroke-dasharray="4 5"
            opacity={l.enabled ? 0.5 : 0.15}
          />
        {/each}

        <!-- Screen / PC -->
        <g class="screen" class:sel={selected?.kind === "screen"} onpointerdown={startScreen}>
          <!-- viewing direction -->
          <line
            x1={scr.center.x}
            y1={scr.center.y}
            x2={scr.center.x + faceVec.x * 70}
            y2={scr.center.y + faceVec.y * 70}
            stroke="rgba(236,235,255,0.5)"
            stroke-width="2"
            stroke-dasharray="3 4"
          />
          <circle cx={scr.center.x + faceVec.x * 70} cy={scr.center.y + faceVec.y * 70} r="6" fill="rgba(236,235,255,0.5)" />
          <line
            x1={ends.left.x}
            y1={ends.left.y}
            x2={ends.right.x}
            y2={ends.right.y}
            stroke="url(#screenGrad)"
            stroke-width="16"
            stroke-linecap="round"
          />
          <circle cx={scr.center.x} cy={scr.center.y} r="9" fill="#0c0b18" stroke="#ecebff" stroke-width="2" />
        </g>
        <text x={scr.center.x} y={scr.center.y - 18} class="screen-label" text-anchor="middle">
          Screen / PC
        </text>

        <!-- Light dots on top -->
        {#each $config.room.lights as l (l.id)}
          {@const cls = classify(scr, l)}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <g
            class="light"
            class:sel={selected?.kind === "light" && selected.id === l.id}
            onpointerdown={(e) => startLight(e, l)}
          >
            <circle
              cx={l.pos.x}
              cy={l.pos.y}
              r="15"
              fill={EDGE_COLOR[cls.edge]}
              opacity={l.enabled ? 1 : 0.35}
            />
            <circle cx={l.pos.x} cy={l.pos.y} r="15" fill="none" stroke="#0c0b18" stroke-width="2" />
            <text x={l.pos.x} y={l.pos.y + 34} class="light-label" text-anchor="middle">{l.name}</text>
          </g>
        {/each}
      </svg>
    </div>
  </div>

  <aside class="inspector">
    <div class="card pad">
      <div class="section-title">Screen / PC</div>
      <label class="field">
        <span>Facing <b>{$config.room.screen.facing_deg}°</b></span>
        <input
          type="range"
          min="-180"
          max="180"
          step="1"
          value={$config.room.screen.facing_deg}
          oninput={(e) => setFacing(+(e.target as HTMLInputElement).value)}
        />
      </label>
      <label class="field">
        <span>Width <b>{Math.round($config.room.screen.width)}</b></span>
        <input
          type="range"
          min="120"
          max="520"
          step="5"
          value={$config.room.screen.width}
          oninput={(e) => setWidth(+(e.target as HTMLInputElement).value)}
        />
      </label>
      <p class="tip faint">
        The dashed arrow points where you sit. Drag the screen's centre knob to move it.
      </p>
    </div>

    <div class="card pad">
      <div class="section-title">Selected</div>
      {#if selected?.kind === "light"}
        {#each $config.room.lights.filter((x) => selected?.kind === "light" && x.id === selected.id) as l (l.id)}
          {@const cls = classify($config.room.screen, l)}
          <div class="sel-name">{l.name}</div>
          <div class="chip" style="--c:{EDGE_COLOR[cls.edge]}">
            mirrors the {EDGE_LABEL[cls.edge]}
          </div>
          <div class="row" style="margin-top:14px; gap:8px;">
            <button class="btn small" onclick={() => identify(l.id)}>Identify</button>
            <button class="btn small ghost danger" onclick={removeSelected}>Remove</button>
          </div>
        {/each}
      {:else if selected?.kind === "corner"}
        <p class="muted">Room corner selected.</p>
        <button class="btn small ghost danger" onclick={removeSelected}>Remove corner</button>
      {:else if selected?.kind === "screen"}
        <p class="muted">Screen selected. Use the sliders above to rotate and resize it.</p>
      {:else}
        <p class="faint">Click a light, the screen, or a corner to edit it.</p>
      {/if}
    </div>

    <div class="card pad legend">
      <div class="section-title">Mapping</div>
      <div class="leg"><span class="d" style="background:{EDGE_COLOR.left}"></span> Left of screen → left edge</div>
      <div class="leg"><span class="d" style="background:{EDGE_COLOR.center}"></span> Behind screen → matching slice</div>
      <div class="leg"><span class="d" style="background:{EDGE_COLOR.right}"></span> Right of screen → right edge</div>
    </div>
  </aside>
</div>

<style>
  .editor {
    display: grid;
    grid-template-columns: 1fr 300px;
    gap: 20px;
    height: 100%;
  }
  .canvas-wrap {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 12px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }
  .hint {
    font-size: 12px;
  }
  .stage {
    flex: 1;
    min-height: 0;
    border-radius: 12px;
    overflow: hidden;
    background: radial-gradient(700px 500px at 50% 0%, rgba(155, 108, 255, 0.08), transparent 60%),
      rgba(8, 7, 16, 0.6);
    border: 1px solid var(--border);
  }
  svg {
    width: 100%;
    height: 100%;
    display: block;
    touch-action: none;
  }
  .corner {
    fill: #191833;
    stroke: rgba(150, 140, 230, 0.6);
    stroke-width: 2;
    cursor: grab;
  }
  .corner.sel {
    stroke: var(--cyan);
    fill: #22284a;
  }
  .screen {
    cursor: grab;
  }
  .screen.sel circle {
    stroke: var(--cyan);
  }
  .screen-label {
    fill: var(--text-dim);
    font-size: 15px;
    font-weight: 600;
    pointer-events: none;
  }
  .light {
    cursor: grab;
  }
  .light.sel circle:nth-child(2) {
    stroke: #fff;
    stroke-width: 3;
  }
  .light-label {
    fill: var(--text-dim);
    font-size: 14px;
    pointer-events: none;
  }
  .inspector {
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow: auto;
  }
  .pad {
    padding: 16px;
  }
  .field {
    display: block;
    margin-bottom: 14px;
  }
  .field span {
    display: block;
    font-size: 13px;
    color: var(--text-dim);
    margin-bottom: 8px;
  }
  .tip,
  .field span b {
    color: var(--text);
  }
  .tip {
    font-size: 12px;
    line-height: 1.5;
    margin: 6px 0 0;
  }
  .sel-name {
    font-size: 15px;
    font-weight: 600;
    margin-bottom: 8px;
  }
  .chip {
    display: inline-block;
    font-size: 12.5px;
    padding: 5px 10px;
    border-radius: 999px;
    color: var(--c);
    border: 1px solid var(--c);
    background: color-mix(in srgb, var(--c) 12%, transparent);
  }
  .btn.small {
    padding: 7px 12px;
    font-size: 13px;
  }
  .danger:hover {
    color: var(--red);
  }
  .legend .leg {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12.5px;
    color: var(--text-dim);
    margin-bottom: 8px;
  }
  .legend .d {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex: none;
  }
</style>
