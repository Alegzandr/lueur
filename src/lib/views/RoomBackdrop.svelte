<script lang="ts">
  import { untrack } from "svelte";
  import { app } from "../state.svelte";
  import { doingOf } from "./lighting/modes";
  import { lookOfDoing, pictureOf, type Look } from "./lighting/pictures";

  // Behind every page but Lighting, the room as its lights make it now: the
  // photograph of what most of them do, dark when they are off, changing
  // with them. It rises from the top of the page and dissolves into it, so
  // the cards stay on plain ground.

  const look = $derived.by((): Look => {
    const looks = app.config.bindings.map((b) => lookOfDoing(doingOf(b.light_id)));
    if (looks.length === 0) return { key: "off", src: pictureOf("off"), tint: null };
    const count = new Map<string, number>();
    for (const l of looks) count.set(l.key, (count.get(l.key) ?? 0) + 1);
    return looks.reduce((best, l) => ((count.get(l.key) ?? 0) > (count.get(best.key) ?? 0) ? l : best));
  });

  const still = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  // A new light fades in over the one before, which goes once it is covered.
  let layers = $state<{ id: number; look: Look }[]>([]);
  let next = 0;
  $effect(() => {
    const l = look;
    untrack(() => {
      if (layers[layers.length - 1]?.look.key === l.key) return;
      const layer = { id: ++next, look: l };
      layers = still || layers.length === 0 ? [layer] : [layers[layers.length - 1], layer];
    });
  });

  function covered(id: number) {
    layers = layers.filter((l) => l.id >= id);
  }
</script>

<div class="room-backdrop" aria-hidden="true">
  {#each layers as { id, look: l }, i (id)}
    <div class="layer" class:entering={i > 0} onanimationend={() => covered(id)}>
      <img src={l.src} alt="" draggable="false" />
      {#if l.tint}<span class="tint" style="background:{l.tint}"></span>{/if}
    </div>
  {/each}
</div>

<style>
  .room-backdrop {
    position: absolute;
    inset: 0 0 auto;
    height: min(62vh, 560px);
    overflow: hidden;
    pointer-events: none;
    opacity: var(--room-photo-opacity);
    /* Gone by the time the cards gather: the page's own ground below. */
    mask-image: linear-gradient(to bottom, #000 0%, rgb(0 0 0 / 0.55) 45%, transparent 100%);
  }
  .layer {
    position: absolute;
    inset: 0;
  }
  .layer.entering {
    animation: dissolve 700ms var(--ease) both;
  }
  @keyframes dissolve {
    from {
      opacity: 0;
    }
  }
  /* Out of focus, as behind a shallow lens: the room's light without its
     details, which would compete with the cards. Scaled a little so the
     blur leaves no soft edge. */
  .layer img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 50% 62%;
    filter: blur(6px) var(--room-photo-filter);
    transform: scale(1.06);
  }
  /* A white or colour set by hand: the room's neutral light takes its hue. */
  .tint {
    position: absolute;
    inset: 0;
    mix-blend-mode: color;
    opacity: 0.9;
  }
  @media (prefers-reduced-motion: reduce) {
    .layer.entering {
      animation: none;
    }
  }
</style>
