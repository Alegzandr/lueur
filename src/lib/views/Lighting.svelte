<script lang="ts">
  import { app, bindingFor, chooseMode, isOn, moodTargets, pickedLights, setAllPower } from "../state.svelte";
  import { t, tn } from "../i18n";
  import Icon from "../ui/Icon.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";
  import Switch from "../ui/Switch.svelte";
  import Lights from "./lighting/Lights.svelte";
  import Modes from "./lighting/Modes.svelte";
  import LightDot from "./lighting/LightDot.svelte";
  import Scenes from "./lighting/Scenes.svelte";
  import SleepButton from "./lighting/SleepButton.svelte";
  import Tune from "./lighting/Tune.svelte";
  import { summaryOf } from "./lighting/modes";

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

  /** One switch for the whole room: on while any light gives light. */
  const anyOn = $derived(app.config.bindings.some((b) => isOn(b.light_id)));

  // --- Where the settings go ---

  /** The lights that can be picked: those with a bulb. */
  const pickable = $derived(lights.filter((l) => bindingFor(l.id)));
  const wholeRoom = $derived(!app.target.some((id) => bindingFor(id)));
  /** What the lights picked are called, over the settings. */
  const targetName = $derived.by(() => {
    if (wholeRoom) return pickable.length === 1 ? pickable[0].name : t("whole_room");
    const picked = pickable.filter((l) => app.target.includes(l.id));
    return picked.length === 1 ? picked[0].name : tn("n_lights", picked.length);
  });
  /** The lights the settings go to, and what they do now. */
  const targetIds = $derived(moodTargets());
  const summary = $derived(summaryOf(targetIds));
  /** The one light the settings go to, if there is one. */
  const single = $derived.by(() => {
    const picked = pickedLights();
    if (picked) return picked.length === 1 ? picked[0] : null;
    return pickable.length === 1 ? pickable[0].id : null;
  });
</script>

<div class="page lighting">
  <header class="page-head">
    <h1 class="page-title">{t("nav_lighting")}</h1>
    {#if bound > 0}
      <div class="head-tools">
        <SleepButton />
        <!-- svelte-ignore a11y_label_has_associated_control: the switch is a button labelled by its own aria-label -->
        <label class="master" title={anyOn ? t("all_off_tip") : undefined}>
          <span aria-hidden="true">{t("all_lights")}</span>
          <Switch
            checked={anyOn}
            label={t("all_lights")}
            disabled={app.busy}
            onchange={(on) => setAllPower(on)}
          />
        </label>
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

  <!-- Lights and scenes on one side, what to do with the lights picked on
       the other, side by side when the window is wide enough. -->
  <div class="layout">
    <div class="col">
      {#if bound > 0}<Scenes />{/if}
      <Lights {pickable} {wholeRoom} />
    </div>
    {#if bound > 0}
      <!-- One heading says who the settings go to; the sections under it
           need not say it again. -->
      <section class="col side" aria-labelledby="selection-name">
        <header class="selection">
          <span class="dots" aria-hidden="true">
            {#each targetIds.slice(0, 5) as id (id)}<LightDot {id} size={22} edge />{/each}
          </span>
          <div class="selection-text">
            <h2 class="selection-name" id="selection-name">{targetName}</h2>
            {#if summary}
              <p class="selection-now" class:live={summary.live}>
                {#if summary.live}<span class="lamp" aria-hidden="true"></span>{/if}
                <span class="sr-only">{t("mode_now")}</span>{summary.text}
              </p>
            {/if}
          </div>
        </header>
        <Modes />
        <Tune {single} />
      </section>
    {/if}
  </div>
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

  .head-tools {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .master {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    font-weight: 500;
    color: var(--text-2);
    cursor: pointer;
  }

  /* --- Who the settings go to --- */
  .selection {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    min-height: 44px;
    margin-bottom: var(--s-5);
  }
  .dots {
    display: flex;
    flex: none;
  }
  .dots > :global(* + *) {
    margin-left: -7px;
  }
  .selection-text {
    min-width: 0;
  }
  /* On the lights' glow, like the section titles: a halo of the page's
     ground under the letters keeps them readable. */
  .selection-name {
    font-size: var(--t-lg);
    font-weight: 650;
    letter-spacing: -0.01em;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-shadow:
      0 0 2px var(--bg),
      0 0 6px var(--bg),
      0 0 14px var(--bg);
  }
  .selection-now {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: var(--t-sm);
    color: var(--text-2);
    text-shadow:
      0 0 2px var(--bg),
      0 0 6px var(--bg);
  }
  .selection-now.live {
    color: var(--glow-text);
    font-weight: 500;
  }
  .selection-now .lamp {
    --lamp: 6px;
  }

  /* Wider than the other pages: two columns once the window has the room. */
  .page.lighting {
    max-width: 1240px;
  }
  .col {
    min-width: 0;
  }
  .group + .layout,
  .layout > .col + .col {
    margin-top: var(--s-6);
  }
  .layout .col > :global(.group + .group) {
    margin-top: var(--s-6);
  }
  @media (min-width: 1200px) {
    .layout {
      display: grid;
      grid-template-columns: minmax(340px, 5fr) minmax(0, 7fr);
      align-items: start;
      gap: var(--s-7);
    }
    .layout > .col + .col {
      margin-top: 0;
    }
  }
</style>
