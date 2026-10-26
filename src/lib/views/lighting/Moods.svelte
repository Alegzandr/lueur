<script lang="ts">
  import { app, applyScene, moodTargets } from "../../state.svelte";
  import { t } from "../../i18n";
  import { MOODS, NATURAL, kelvinRgb, rgbCss } from "../../scenes";
  import Status from "./Status.svelte";
  import { moodOf, naturalSwatch } from "./modes";

  // The built-in moods, the natural light first.

  let {
    targetName,
  }: {
    /** What the lights picked are called, or null for the whole room. */
    targetName: string | null;
  } = $props();

  const moodIds = $derived(moodTargets());
  /** How many of the lights a mood would go to show `mood` already. */
  const moodShare = (mood: string) => moodIds.filter((id) => moodOf(id) === mood).length;
  const moodAll = (mood: string) => moodIds.length > 0 && moodShare(mood) === moodIds.length;
</script>

<section class="group">
  <div class="group-head">
    <h2 class="group-title">
      {t("moods")}{#if targetName}<span class="for">{` · ${targetName}`}</span>{/if}
    </h2>
  </div>
  <div class="moods">
    <button
      class="mood"
      class:active={moodAll(NATURAL)}
      class:partial={moodShare(NATURAL) > 0 && !moodAll(NATURAL)}
      class:pending={app.pending === NATURAL}
      aria-pressed={moodAll(NATURAL) ? true : moodShare(NATURAL) > 0 ? "mixed" : false}
      title={t("mood_natural_tip")}
      disabled={app.pending !== null}
      onclick={() => applyScene(NATURAL)}
    >
      <span class="mood-dot" style="--c:{rgbCss(kelvinRgb(4000))};--a:1;background:{naturalSwatch}" aria-hidden="true"
      ></span>
      <span class="mood-text">
        {t("mood_natural")}
        <Status all={moodAll(NATURAL)} share={moodShare(NATURAL)} total={moodIds.length} />
      </span>
    </button>
    {#each MOODS as m (m.id)}
      <button
        class="mood"
        class:active={moodAll(m.id)}
        class:partial={moodShare(m.id) > 0 && !moodAll(m.id)}
        class:pending={app.pending === m.id}
        aria-pressed={moodAll(m.id) ? true : moodShare(m.id) > 0 ? "mixed" : false}
        disabled={app.pending !== null}
        onclick={() => applyScene(m.id)}
      >
        <span
          class="mood-dot"
          style="--c:{rgbCss(kelvinRgb(m.kelvin))};--a:{0.4 + (0.6 * m.bright) / 100}"
          aria-hidden="true"
        ></span>
        <span class="mood-text">
          {t(`mood_${m.id}`)}
          <Status all={moodAll(m.id)} share={moodShare(m.id)} total={moodIds.length} />
        </span>
      </button>
    {/each}
  </div>
</section>

<style>
  .moods {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--s-2);
  }
  /* Names wrap rather than truncate: French ones run long. */
  .mood {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    min-height: 38px;
    padding: 6px var(--s-3);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
    font-weight: 500;
    line-height: 1.2;
    text-align: left;
    text-wrap: balance;
    transition: box-shadow 200ms var(--ease);
  }
  .mood:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .mood:disabled:not(.pending) {
    opacity: 0.6;
  }
  .mood-text {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  /* Playing: an amber ring. Some of the lights picked show it: a dashed ring. */
  .mood.partial {
    outline: 1.5px dashed var(--active);
    outline-offset: -1.5px;
  }
  .mood.active {
    box-shadow: inset 0 0 0 2px var(--active);
  }
  .mood.pending .mood-dot {
    animation: breathe 800ms var(--ease) infinite alternate;
  }
  @keyframes breathe {
    to {
      opacity: 0.45;
    }
  }
  /* A dimmer mood is a fainter dot; the fade is on the fill alone, so the
     edge that keeps Daylight visible on a white tile stays whole. */
  .mood-dot {
    width: 14px;
    height: 14px;
    flex: none;
    border-radius: 50%;
    background: color-mix(in oklab, var(--c) calc(var(--a) * 100%), transparent);
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.14);
  }

  /* Reduced motion: a pending mood dims instead of breathing. */
  @media (prefers-reduced-motion: reduce) {
    .mood.pending .mood-dot {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
