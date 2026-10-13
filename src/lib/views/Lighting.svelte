<script lang="ts">
  import {
    app,
    edit,
    bindingFor,
    isOn,
    setAllPower,
    setLightEnabled,
    toggleEffect,
    toggleLightPower,
  } from "../state.svelte";
  import { t, pct } from "../i18n";
  import { describe, mapLight, EDGE_COLOR } from "../geometry";
  import { applyFeel, currentFeel, FEEL_ORDER, type Feel } from "../presets";
  import { defaultConfig } from "../types";
  import Icon from "../ui/Icon.svelte";
  import Switch from "../ui/Switch.svelte";
  import Slider from "../ui/Slider.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

  const DEF = defaultConfig();

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const arranged = $derived(app.config.interface.room_arranged);
  const setupDone = $derived(bound > 0 && arranged);
  const feel = $derived(currentFeel(app.config));

  const feelOptions = $derived(
    FEEL_ORDER.map((f) => ({ value: f, label: t(`feel_${f}`), title: t(`feel_${f}_hint`) })),
  );

  const steps = $derived([
    { done: bound > 0, title: t("step1_title"), text: t("step1_text"), btn: t("step1_btn"), go: () => (app.view = "bulbs") },
    { done: arranged, title: t("step2_title"), text: t("step2_text"), btn: t("step2_btn"), go: () => (app.view = "room") },
    { done: app.status.running, title: t("step3_title"), text: t("step3_text"), btn: t("start"), go: toggleEffect },
  ]);
  const next = $derived(steps.findIndex((s) => !s.done));

  function showOnPlan(id: string) {
    app.selectedLight = id;
    app.view = "room";
  }
</script>

<div class="page">
  <header class="page-head">
    <h1 class="page-title">{t("nav_lighting")}</h1>
    {#if bound > 0}
      <div class="bulk">
        <button class="btn" disabled={app.busy} onclick={() => setAllPower(true)}>{t("all_on")}</button>
        <button class="btn" disabled={app.busy} title={t("all_off_hint")} onclick={() => setAllPower(false)}>
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
              {#if s.done}<Icon name="check" size={13} />{/if}
            </span>
            <div class="step-text">
              <div class="step-title">{s.title}</div>
              <p class="step-sub">{s.text}</p>
            </div>
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

  {#if lights.length > 0}
    <section class="group">
      <div class="card">
        <div class="thead">
          <span>{t("lights")}</span>
          <span>{t("follows_short")}</span>
        </div>
        <div class="rows">
          {#each lights as l (l.id)}
            {@const m = mapLight(app.config.room.screen, l)}
            {@const b = bindingFor(l.id)}
            <div class="light" class:excluded={!l.enabled}>
              <span class="swatch" style="--c:{EDGE_COLOR[m.edge]}" aria-hidden="true"></span>
              <button class="light-text" title={t("show_on_plan")} onclick={() => showOnPlan(l.id)}>
                <span class="light-name">{l.name}</span>
                <span class="light-sub">
                  {#if b}{t(describe(m))}{:else}<span class="warn">{t("no_bulb")}</span>{/if}
                </span>
              </button>
              {#if b}
                <button
                  class="btn plain icon sm power"
                  class:on={isOn(l.id)}
                  disabled={app.powerBusy[l.id]}
                  title={isOn(l.id) ? t("turn_off") : t("turn_on")}
                  aria-label="{l.name}: {isOn(l.id) ? t('turn_off') : t('turn_on')}"
                  onclick={() => toggleLightPower(l.id)}
                >
                  <Icon name="power" size={15} />
                </button>
              {:else}
                <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
              {/if}
              <Switch
                checked={l.enabled}
                label="{l.name}: {t('follows')}"
                onchange={(v) => setLightEnabled(l.id, v)}
              />
            </div>
          {/each}
        </div>
      </div>
    </section>
  {/if}

  {#if setupDone}
    <section class="group">
      <div class="card rows">
        <div class="row">
          <div>
            <div class="label">{t("response")}</div>
            <p class="hint">{feel ? t(`feel_${feel}_hint`) : t("feel_custom")}</p>
          </div>
          <Segmented
            label={t("response")}
            value={feel}
            options={feelOptions}
            onchange={(f: Feel) => edit((c) => applyFeel(c, f))}
          />
        </div>
        <div class="row">
          <div class="label">{t("brightness")}</div>
          <div class="slider-cell">
            <Slider
              label={t("brightness")}
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
        <div class="row">
          <div class="label">{t("intensity")}</div>
          <div class="slider-cell">
            <Slider
              label={t("intensity")}
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
  {/if}
</div>

<style>
  .bulk {
    display: flex;
    gap: var(--s-2);
  }
  .slot:not(:empty) {
    margin-bottom: var(--s-5);
  }

  /* First-run checklist */
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .step {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-3) var(--s-4);
    min-height: 56px;
  }
  .mark {
    width: 18px;
    height: 18px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1.5px solid var(--line-strong);
  }
  .done .mark {
    border-color: var(--ok);
    background: var(--ok);
    color: var(--surface);
  }
  .step-text {
    flex: 1;
  }
  .step-title {
    font-weight: 500;
  }
  .step:not(.next):not(.done) .step-title,
  .done .step-title {
    color: var(--text-2);
  }
  .step-sub {
    font-size: var(--t-sm);
    color: var(--text-3);
  }

  /* Lights table */
  .thead {
    display: flex;
    justify-content: space-between;
    padding: var(--s-2) var(--s-4);
    border-bottom: 1px solid var(--line);
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text-2);
  }
  .light {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-4);
    min-height: 50px;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: none;
    background: var(--c);
  }
  .excluded .swatch {
    background: var(--line-strong);
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
    margin-right: var(--s-2);
  }
  .power.on {
    color: var(--glow-text);
  }

  /* Behaviour */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-5);
    padding: var(--s-3) var(--s-4);
    min-height: 52px;
  }
  .label {
    font-weight: 500;
  }
  .hint {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .slider-cell {
    width: min(340px, 55%);
  }
</style>
