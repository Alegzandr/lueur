<script lang="ts">
  import { app, bindingFor, chooseMode, setAllPower, toggleTarget } from "../state.svelte";
  import { t, tn } from "../i18n";
  import Icon from "../ui/Icon.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";
  import Effects from "./lighting/Effects.svelte";
  import Lights from "./lighting/Lights.svelte";
  import Moods from "./lighting/Moods.svelte";
  import Sleep from "./lighting/Sleep.svelte";

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const arranged = $derived(app.config.interface.room_arranged);
  const setupDone = $derived(bound > 0 && arranged);
  const live = $derived(app.status.running);

  // The last step starts the Ambilight the way its tile does.
  const steps = $derived([
    { done: bound > 0, title: t("step1_title"), btn: t("step1_btn"), go: () => (app.view = "bulbs") },
    { done: arranged, title: t("step2_title"), btn: t("step2_btn"), go: () => (app.view = "room") },
    { done: live, title: t("step3_title"), btn: t("start_ambilight"), go: () => chooseMode("ambilight") },
  ]);
  const next = $derived(steps.findIndex((s) => !s.done));

  // --- Where effects and moods go ---

  /** The lights that can be picked: those with a bulb. */
  const pickable = $derived(lights.filter((l) => bindingFor(l.id)));
  const wholeRoom = $derived(!app.target.some((id) => bindingFor(id)));
  /** What the lights picked are called, shown by the effects and moods. */
  const targetName = $derived.by(() => {
    if (wholeRoom) return null;
    const picked = pickable.filter((l) => app.target.includes(l.id));
    return picked.length === 1 ? picked[0].name : tn("n_lights", picked.length);
  });
</script>

<div class="page">
  <header class="page-head">
    <h1 class="page-title">{t("nav_lighting")}</h1>
    {#if bound > 0}
      <div class="btn-group">
        <button class="btn" disabled={app.busy} onclick={() => setAllPower(true)}>{t("all_on")}</button>
        <button class="btn" disabled={app.busy} title={t("all_off_tip")} onclick={() => setAllPower(false)}>
          {t("all_off")}
        </button>
      </div>
    {/if}
  </header>

  <div class="slot"><RestartBanner /></div>

  {#if app.ready && !setupDone}
    <section class="group">
      <div class="group-head"><h2 class="group-title">{t("setup_title")}</h2></div>
      <ol class="card rows steps">
        {#each steps as s, i}
          <li class="step" class:done={s.done} class:next={i === next}>
            <span class="mark" aria-hidden="true">
              {#if s.done}<Icon name="check" size={12} />{:else}{i + 1}{/if}
            </span>
            <span class="step-title">{s.title}</span>
            {#if i === next}
              <button class="btn primary" disabled={i === 2 && (app.busy || bound === 0)} onclick={s.go}>
                {s.btn}
              </button>
            {/if}
          </li>
        {/each}
      </ol>
    </section>
  {/if}

  {#if pickable.length > 1}
    <section class="group">
      <div class="group-head"><h2 class="group-title" title={t("apply_to_tip")}>{t("apply_to")}</h2></div>
      <!-- Who the effects and moods go to, not what plays: a neutral edge and
           a tick, never the amber of something running. -->
      <div class="targets" role="group" aria-label={t("apply_to")}>
        <button class="chip target" class:picked={wholeRoom} aria-pressed={wholeRoom} onclick={() => (app.target = [])}>
          <span class="tick" aria-hidden="true">{#if wholeRoom}<Icon name="check" size={10} />{/if}</span>
          {t("whole_room")}
        </button>
        {#each pickable as l (l.id)}
          {@const picked = !wholeRoom && app.target.includes(l.id)}
          <button class="chip target" class:picked aria-pressed={picked} onclick={() => toggleTarget(l.id)}>
            <span class="tick" aria-hidden="true">{#if picked}<Icon name="check" size={10} />{/if}</span>
            {l.name}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if bound > 0}
    <Effects {targetName} />
    <Moods {targetName} />
    <Sleep />
  {/if}

  <Lights {pickable} {wholeRoom} />
</div>

<style>
  .slot:not(:empty) {
    margin-bottom: var(--s-5);
  }

  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: 0 var(--s-2) 0 var(--s-4);
    min-height: 48px;
    color: var(--text-3);
  }
  .step.next {
    color: var(--text);
  }
  .mark {
    width: 20px;
    height: 20px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px var(--line-strong);
    font-size: var(--t-xs);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .next .mark {
    box-shadow: inset 0 0 0 1.5px var(--text);
  }
  .done .mark {
    background: var(--ok);
    box-shadow: none;
    color: #fff;
  }
  .step-title {
    flex: 1;
    font-weight: 500;
  }
  .done .step-title {
    text-decoration: line-through;
    text-decoration-color: var(--line-strong);
  }

  /* The section titles sit on the lights' glow, not on a card: a step darker
     than elsewhere, and a halo of the page's own ground under the letters,
     so they keep 4.5:1 however bright the room is. Every section of the
     page has them, its own pieces' included. */
  .page :global(.group-title) {
    color: var(--text-2);
    text-shadow:
      0 0 2px var(--bg),
      0 0 6px var(--bg),
      0 0 14px var(--bg);
  }
  /* Who it goes to, in the neutral ink of a selection. */
  .page :global(.for) {
    color: var(--text);
    font-weight: 500;
  }

  /* --- Where effects and moods go --- */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 var(--s-3) 0 var(--s-2);
    border-radius: 999px;
    background: var(--surface);
    box-shadow: var(--card-edge);
    font-size: var(--t-sm);
    font-weight: 500;
    white-space: nowrap;
    transition: box-shadow 200ms var(--ease);
  }
  .chip:hover {
    background: var(--surface-hover);
  }
  /* Picked: a neutral edge and a tick, like a checkbox. */
  .chip.picked {
    background: var(--select-soft);
    box-shadow: inset 0 0 0 1.5px var(--select);
  }
  .tick {
    width: 14px;
    height: 14px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px var(--control-line);
  }
  .picked .tick {
    background: var(--select);
    box-shadow: none;
    color: var(--on-ink);
  }
  .targets {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1);
  }
</style>
