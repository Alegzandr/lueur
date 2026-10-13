<script lang="ts">
  import {
    app,
    edit,
    bindingFor,
    isOn,
    plural,
    setAllPower,
    setLightEnabled,
    toggleEffect,
    toggleLightPower,
  } from "../state.svelte";
  import { describe, mapLight, EDGE_COLOR } from "../geometry";
  import { applyFeel, currentFeel, FEEL_OPTIONS } from "../presets";
  import { defaultConfig } from "../types";
  import Icon from "../ui/Icon.svelte";
  import Switch from "../ui/Switch.svelte";
  import Slider from "../ui/Slider.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import RoomThumb from "../ui/RoomThumb.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

  const DEF = defaultConfig();
  const pct = (v: number) => `${Math.round(v * 100)}%`;

  const hour = new Date().getHours();
  const greeting = hour < 5 ? "Good night" : hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const hasPlacedScreen = $derived(lights.length > 0);
  const setupDone = $derived(bound > 0 && lights.length > 0);
  const feel = $derived(currentFeel(app.config));

  const steps = $derived([
    {
      done: bound > 0,
      title: "Find your bulbs",
      text: "Lueur looks for Yeelight bulbs on your network.",
      action: "Find bulbs",
      go: () => (app.view = "bulbs"),
    },
    {
      done: bound > 0 && hasPlacedScreen,
      title: "Sketch your room",
      text: "Place the screen and each light where they really are.",
      action: "Open the plan",
      go: () => (app.view = "room"),
    },
    {
      done: app.status.running,
      title: "Turn it on",
      text: "The light around you follows the picture.",
      action: "Start",
      go: () => toggleEffect(),
    },
  ]);
  const nextStep = $derived(steps.findIndex((s) => !s.done));
</script>

