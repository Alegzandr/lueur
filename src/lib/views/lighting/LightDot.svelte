<script lang="ts">
  import { bindingFor, isDriven, isOn, lightState, modeOf } from "../../state.svelte";
  import { tintCss } from "../../scenes";
  import { modeSwatch } from "./modes";

  // A light as it glows now, drawn round: its white or colour, fainter as it
  // dims, the gradient of the effect it plays, or a hollow ring while off.
  // What a bulb gives is content, so its real colour belongs here.

  let { id, size = 26, edge = false }: { id: string; size?: number; edge?: boolean } = $props();

  const bound = $derived(!!bindingFor(id));
  const on = $derived(bound && isOn(id));
  const driven = $derived(isDriven(id));
  const s = $derived(lightState(id));
  const style = $derived.by(() => {
    if (!on) return "";
    if (driven) return `--fill:${modeSwatch(modeOf(id), s)};--c:var(--active)`;
    const c = tintCss(s.tint);
    return `--fill:color-mix(in oklab, ${c} ${40 + 0.6 * s.bright}%, transparent);--c:${c}`;
  });
</script>

<span
  class="light-dot"
  class:on
  class:off={bound && !on}
  class:none={!bound}
  class:edge
  style="--size:{size}px;{style}"
  aria-hidden="true"
></span>

<style>
  .light-dot {
    position: relative;
    width: var(--size);
    height: var(--size);
    flex: none;
    border-radius: 50%;
    transition:
      background 300ms var(--ease),
      box-shadow 400ms var(--ease);
  }
  /* A hairline keeps a near-white bulb visible on a white card; the halo is
     the bulb's own colour, as large as the dot is. */
  .light-dot.on {
    background: var(--fill);
    box-shadow:
      inset 0 0 0 1px oklch(0 0 0 / 0.12),
      0 0 calc(var(--size) * 0.45) color-mix(in oklab, var(--c) 45%, transparent);
  }
  .light-dot.off {
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
  }
  /* No bulb: a dashed outline, a place for one. */
  .light-dot.none {
    border: 1.5px dashed var(--line-strong);
  }
  /* Stacked over a neighbour, as in a group of lights: a ring of the ground
     it sits on keeps each one apart. */
  .light-dot.edge {
    outline: 2px solid var(--bg);
  }
</style>
