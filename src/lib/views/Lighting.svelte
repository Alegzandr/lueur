<script lang="ts">
  import {
    app,
    edit,
    adjustLight,
    applyScene,
    bindingFor,
    identifyLight,
    isDriven,
    isOn,
    lightState,
    removeScene,
    saveScene,
    setAllPower,
    setLightEnabled,
    toggleEffect,
    toggleLightPower,
  } from "../state.svelte";
  import { t, pct } from "../i18n";
  import { describe, mapLight, EDGE_COLOR } from "../geometry";
  import { applyFeel, currentFeel, FEEL_ORDER, type Feel } from "../presets";
  import {
    MAX_KELVIN,
    MIN_KELVIN,
    MOODS,
    PALETTE,
    hexOf,
    kelvinRgb,
    rgbCss,
    rgbOfHex,
    sameRgb,
    tintCss,
  } from "../scenes";
  import { defaultConfig, type LightState } from "../types";
  import Icon from "../ui/Icon.svelte";
  import Switch from "../ui/Switch.svelte";
  import Slider from "../ui/Slider.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Row from "../ui/Row.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

  const DEF = defaultConfig();
  // Fullscreen can only be detected on Windows (see src-tauri/src/auto.rs).
  const AUTO_SUPPORTED = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const arranged = $derived(app.config.interface.room_arranged);
  const setupDone = $derived(bound > 0 && arranged);
  const feel = $derived(currentFeel(app.config));
  const live = $derived(app.status.running);

  const feelOptions = $derived(
    FEEL_ORDER.map((f) => ({ value: f, label: t(`feel_${f}`), title: t(`feel_${f}_tip`) })),
  );
  const tintOptions = $derived([
    { value: "white" as const, label: t("white") },
    { value: "color" as const, label: t("tint_colour") },
  ]);

  const steps = $derived([
    { done: bound > 0, title: t("step1_title"), btn: t("step1_btn"), go: () => (app.view = "bulbs") },
    { done: arranged, title: t("step2_title"), btn: t("step2_btn"), go: () => (app.view = "room") },
    { done: live, title: t("step3_title"), btn: t("start"), go: toggleEffect },
  ]);
  const next = $derived(steps.findIndex((s) => !s.done));

  /** The light whose white and colour controls are showing. */
  let open = $state<string | null>(null);

  // What each light was last given in the other mode, so switching between
  // White and Colour comes back to it rather than to a default.
  const lastKelvin: Record<string, number> = {};
  const lastColour: Record<string, [number, number, number]> = {};

  function setMode(id: string, mode: "white" | "color") {
    const tint = lightState(id).tint;
    if (tint.mode === mode) return;
    if (tint.mode === "white") lastKelvin[id] = tint.kelvin;
    else lastColour[id] = [tint.r, tint.g, tint.b];
    if (mode === "white") {
      adjustLight(id, { tint: { mode: "white", kelvin: lastKelvin[id] ?? 2700 } });
    } else {
      const [r, g, b] = lastColour[id] ?? PALETTE[1];
      adjustLight(id, { tint: { mode: "color", r, g, b } });
    }
  }

  function setColour(id: string, [r, g, b]: [number, number, number]) {
    adjustLight(id, { tint: { mode: "color", r, g, b } });
  }

  function showOnPlan(id: string) {
    app.selectedLight = id;
    app.view = "room";
  }

  // --- Scenes ---

  const same = (a: LightState | undefined, b: LightState) =>
    !!a && a.on === b.on && a.bright === b.bright && JSON.stringify(a.tint) === JSON.stringify(b.tint);

  /** Whether every light with a bulb shows what `wish` says. */
  function showing(wish: (lightId: string) => LightState | undefined): boolean {
    if (live || bound === 0) return false;
    return app.config.bindings.every((b) => {
      const w = wish(b.light_id);
      return !w || same(app.config.states[b.light_id], w);
    });
  }

  let naming = $state(false);
  let sceneName = $state("");

  function startNaming() {
    sceneName = t("scene_n", { n: app.config.scenes.length + 1 });
    naming = true;
  }

  function commitScene(e: SubmitEvent) {
    e.preventDefault();
    saveScene(sceneName);
    naming = false;
  }

  function focusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
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

  {#if bound > 0}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title">
          {t("scenes")}
          {#if live}
            <span class="tip" title={t("scenes_tip")} aria-label={t("scenes_tip")} role="img">
              <Icon name="info" size={12} />
            </span>
          {/if}
        </h2>
      </div>
      <div class="scenes">
        {#each MOODS as m (m.id)}
          {@const active = showing(() => ({ on: true, bright: m.bright, tint: { mode: "white", kelvin: m.kelvin } }))}
          <button
            class="chip"
            class:active
            aria-pressed={active}
            disabled={app.sceneBusy !== null}
            class:pending={app.sceneBusy === m.id}
            onclick={() => applyScene(m.id)}
          >
            <span
              class="chip-dot"
              style="--c:{rgbCss(kelvinRgb(m.kelvin))};--a:{0.35 + (0.65 * m.bright) / 100}"
              aria-hidden="true"
            ></span>
            {t(`mood_${m.id}`)}
          </button>
        {/each}
        {#each app.config.scenes as s (s.id)}
          {@const active = showing((id) => s.lights[id])}
          <span class="chip-wrap">
            <button
              class="chip user"
              class:active
              aria-pressed={active}
              disabled={app.sceneBusy !== null}
              class:pending={app.sceneBusy === s.id}
              onclick={() => applyScene(s.id)}
            >
              <span class="chip-dots" aria-hidden="true">
                {#each Object.values(s.lights).filter((l) => l.on).slice(0, 3) as l}
                  <span class="chip-dot" style="--c:{tintCss(l.tint)};--a:1"></span>
                {/each}
              </span>
              {s.name}
            </button>
            <button
              class="chip-x"
              title={t("remove_scene", { name: s.name })}
              aria-label={t("remove_scene", { name: s.name })}
              onclick={() => removeScene(s.id)}
            >
              <Icon name="close" size={11} />
            </button>
          </span>
        {/each}
        {#if naming}
          <form class="name-form" onsubmit={commitScene}>
            <input
              class="input sm"
              aria-label={t("scene_name")}
              maxlength="40"
              bind:value={sceneName}
              use:focusSelect
              onkeydown={(e) => e.key === "Escape" && (naming = false)}
              onblur={() => (naming = false)}
            />
            <button class="btn sm primary" type="submit" onmousedown={(e) => e.preventDefault()}>{t("save")}</button>
          </form>
        {:else}
          <button class="chip add" title={t("save_scene_tip")} onclick={startNaming}>
            <Icon name="plus" size={12} />
            {t("save_scene")}
          </button>
        {/if}
      </div>
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
          {@const s = lightState(l.id)}
          {@const driven = isDriven(l.id)}
          {@const expanded = open === l.id}
          <div class="light" class:excluded={!l.enabled && !b} class:expanded>
            <div class="light-row">
              <span
                class="swatch"
                class:lit={b && isOn(l.id)}
                class:off={b && !isOn(l.id)}
                style="--c:{driven ? EDGE_COLOR[m.edge] : b ? tintCss(s.tint) : 'var(--line-strong)'}"
                aria-hidden="true"
              ></span>
              <button
                class="light-text"
                aria-expanded={b ? expanded : undefined}
                title={b ? t("light_more", { name: l.name }) : t("show_on_plan")}
                onclick={() => (b ? (open = expanded ? null : l.id) : showOnPlan(l.id))}
              >
                <span class="light-name">{l.name}</span>
                <span class="light-sub">
                  {#if !b}{t("no_bulb")}{:else if l.enabled}{t(describe(m))}{/if}
                </span>
                {#if b}
                  <span class="chev" aria-hidden="true"><Icon name="chevron" size={12} /></span>
                {/if}
              </button>

              {#if !b}
                <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
              {:else if driven}
                <span class="live-tag">
                  <span class="live-dot" aria-hidden="true"></span>
                  {t("follows")}
                </span>
              {:else}
                <div class="bright" class:dim={!s.on}>
                  <input
                    type="range"
                    min="1"
                    max="100"
                    step="1"
                    value={s.bright}
                    style="--fill:{(s.bright - 1) / 99}"
                    aria-label="{l.name}: {t('brightness')}"
                    aria-valuetext={pct(s.bright / 100)}
                    oninput={(e) => adjustLight(l.id, { bright: +e.currentTarget.value })}
                  />
                  <output class="num">{pct(s.bright / 100)}</output>
                </div>
              {/if}

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
              {/if}
              <Switch checked={l.enabled} label="{l.name}: {t('follows')}" onchange={(v) => setLightEnabled(l.id, v)} />
            </div>

            {#if expanded && b}
              <div class="panel">
                {#if driven}
                  <p class="hint">{t("driven_hint")}</p>
                {:else}
                  <div class="mode">
                    <Segmented
                      label="{l.name}: {t('white')} / {t('tint_colour')}"
                      value={s.tint.mode}
                      options={tintOptions}
                      onchange={(v) => setMode(l.id, v)}
                    />
                  </div>
                  {#if s.tint.mode === "white"}
                    <div class="warmth">
                      <input
                        class="kelvin"
                        type="range"
                        min={MIN_KELVIN}
                        max={MAX_KELVIN}
                        step="100"
                        value={s.tint.kelvin}
                        style="--warm:{rgbCss(kelvinRgb(MIN_KELVIN))};--mid:{rgbCss(kelvinRgb(4000))};--cool:{rgbCss(kelvinRgb(MAX_KELVIN))}"
                        aria-label="{l.name}: {t('warmth')}"
                        aria-valuetext="{s.tint.kelvin} K"
                        oninput={(e) => adjustLight(l.id, { tint: { mode: "white", kelvin: +e.currentTarget.value } })}
                      />
                      <div class="ends">
                        <span>{t("warmth_warm")}</span>
                        <span class="num">{s.tint.kelvin} K</span>
                        <span>{t("warmth_cool")}</span>
                      </div>
                    </div>
                  {:else}
                    <div class="palette" role="group" aria-label="{l.name}: {t('tint_colour')}">
                      {#each PALETTE as c}
                        <button
                          class="dot"
                          class:picked={sameRgb(s.tint, c)}
                          style="--c:{rgbCss(c)}"
                          aria-label={rgbCss(c)}
                          aria-pressed={sameRgb(s.tint, c)}
                          onclick={() => setColour(l.id, c)}
                        ></button>
                      {/each}
                      <label
                        class="dot any"
                        class:picked={!PALETTE.some((c) => sameRgb(s.tint, c))}
                        title={t("custom_colour")}
                      >
                        <input
                          type="color"
                          value={hexOf(s.tint)}
                          aria-label="{l.name}: {t('custom_colour')}"
                          oninput={(e) => {
                            const c = rgbOfHex(e.currentTarget.value);
                            if (c) setColour(l.id, c);
                          }}
                        />
                      </label>
                    </div>
                  {/if}
                {/if}
                <div class="panel-foot">
                  <button class="btn plain sm" onclick={() => identifyLight(l.id)} title={t("identify_tip")}>
                    <Icon name="flash" size={13} />
                    {t("identify")}
                  </button>
                  <button class="btn plain sm" onclick={() => showOnPlan(l.id)}>
                    <Icon name="plan" size={13} />
                    {t("show_on_plan")}
                  </button>
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/if}

  {#if setupDone}
    <section class="group">
      <div class="group-head"><h2 class="group-title">{t("ambilight")}</h2></div>
      <div class="card rows">
        {#if AUTO_SUPPORTED}
          <Row label={t("auto_mode")} tip={t("auto_mode_tip")}>
            <Switch
              checked={app.config.behavior.auto_fullscreen}
              label={t("auto_mode")}
              onchange={(v) => edit((c) => (c.behavior.auto_fullscreen = v))}
            />
          </Row>
        {/if}
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

  .tip {
    display: inline-grid;
    vertical-align: -2px;
    margin-left: 4px;
    opacity: 0.7;
  }

  /* --- Scenes --- */
  .scenes {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 var(--s-3) 0 10px;
    border-radius: var(--r-full);
    background: var(--surface);
    box-shadow: var(--card-edge);
    font-weight: 500;
    white-space: nowrap;
    transition:
      background var(--dur) var(--ease),
      box-shadow var(--dur) var(--ease);
  }
  .chip:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .chip:disabled:not(.pending) {
    opacity: 0.6;
  }
  .chip.active {
    box-shadow:
      inset 0 0 0 1.5px var(--glow),
      var(--card-edge);
  }
  .chip.pending .chip-dot {
    animation: breathe 900ms var(--ease) infinite alternate;
  }
  @keyframes breathe {
    to {
      opacity: 0.3;
    }
  }
  .chip-dot {
    width: 12px;
    height: 12px;
    flex: none;
    border-radius: 50%;
    background: var(--c);
    opacity: var(--a);
    box-shadow:
      inset 0 0 0 0.5px oklch(0 0 0 / 0.15),
      0 0 6px color-mix(in oklab, var(--c) 60%, transparent);
  }
  .chip-dots {
    display: inline-flex;
  }
  .chip-dots .chip-dot + .chip-dot {
    margin-left: -5px;
  }
  .chip-wrap {
    position: relative;
    display: inline-flex;
  }
  .chip.user {
    padding-right: 26px;
  }
  .chip-x {
    position: absolute;
    right: 5px;
    top: 50%;
    width: 18px;
    height: 18px;
    margin-top: -9px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--text-3);
    opacity: 0;
    transition: opacity var(--dur) var(--ease);
  }
  .chip-wrap:hover .chip-x,
  .chip-x:focus-visible {
    opacity: 1;
  }
  .chip-x:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .chip.add {
    background: transparent;
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--text-2);
  }
  .chip.add:hover {
    color: var(--text);
  }
  .name-form {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
  }
  .name-form input {
    height: 28px;
    width: 180px;
    padding: 0 var(--s-2);
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--control-edge);
    color: var(--text);
    font: inherit;
  }

  /* --- Lights --- */
  .light-row {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: 0 var(--s-4);
    min-height: 52px;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex: none;
    background: var(--c);
    transition:
      box-shadow 400ms var(--ease),
      background 300ms var(--ease);
  }
  .swatch.lit {
    box-shadow:
      0 0 0 3px color-mix(in oklab, var(--c) 22%, transparent),
      0 0 10px 2px color-mix(in oklab, var(--c) 55%, transparent);
  }
  .swatch.off {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
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
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chev {
    display: inline-grid;
    align-self: center;
    color: var(--text-3);
    transform: rotate(90deg);
    transition: transform 200ms var(--ease);
  }
  .expanded .chev {
    transform: rotate(-90deg);
  }

  .bright {
    width: 190px;
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--s-2);
    transition: opacity var(--dur) var(--ease);
  }
  .bright.dim {
    opacity: 0.45;
  }
  .bright.dim:hover,
  .bright.dim:focus-within {
    opacity: 1;
  }
  .bright output {
    min-width: 4.5ch;
    text-align: right;
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .live-tag {
    width: 190px;
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-sm);
    color: var(--glow-text);
  }
  .live-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--glow);
    box-shadow: 0 0 6px var(--glow);
  }
  .power {
    color: var(--text-3);
  }
  .power.on {
    color: var(--glow-text);
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
    padding: var(--s-1) var(--s-4) var(--s-3) calc(var(--s-4) + 10px + var(--s-3));
    animation: unfold 180ms var(--ease);
  }
  @keyframes unfold {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .hint {
    margin: 0;
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .mode {
    width: 220px;
  }
  .warmth {
    max-width: 420px;
  }
  .kelvin::-webkit-slider-runnable-track {
    height: 8px;
    background: linear-gradient(to right, var(--warm), var(--mid), var(--cool));
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.12);
  }
  .kelvin::-webkit-slider-thumb {
    margin-top: -5px;
  }
  .kelvin::-moz-range-track {
    height: 8px;
    background: linear-gradient(to right, var(--warm), var(--mid), var(--cool));
  }
  .kelvin::-moz-range-progress {
    background: transparent;
  }
  .ends {
    display: flex;
    justify-content: space-between;
    margin-top: 2px;
    font-size: var(--t-xs);
    color: var(--text-3);
  }
  .palette {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .dot {
    position: relative;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.18);
    cursor: pointer;
    transition: transform var(--dur) var(--spring);
  }
  .dot:hover {
    transform: scale(1.08);
  }
  .dot.picked {
    box-shadow:
      0 0 0 2px var(--surface),
      0 0 0 3.5px var(--text);
  }
  .dot.any {
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
  }
  .dot.any input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
  }
  .dot.any:focus-within {
    box-shadow: var(--ring);
  }
  .panel-foot {
    display: flex;
    gap: var(--s-1);
    margin-left: calc(var(--s-2) * -1);
  }
</style>