<div class="page">
  <header class="page-head">
    <div>
      <h1 class="page-title">{greeting}</h1>
      <p class="page-lede">
        {#if app.status.running}
          Your room is following the screen.
        {:else if setupDone}
          Everything is in place. Start the ambilight whenever you are ready.
        {:else}
          Three short steps and the light around you will follow what is on screen.
        {/if}
      </p>
    </div>
  </header>

  <div class="banner-slot"><RestartBanner /></div>

  {#if app.ready && !setupDone}
    <section class="card steps" aria-label="Getting started">
      {#each steps as s, i}
        <div class="step" class:done={s.done} class:next={i === nextStep}>
          <span class="step-n" aria-hidden="true">
            {#if s.done}<Icon name="check" size={14} />{:else}{i + 1}{/if}
          </span>
          <div class="step-text">
            <h3>{s.title}</h3>
            <p>{s.text}</p>
          </div>
          {#if i === nextStep}
            <button class="btn primary" onclick={s.go} disabled={i === 2 && app.busy}>
              {s.action}
            </button>
          {/if}
        </div>
      {/each}
    </section>
  {:else}
    <section class="card hero" class:live={app.status.running}>
      <div class="hero-text">
        <span class="eyebrow">{app.status.running ? "On" : "Off"}</span>
        <h2>
          {#if app.status.running}
            {app.status.connected_lights === 0
              ? "No bulb is answering"
              : `${plural(app.status.connected_lights, "bulb")} following the picture`}
          {:else}
            {plural(bound, "bulb")} ready to follow the picture
          {/if}
        </h2>
        <div class="hero-actions">
          <button class="btn lg" class:primary={!app.status.running} disabled={app.busy} onclick={toggleEffect}>
            <Icon name="power" />
            {app.status.running ? "Stop ambilight" : "Start ambilight"}
          </button>
          <button class="btn plain" onclick={() => (app.view = "room")}>Edit room</button>
        </div>
      </div>
      <div class="hero-art">
        <RoomThumb live={app.status.running} />
      </div>
    </section>
  {/if}

  <section class="group">
    <div class="group-head">
      <h2 class="group-title">Feel</h2>
      <span class="group-note">Applies instantly, even while running</span>
    </div>
    <div class="card rows">
      <div class="feel-row">
        <div>
          <div class="label">Response</div>
          <p class="hint">
            {#if feel === null}
              Custom, tuned in Settings.
            {:else}
              {FEEL_OPTIONS.find((o) => o.value === feel)?.title}
            {/if}
          </p>
        </div>
        <Segmented
          label="Response"
          value={feel}
          options={FEEL_OPTIONS}
          onchange={(f) => edit((c) => applyFeel(c, f))}
        />
      </div>
      <div class="feel-row">
        <div class="label">Brightness</div>
        <div class="slider-cell">
          <Slider
            label="Brightness"
            value={app.config.engine.brightness}
            min={0.05}
            max={1}
            step={0.01}
            def={DEF.engine.brightness}
            format={pct}
            onchange={(v) => edit((c) => (c.engine.brightness = v))}
          />
        </div>
      </div>
      <div class="feel-row">
        <div class="label">Colour intensity</div>
        <div class="slider-cell">
          <Slider
            label="Colour intensity"
            value={app.config.engine.saturation}
            min={0}
            max={1}
            step={0.01}
            def={DEF.engine.saturation}
            format={pct}
            onchange={(v) => edit((c) => (c.engine.saturation = v))}
          />
        </div>
      </div>
    </div>
  </section>

  {#if lights.length > 0}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title">Lights</h2>
        {#if bound > 0}
          <div class="bulk">
            <button class="btn plain sm" disabled={app.busy} onclick={() => setAllPower(true)}>All on</button>
            <button
              class="btn plain sm"
              disabled={app.busy}
              title="Turns every bulb off and stops the ambilight"
              onclick={() => setAllPower(false)}
            >
              All off
            </button>
          </div>
        {/if}
      </div>
      <div class="card rows">
        {#each lights as l (l.id)}
          {@const m = mapLight(app.config.room.screen, l)}
          {@const b = bindingFor(l.id)}
          <div class="light" class:excluded={!l.enabled}>
            <span class="swatch" style="--c:{EDGE_COLOR[m.edge]}" aria-hidden="true"></span>
            <button
              class="light-text"
              title="Show on the plan"
              onclick={() => {
                app.selectedLight = l.id;
                app.view = "room";
              }}
            >
              <span class="light-name">{l.name}</span>
              <span class="light-sub">
                {#if b}{describe(m)}{:else}<span class="warn">No bulb assigned</span>{/if}
              </span>
            </button>
            {#if b}
              <button
                class="btn plain icon sm power"
                class:on={isOn(l.id)}
                disabled={app.powerBusy[l.id]}
                title={isOn(l.id) ? "Turn bulb off" : "Turn bulb on"}
                aria-label={isOn(l.id) ? `Turn ${l.name} off` : `Turn ${l.name} on`}
                onclick={() => toggleLightPower(l.id)}
              >
                <Icon name="power" size={15} />
              </button>
            {:else}
              <button class="btn sm" onclick={() => (app.view = "bulbs")}>Assign</button>
            {/if}
            <Switch
              checked={l.enabled}
              label="{l.name} follows the picture"
              onchange={(v) => setLightEnabled(l.id, v)}
            />
          </div>
        {/each}
      </div>
      <p class="foot">The switch decides whether a light follows the picture. Excluded lights keep their current colour.</p>
    </section>
  {/if}
</div>

<style>
  .banner-slot:not(:empty) {
    margin-bottom: var(--s-4);
  }

  /* Getting started */
  .steps {
    padding: var(--s-2) 0;
    margin-bottom: var(--s-6);
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--s-4);
    padding: var(--s-3) var(--s-5);
    color: var(--text-3);
  }
  .step.next,
  .step.done {
    color: var(--text);
  }
  .step-n {
    width: 26px;
    height: 26px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
    font-weight: 600;
    font-size: var(--t-sm);
  }
  .step.next .step-n {
    box-shadow: inset 0 0 0 1.5px var(--text);
  }
  .step.done .step-n {
    background: var(--glow);
    color: #fff;
    box-shadow: none;
  }
  .step-text {
    flex: 1;
  }
  .step-text h3 {
    font-size: var(--t-lg);
    font-weight: 600;
  }
  .step.done h3 {
    color: var(--text-2);
  }
  .step-text p {
    font-size: var(--t-md);
    color: var(--text-3);
  }

  /* Hero */
  .hero {
    display: grid;
    grid-template-columns: 1.15fr 1fr;
    gap: var(--s-6);
    padding: var(--s-6);
    margin-bottom: var(--s-6);
    overflow: hidden;
  }
  .eyebrow {
    display: inline-block;
    padding: 2px 8px;
    border-radius: var(--r-full);
    background: var(--surface-2);
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .live .eyebrow {
    background: var(--glow-soft);
    color: var(--glow-text);
  }
  .hero h2 {
    margin: var(--s-3) 0 var(--s-5);
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 500;
    line-height: 1.25;
    text-wrap: balance;
  }
  .hero-text {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: center;
  }
  .hero-actions {
    display: flex;
    gap: var(--s-2);
  }
  .hero-art {
    height: 180px;
    border-radius: var(--r-md);
    background: var(--plan-bg);
    padding: var(--s-3);
  }

  /* Feel */
  .feel-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-5);
    padding: var(--s-3) var(--s-4);
    min-height: 56px;
  }
  .label {
    font-weight: 500;
  }
  .hint {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .slider-cell {
    width: min(360px, 55%);
  }

  /* Lights */
  .bulk {
    display: flex;
    gap: 2px;
  }
  .light {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-4);
    min-height: 54px;
  }
  .swatch {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex: none;
    background: var(--c);
    box-shadow: 0 0 0 4px color-mix(in oklab, var(--c) 20%, transparent);
  }
  .excluded .swatch {
    background: var(--line-strong);
    box-shadow: none;
  }
  .light-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    text-align: left;
    border-radius: var(--r-sm);
  }
  .light-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .light-text:hover .light-name {
    text-decoration: underline;
    text-decoration-color: var(--line-strong);
    text-underline-offset: 3px;
  }
  .excluded .light-name {
    color: var(--text-3);
  }
  .light-sub {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .warn {
    color: var(--glow-text);
  }
  .power {
    color: var(--text-3);
  }
  .power.on {
    color: var(--glow-text);
  }
  .foot {
    margin-top: var(--s-2);
    padding: 0 var(--s-1);
    font-size: var(--t-sm);
    color: var(--text-3);
  }
</style>
