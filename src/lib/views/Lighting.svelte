<script lang="ts">
  import {
    app,
    edit,
    adjustLight,
    applyScene,
    bindingFor,
    chooseMode,
    identifyLight,
    isDriven,
    isOn,
    isPlaying,
    lightState,
    modeOf,
    setAllPower,
    setLightMode,
    setSleepTimer,
    toggleEffect,
    toggleLightPower,
  } from "../state.svelte";
  import { t, pct, times } from "../i18n";
  import { describe, mapLight } from "../geometry";
  import { applyFeel, currentFeel, FEEL_ORDER, type Feel } from "../presets";
  import {
    MAX_KELVIN,
    MIN_KELVIN,
    MOODS,
    NATURAL,
    PALETTE,
    PALETTE_NAMES,
    PACED_SHOWS,
    SHOWS,
    hexOf,
    kelvinRgb,
    rgbCss,
    rgbOfHex,
    sameRgb,
    showPace,
    tintCss,
    type RgbTuple,
  } from "../scenes";
  import { defaultConfig, type LightState, type Mode } from "../types";
  import { DETECTS_FULLSCREEN } from "../platform";
  import Icon from "../ui/Icon.svelte";
  import Slider from "../ui/Slider.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Row from "../ui/Row.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

  const DEF = defaultConfig();

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const arranged = $derived(app.config.interface.room_arranged);
  const setupDone = $derived(bound > 0 && arranged);
  const live = $derived(app.status.running);
  const showPlaying = $derived(app.status.groups.some((g) => g.mode !== "ambilight"));
  const pacedPlaying = $derived(app.status.groups.some((g) => (PACED_SHOWS as Mode[]).includes(g.mode)));

  const feelOptions = $derived(
    FEEL_ORDER.map((f) => ({ value: f, label: t(`feel_${f}`), title: t(`feel_${f}_tip`) })),
  );
  /** The three feels, explained together behind the Response row's info mark. */
  const feelTip = $derived(FEEL_ORDER.map((f) => `${t(`feel_${f}`)}: ${t(`feel_${f}_tip`)}`).join("\n"));

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

  const modeName = (m: Mode | "manual") => t(`mode_${m}`);

  /** What a light can play, by hand first. */
  const LIGHT_MODES: (Mode | null)[] = [null, "ambilight", ...SHOWS.map((s) => s.id)];
  const SCREEN_SWATCH = "linear-gradient(90deg, oklch(0.62 0.2 30), oklch(0.45 0.12 300), oklch(0.6 0.16 250))";
  const modeSwatch = (m: Mode | null, s: LightState) =>
    m === null ? tintCss(s.tint) : m === "ambilight" ? SCREEN_SWATCH : (SHOWS.find((x) => x.id === m)?.preview ?? "");

  // --- Lights by hand ---

  /** The light whose white and colour controls are showing. */
  let open = $state<string | null>(null);

  // What each light was last given in the other mode, so switching between
  // White and Colour comes back to it rather than to a default.
  const lastKelvin: Record<string, number> = {};
  const lastColour: Record<string, RgbTuple> = {};

  function setTintMode(id: string, to: "white" | "color") {
    const tint = lightState(id).tint;
    if (tint.mode === to) return;
    if (tint.mode === "white") lastKelvin[id] = tint.kelvin;
    else lastColour[id] = [tint.r, tint.g, tint.b];
    if (to === "white") {
      adjustLight(id, { tint: { mode: "white", kelvin: lastKelvin[id] ?? 2700 } });
    } else {
      const [r, g, b] = lastColour[id] ?? PALETTE[1];
      adjustLight(id, { tint: { mode: "color", r, g, b } });
    }
  }

  function setColour(id: string, [r, g, b]: RgbTuple) {
    adjustLight(id, { tint: { mode: "color", r, g, b } });
  }

  function showOnPlan(id: string) {
    app.selectedLight = id;
    app.view = "room";
  }

  // --- Moods ---

  const same = (a: LightState | undefined, b: LightState) =>
    !!a && a.on === b.on && a.bright === b.bright && JSON.stringify(a.tint) === JSON.stringify(b.tint);

  /** The mood every light with a bulb shows now, if any. */
  const currentMood = $derived(
    live
      ? null
      : app.config.natural
        ? NATURAL
        : (MOODS.find((m) =>
          app.config.bindings.every((b) =>
            same(app.config.states[b.light_id], {
              on: true,
              bright: m.bright,
              tint: { mode: "white", kelvin: m.kelvin },
            }),
          ),
        )?.id ?? null),
  );

  /** The natural light's swatch: its day, from night to noon and back. */
  const naturalSwatch = `linear-gradient(135deg, ${[2200, 4000, 5500, 3000].map((k) => rgbCss(kelvinRgb(k))).join(", ")})`;

  // --- Sleep timer ---

  /** The timers offered, in minutes; the tray offers the same. */
  const SLEEP_PRESETS = [15, 30, 60, 90];
  const sleepOptions = $derived([
    { value: "off", label: t("sleep_off") },
    ...SLEEP_PRESETS.map((m) => ({ value: String(m), label: `${m} min` })),
  ]);

  let now = $state(Date.now());
  $effect(() => {
    if (!app.sleep) return;
    const tick = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(tick);
  });
  /** Seconds until the lights go off. */
  const sleepLeft = $derived(
    // `now` may predate the timer until the next tick.
    app.sleep ? Math.max(0, app.sleep.status.left - Math.max(0, now - app.sleep.at) / 1000) : null,
  );
  const sleepLabel = $derived(
    sleepLeft === null
      ? t("sleep_timer")
      : sleepLeft <= 60
        ? t("sleep_fading")
        : t("sleep_left", { n: Math.ceil(sleepLeft / 60) }),
  );
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
      <div class="group-head"><h2 class="group-title">{t("effects")}</h2></div>
      <div class="modes">
        <button
          class="tile screen"
          class:active={isPlaying("ambilight")}
          class:pending={app.pending === "ambilight"}
          aria-pressed={isPlaying("ambilight")}
          title={isPlaying("ambilight") ? t("click_to_stop") : undefined}
          disabled={app.busy && app.pending !== "ambilight"}
          onclick={() => chooseMode("ambilight")}
        >
          <span class="preview" aria-hidden="true">
            <span class="monitor"></span>
          </span>
          <span class="tile-text">
            <span class="tile-name">{t("mode_ambilight")}</span>
            <span class="tile-sub">
              {DETECTS_FULLSCREEN && app.config.behavior.auto_fullscreen ? t("mode_ambilight_auto") : t("mode_ambilight_sub")}
            </span>
          </span>
          {#if isPlaying("ambilight")}<span class="on-dot" aria-hidden="true"></span>{/if}
        </button>
        {#each SHOWS as s (s.id)}
          <button
            class="tile"
            class:active={isPlaying(s.id)}
            class:pending={app.pending === s.id}
            aria-pressed={isPlaying(s.id)}
            title={isPlaying(s.id) ? t("click_to_stop") : undefined}
            disabled={app.busy && app.pending !== s.id}
            onclick={() => chooseMode(s.id)}
          >
            <span class="preview" style="--p:{s.preview}" aria-hidden="true"></span>
            <span class="tile-text">
              <span class="tile-name">{modeName(s.id)}</span>
            </span>
            {#if isPlaying(s.id)}<span class="on-dot" aria-hidden="true"></span>{/if}
          </button>
        {/each}
      </div>

      {#if isPlaying("ambilight")}
        <div class="card rows tune">
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
          <Row label={t("response")} tip={feelTip}>
            <Segmented
              label={t("response")}
              value={currentFeel(app.config)}
              options={feelOptions}
              onchange={(f: Feel) => edit((c) => applyFeel(c, f))}
            />
          </Row>
        </div>
      {/if}
      {#if showPlaying}
        <div class="card rows tune">
          <Row label={t("brightness")}>
            <Slider
              label={t("brightness")}
              value={app.config.show.bright}
              min={5}
              max={100}
              step={1}
              def={DEF.show.bright}
              format={(v) => pct(v / 100)}
              onchange={(v) => edit((c) => (c.show.bright = v))}
            />
          </Row>
          {#if pacedPlaying}
            <Row label={t("speed")}>
              <Slider
                label={t("speed")}
                value={app.config.show.speed}
                min={1}
                max={100}
                step={1}
                def={DEF.show.speed}
                format={(v) => times(showPace(v))}
                ends={[t("speed_slow"), t("speed_fast")]}
                onchange={(v) => edit((c) => (c.show.speed = v))}
              />
            </Row>
          {/if}
        </div>
      {/if}
    </section>

    <section class="group">
      <div class="group-head"><h2 class="group-title">{t("moods")}</h2></div>
      <div class="moods">
        <button
          class="mood"
          class:active={currentMood === NATURAL}
          class:pending={app.pending === NATURAL}
          aria-pressed={currentMood === NATURAL}
          title={t("mood_natural_tip")}
          disabled={app.pending !== null}
          onclick={() => applyScene(NATURAL)}
        >
          <span class="mood-dot" style="--c:{rgbCss(kelvinRgb(4000))};--a:1;background:{naturalSwatch}" aria-hidden="true"
          ></span>
          {t("mood_natural")}
        </button>
        {#each MOODS as m (m.id)}
          <button
            class="mood"
            class:active={currentMood === m.id}
            class:pending={app.pending === m.id}
            aria-pressed={currentMood === m.id}
            disabled={app.pending !== null}
            onclick={() => applyScene(m.id)}
          >
            <span
              class="mood-dot"
              style="--c:{rgbCss(kelvinRgb(m.kelvin))};--a:{0.4 + (0.6 * m.bright) / 100}"
              aria-hidden="true"
            ></span>
            {t(`mood_${m.id}`)}
          </button>
        {/each}
      </div>
      <div class="card rows tune">
        <Row label={sleepLabel} tip={t("sleep_timer_tip")}>
          <Segmented
            label={t("sleep_timer")}
            value={app.sleep ? String(app.sleep.status.minutes) : "off"}
            options={sleepOptions}
            onchange={(v) => setSleepTimer(v === "off" ? null : +v)}
          />
        </Row>
      </div>
    </section>
  {/if}

  {#if lights.length > 0}
    <section class="group">
      <div class="group-head"><h2 class="group-title">{t("lights")}</h2></div>
      <div class="card rows">
        {#each lights as l (l.id)}
          {@const m = mapLight(app.config.room.screen, l)}
          {@const b = bindingFor(l.id)}
          {@const s = lightState(l.id)}
          {@const driven = isDriven(l.id)}
          {@const lm = modeOf(l.id)}
          {@const expanded = open === l.id && !!b}
          <div class="light" class:expanded>
            <div class="light-row">
              <span
                class="swatch"
                class:lit={b && isOn(l.id)}
                class:off={b && !isOn(l.id)}
                class:live={driven}
                style="--c:{b && !driven ? tintCss(s.tint) : 'var(--line-strong)'}"
                aria-hidden="true"
              ></span>
              <button
                class="light-text"
                aria-expanded={b ? expanded : undefined}
                title={b ? t("light_more", { name: l.name }) : t("show_on_plan")}
                onclick={() => (b ? (open = expanded ? null : l.id) : showOnPlan(l.id))}
              >
                <span class="light-name">{l.name}</span>
                {#if !b}
                  <span class="light-sub">{t("no_bulb")}</span>
                {:else}
                  <span class="chev" aria-hidden="true"><Icon name="chevron" size={12} /></span>
                {/if}
              </button>

              {#if !b}
                <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
              {:else if lm}
                <span class="live-tag">
                  <span class="lamp" aria-hidden="true"></span>
                  {modeName(lm)}{#if lm === "ambilight"}<span class="where">· {t(describe(m))}</span>{/if}
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
            </div>

            {#if expanded}
              <div class="panel">
                <div class="light-modes" role="radiogroup" aria-label="{l.name}: {t('light_mode')}" title={t("light_mode_tip")}>
                  {#each LIGHT_MODES as option (option ?? "manual")}
                    <button
                      class="chip"
                      class:picked={lm === option}
                      role="radio"
                      aria-checked={lm === option}
                      disabled={app.busy || app.modeBusy[l.id]}
                      onclick={() => setLightMode(l.id, option)}
                    >
                      <span class="chip-dot" style="background:{modeSwatch(option, s)}" aria-hidden="true"></span>
                      {modeName(option ?? "manual")}
                    </button>
                  {/each}
                </div>
                {#if driven}
                  <p class="hint">{t("driven_hint")}</p>
                {:else}
                  <div class="tint-mode">
                    <Segmented
                      label="{l.name}: {t('white')} / {t('tint_colour')}"
                      value={s.tint.mode}
                      options={tintOptions}
                      onchange={(v) => setTintMode(l.id, v)}
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
                        <span>{t("warmth_cool")}</span>
                      </div>
                    </div>
                  {:else}
                    <div class="palette" role="group" aria-label="{l.name}: {t('tint_colour')}">
                      {#each PALETTE as c, i}
                        <button
                          class="dot"
                          class:picked={sameRgb(s.tint, c)}
                          style="--c:{rgbCss(c)}"
                          title={t(`colour_${PALETTE_NAMES[i]}`)}
                          aria-label={t(`colour_${PALETTE_NAMES[i]}`)}
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

  /* --- Effects --- */
  .modes {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--s-2);
  }
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--s-2);
    padding: var(--s-2);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
    text-align: left;
    transition:
      box-shadow 200ms var(--ease),
      transform 200ms var(--spring);
  }
  .tile:hover:not(:disabled) {
    transform: translateY(-1px);
  }
  .tile:disabled:not(.pending) {
    opacity: 0.55;
  }
  /* The flagship gets the whole first row. */
  .tile.screen {
    grid-column: 1 / -1;
    flex-direction: row;
    align-items: center;
    gap: var(--s-4);
  }
  .screen .tile-text {
    flex: 1;
  }
  .tile.active {
    box-shadow:
      inset 0 0 0 2px var(--glow),
      0 6px 22px -8px var(--glow);
  }
  .preview {
    flex: 1;
    min-height: 54px;
    border-radius: var(--r-md);
    background: var(--p);
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.12);
  }
  .active .preview {
    animation: drift 6s linear infinite alternate;
    background-size: 160% 160%;
  }
  @keyframes drift {
    from {
      background-position: 0% 50%;
    }
    to {
      background-position: 100% 50%;
    }
  }
  .pending .preview {
    animation: breathe 800ms var(--ease) infinite alternate;
  }
  @keyframes breathe {
    to {
      opacity: 0.45;
    }
  }
  /* The screen tile: a monitor whose glow spills onto the wall behind. */
  .screen .preview {
    position: relative;
    flex: none;
    width: 200px;
    display: grid;
    place-items: center;
    min-height: 96px;
    background:
      radial-gradient(60% 70% at 30% 50%, oklch(0.7 0.18 30 / 0.55), transparent 70%),
      radial-gradient(60% 70% at 70% 50%, oklch(0.65 0.15 250 / 0.55), transparent 70%),
      var(--sunken);
  }
  .screen:not(.active) .preview {
    filter: saturate(0.5);
  }
  .monitor {
    width: 46%;
    aspect-ratio: 16 / 9;
    border-radius: 4px;
    background: linear-gradient(90deg, oklch(0.62 0.2 30), oklch(0.45 0.12 300), oklch(0.6 0.16 250));
    box-shadow:
      0 0 0 3px oklch(0.2 0 0),
      0 10px 18px -6px oklch(0 0 0 / 0.5);
  }
  .tile-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 2px 2px;
  }
  .tile-name {
    font-weight: 600;
  }
  .screen .tile-name {
    font-size: var(--t-lg);
  }
  .tile-sub {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .on-dot {
    position: absolute;
    top: calc(var(--s-2) + 7px);
    right: calc(var(--s-2) + 7px);
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--glow);
    box-shadow:
      0 0 0 2px var(--surface),
      0 0 8px var(--glow);
  }
  .tune {
    margin-top: var(--s-3);
  }
  /* --- Moods --- */
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
  .mood.active {
    box-shadow: inset 0 0 0 2px var(--glow);
  }
  .mood.pending .mood-dot {
    animation: breathe 800ms var(--ease) infinite alternate;
  }
  .mood-dot {
    width: 14px;
    height: 14px;
    flex: none;
    border-radius: 50%;
    background: var(--c);
    opacity: var(--a);
    box-shadow:
      inset 0 0 0 0.5px oklch(0 0 0 / 0.15),
      0 0 8px color-mix(in oklab, var(--c) 70%, transparent);
  }

  /* --- Lights --- */
  .light-row {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: 0 var(--s-2) 0 var(--s-4);
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
  .swatch.live {
    background: conic-gradient(#ff3b30, #ffb800, #34c759, #0a84ff, #bf5af2, #ff3b30);
    animation: spin 4s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(1turn);
    }
  }
  .light-text {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
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
  .light-sub {
    font-size: var(--t-sm);
    color: var(--text-3);
    white-space: nowrap;
  }
  .chev {
    display: inline-grid;
    color: var(--text-3);
    transform: rotate(90deg);
    transition: transform 200ms var(--ease);
  }
  .expanded .chev {
    transform: rotate(-90deg);
  }
  .bright {
    width: 200px;
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
    width: 200px;
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--glow-text);
    white-space: nowrap;
    overflow: hidden;
  }
  .where {
    font-weight: 400;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .live-tag .lamp {
    --lamp: 6px;
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
  .light-modes {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1);
  }
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
  .chip:hover:not(:disabled) {
    background: var(--surface-hover);
  }
  .chip:disabled:not(.picked) {
    opacity: 0.55;
  }
  .chip.picked {
    box-shadow: inset 0 0 0 2px var(--glow);
  }
  .chip-dot {
    width: 12px;
    height: 12px;
    flex: none;
    border-radius: 50%;
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.18);
  }
  .hint {
    margin: 0;
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  .tint-mode {
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
  .dot.any:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .panel-foot {
    display: flex;
    gap: var(--s-1);
    margin-left: calc(var(--s-2) * -1);
  }
  /* Reduced motion: the loops stand still. A pending choice dims instead of
     breathing; a light the effect drives shows the spectrum without turning. */
  @media (prefers-reduced-motion: reduce) {
    .active .preview,
    .swatch.live {
      animation: none;
    }
    .pending .preview,
    .mood.pending .mood-dot {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
