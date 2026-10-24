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
  import Row from "../ui/Row.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

  const DEF = defaultConfig();

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const arranged = $derived(app.config.interface.room_arranged);
  const setupDone = $derived(bound > 0 && arranged);
  const feel = $derived(currentFeel(app.config));
  const live = $derived(app.status.running);

  const feelOptions = $derived(
    FEEL_ORDER.map((f) => ({ value: f, label: t(`feel_${f}`), title: t(`feel_${f}_tip`) })),
  );

  const steps = $derived([
    { done: bound > 0, title: t("step1_title"), btn: t("step1_btn"), go: () => (app.view = "bulbs") },
    { done: arranged, title: t("step2_title"), btn: t("step2_btn"), go: () => (app.view = "room") },
    { done: live, title: t("step3_title"), btn: t("start"), go: toggleEffect },
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

  {#if lights.length > 0}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title">{t("lights")}</h2>
        <span class="group-title">{t("follows")}</span>
      </div>
      <div class="card rows">
        {#each lights as l (l.id)}
          {@const m = mapLight(app.config.room.screen, l)}
          {@const b = bindingFor(l.id)}
          <div class="light" class:excluded={!l.enabled}>
            <span
              class="swatch"
              class:lit={live && l.enabled && b && isOn(l.id)}
              style="--c:{EDGE_COLOR[m.edge]}"
              aria-hidden="true"
            ></span>
            <button class="light-text" title={t("show_on_plan")} onclick={() => showOnPlan(l.id)}>
              <span class="light-name">{l.name}</span>
              <span class="light-sub">{b ? t(describe(m)) : t("no_bulb")}</span>
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
                <Icon name="power" size={14} />
              </button>
            {:else}
              <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
            {/if}
            <Switch checked={l.enabled} label="{l.name}: {t('follows')}" onchange={(v) => setLightEnabled(l.id, v)} />
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if setupDone}
    <section class="group">
      <div class="card rows">
        <Row label={t("response")}>
          <Segmented
            label={t("response")}
            value={feel}
            options={feelOptions}
            onchange={(f: Feel) => edit((c) => applyFeel(c, f))}
          />
        </Row>
        <Row label={t("brightness")}>
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
        </Row>
        <Row label={t("intensity")} tip={t("intensity_tip")}>
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
        </Row>
      </div>
    </section>
  {/if}
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

  .light {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: 0 var(--s-4);
    min-height: 48px;
  }
  .swatch {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
    background: var(--c);
    transition: box-shadow 400ms var(--ease);
  }
  .swatch.lit {
    box-shadow:
      0 0 0 3px color-mix(in oklab, var(--c) 22%, transparent),
      0 0 10px 2px color-mix(in oklab, var(--c) 55%, transparent);
  }
  .excluded .swatch {
    background: var(--line-strong);
  }
  .light-text {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    text-align: left;
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
    white-space: nowrap;
  }
  .power {
    color: var(--text-3);
    margin-right: var(--s-1);
  }
  .power.on {
    color: var(--glow-text);
  }
</style>
