<script lang="ts">
  // A small, read-only drawing of the room: walls, screen and the lights,
  // each glowing in the colour of the screen zone it mirrors.
  import { app } from "../state.svelte";
  import { endpoints, mapLight, EDGE_COLOR } from "../geometry";

  interface Props {
    live?: boolean;
  }
  let { live = false }: Props = $props();

  const room = $derived(app.config.room);
  const box = $derived.by(() => {
    const pts = [...room.walls, ...room.lights.map((l) => l.pos), room.screen.center];
    const xs = pts.map((p) => p.x);
    const ys = pts.map((p) => p.y);
    const pad = 40;
    const x = Math.min(...xs) - pad;
    const y = Math.min(...ys) - pad;
    return { x, y, w: Math.max(...xs) + pad - x, h: Math.max(...ys) + pad - y };
  });
  const walls = $derived(room.walls.map((p) => `${p.x},${p.y}`).join(" "));
  const ends = $derived(endpoints(room.screen));
</script>

<svg
  class="thumb"
  class:live
  viewBox="{box.x} {box.y} {box.w} {box.h}"
  preserveAspectRatio="xMidYMid meet"
  role="img"
  aria-label="Your room"
>
  <defs>
    {#each ["left", "center", "right"] as edge}
      <radialGradient id="thumb-{edge}">
        <stop offset="0" stop-color={EDGE_COLOR[edge as "left"]} stop-opacity="0.75" />
        <stop offset="1" stop-color={EDGE_COLOR[edge as "left"]} stop-opacity="0" />
      </radialGradient>
    {/each}
  </defs>
  <polygon points={walls} class="floor" />
  {#each room.lights as l (l.id)}
    {@const m = mapLight(room.screen, l)}
    {#if l.enabled}
      <circle class="halo" cx={l.pos.x} cy={l.pos.y} r={110 + 60 * m.spread} fill="url(#thumb-{m.edge})" />
    {/if}
  {/each}
  <polygon points={walls} class="walls" />
  <line class="screen" x1={ends.left.x} y1={ends.left.y} x2={ends.right.x} y2={ends.right.y} />
  {#each room.lights as l (l.id)}
    {@const m = mapLight(room.screen, l)}
    <circle cx={l.pos.x} cy={l.pos.y} r="16" fill={EDGE_COLOR[m.edge]} opacity={l.enabled ? 1 : 0.3} />
  {/each}
</svg>

<style>
  .thumb {
    display: block;
    width: 100%;
    height: 100%;
  }
  .floor {
    fill: var(--plan-floor);
  }
  .walls {
    fill: none;
    stroke: var(--plan-wall);
    stroke-width: 8;
    stroke-linejoin: round;
  }
  .screen {
    stroke: var(--plan-wall);
    stroke-width: 18;
    stroke-linecap: round;
  }
  .halo {
    opacity: 0.25;
    transition: opacity 600ms var(--ease);
  }
  .live .halo {
    opacity: 1;
    animation: pulse 4s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.7;
    }
  }
</style>
