<script lang="ts">
  import {
    app,
    edit,
    adjustLight,
    applyScene,
    bindingFor,
    chooseMode,
    effectTargets,
    identifyLight,
    isDriven,
    isOn,
    isPlaying,
    lightState,
    modeOf,
    moodTargets,
    setAllPower,
    setLightMode,
    setSleepTimer,
    toggleEffect,
    toggleLightPower,
    toggleTarget,
  } from "../state.svelte";
  import { t, tn, pct, times } from "../i18n";
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
    type MoodId,
    type RgbTuple,
  } from "../scenes";
  import { defaultConfig, type LightState, type Mode } from "../types";
  import { DETECTS_FULLSCREEN } from "../platform";
  import Icon from "../ui/Icon.svelte";
  import Slider from "../ui/Slider.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Row from "../ui/Row.svelte";
  import Switch from "../ui/Switch.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";

  const DEF = defaultConfig();

  const lights = $derived(app.config.room.lights);
  const bound = $derived(app.config.bindings.length);
  const arranged = $derived(app.config.interface.room_arranged);
  const setupDone = $derived(bound > 0 && arranged);
  const live = $derived(app.status.running);
  const showPlaying = $derived(app.status.groups.some((g) => g.mode !== "ambilight"));
  const pacedPlaying = $derived(app.status.groups.some((g) => (PACED_SHOWS as Mode[]).includes(g.mode)));
  const musicPlaying = $derived(app.status.groups.some((g) => g.mode === "music"));

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

  // --- Where effects and moods go ---

  /** The lights that can be picked: those with a bulb. */
  const pickable = $derived(lights.filter((l) => bindingFor(l.id)));
  const wholeRoom = $derived(!app.target.some((id) => bindingFor(id)));
  const targets = $derived(effectTargets());
  /** What the lights picked are called, shown by the effects and moods. */
  const targetName = $derived.by(() => {
    if (wholeRoom) return null;
    const picked = pickable.filter((l) => app.target.includes(l.id));
    return picked.length === 1 ? picked[0].name : tn("n_lights", picked.length);
  });

  /** How many of the lights an effect would go to play `mode` already. */
  const shareOf = (mode: Mode) => targets.filter((id) => modeOf(id) === mode).length;
  const playsAll = (mode: Mode) => targets.length > 0 && shareOf(mode) === targets.length;
  const tileTitle = (mode: Mode) =>
    playsAll(mode) ? t("click_to_stop") : shareOf(mode) > 0 ? t("click_to_extend") : undefined;

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

  /** The mood a light shows now, if it shows one: the natural light while
   *  it follows it, or the mood its state is. */
  const moodOf = (id: string): string | null => {
    if (isDriven(id)) return null;
    if (app.config.natural?.includes(id)) return NATURAL;
    const s = app.config.states[id];
    return (
      MOODS.find((m) => same(s, { on: true, bright: m.bright, tint: { mode: "white", kelvin: m.kelvin } }))?.id ?? null
    );
  };
  const moodName = (id: string) => t(id === NATURAL ? "mood_natural" : `mood_${id as MoodId}`);

  const moodIds = $derived(moodTargets());
  /** How many of the lights a mood would go to show `mood` already. */
  const moodShare = (mood: string) => moodIds.filter((id) => moodOf(id) === mood).length;
  const moodAll = (mood: string) => moodIds.length > 0 && moodShare(mood) === moodIds.length;

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

  {#if pickable.length > 1}
    <section class="group">
      <div class="group-head"><h2 class="group-title" title={t("apply_to_tip")}>{t("apply_to")}</h2></div>
      <div class="targets" role="group" aria-label={t("apply_to")}>
        <button
          class="chip"
          class:picked={wholeRoom}
          aria-pressed={wholeRoom}
          onclick={() => (app.target = [])}
        >
          <Icon name="plan" size={13} />
          {t("whole_room")}
        </button>
        {#each pickable as l (l.id)}
          {@const picked = !wholeRoom && app.target.includes(l.id)}
          {@const dark = !isDriven(l.id) && !isOn(l.id)}
          <button class="chip" class:picked aria-pressed={picked} onclick={() => toggleTarget(l.id)}>
            <span
              class="chip-dot"
              class:off={dark}
              style={dark ? undefined : `background:${modeSwatch(modeOf(l.id), lightState(l.id))}`}
              aria-hidden="true"
            ></span>
            {l.name}
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if bound > 0}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title">
          {t("effects")}{#if targetName}<span class="for">{` · ${targetName}`}</span>{/if}
        </h2>
      </div>
      <div class="modes">
        <button
          class="tile screen"
          class:active={playsAll("ambilight")}
          class:partial={shareOf("ambilight") > 0 && !playsAll("ambilight")}
          class:pending={app.pending === "ambilight"}
          aria-pressed={playsAll("ambilight") ? true : shareOf("ambilight") > 0 ? "mixed" : false}
          title={tileTitle("ambilight")}
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
          {#if shareOf("ambilight") > 0 && !playsAll("ambilight")}
            <span class="share">{t("share_of", { n: shareOf("ambilight"), total: targets.length })}</span>
          {/if}
        </button>
        {#each SHOWS as s (s.id)}
          <button
            class="tile"
            class:active={playsAll(s.id)}
            class:partial={shareOf(s.id) > 0 && !playsAll(s.id)}
            class:pending={app.pending === s.id}
            aria-pressed={playsAll(s.id) ? true : shareOf(s.id) > 0 ? "mixed" : false}
            title={tileTitle(s.id)}
            disabled={app.busy && app.pending !== s.id}
            onclick={() => chooseMode(s.id)}
          >
            <span class="preview" style="--p:{s.preview}" aria-hidden="true"></span>
            <span class="tile-text">
              <span class="tile-name">{modeName(s.id)}</span>
              {#if shareOf(s.id) > 0 && !playsAll(s.id)}
                <span class="tile-sub">{t("share_of", { n: shareOf(s.id), total: targets.length })}</span>
              {/if}
            </span>
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
          {#if musicPlaying}
            <Row label={t("flashes")} tip={t("flashes_tip")}>
              <Switch
                checked={app.config.show.flashes}
                label={t("flashes")}
                onchange={(v) => edit((c) => (c.show.flashes = v))}
              />
            </Row>
          {/if}
        </div>
      {/if}
    </section>

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
            {#if moodShare(NATURAL) > 0 && !moodAll(NATURAL)}
              <span class="share">{t("share_of", { n: moodShare(NATURAL), total: moodIds.length })}</span>
            {/if}
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
              {#if moodShare(m.id) > 0 && !moodAll(m.id)}
                <span class="share">{t("share_of", { n: moodShare(m.id), total: moodIds.length })}</span>
              {/if}
            </span>
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
          <div class="light" class:expanded class:targeted={!wholeRoom && app.target.includes(l.id)}>
            <div class="light-row">
              <span
                class="swatch"
                class:lit={b && isOn(l.id)}
                class:off={b && !isOn(l.id)}
                style="--c:{b && !driven ? tintCss(s.tint) : 'var(--line-strong)'}{driven ? `;background:${modeSwatch(lm, s)}` : ''}"
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
                  {@const mood = s.on ? moodOf(l.id) : null}
                  {#if mood}<span class="light-sub">{moodName(mood)}</span>{/if}
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
      background var(--dur) var(--ease);
  }
  .tile:hover:not(:disabled) {
    background: var(--surface-hover);
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
  /* Some of the lights picked play it: a dashed edge, not the full ring. */
  .tile.partial,
  .mood.partial {
    outline: 1.5px dashed color-mix(in oklab, var(--glow) 70%, transparent);
    outline-offset: -1.5px;
  }
  .share {
    font-size: var(--t-sm);
    color: var(--text-3);
    white-space: nowrap;
  }
  .tile.active {
    box-shadow: inset 0 0 0 2px var(--glow);
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
  .mood-text {
    display: flex;
    flex-direction: column;
  }
  .mood-text .share {
    font-size: var(--t-xs);
    font-weight: 400;
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
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.15);
  }

  .for {
    color: var(--glow-text);
    font-weight: 500;
  }

  /* --- Lights --- */
  .light.targeted {
    background: color-mix(in oklab, var(--glow) 8%, transparent);
  }
  .light:first-child {
    border-radius: var(--r-lg) var(--r-lg) 0 0;
  }
  .light:last-child {
    border-radius: 0 0 var(--r-lg) var(--r-lg);
  }
  .light:only-child {
    border-radius: var(--r-lg);
  }
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
      0 0 0 3px color-mix(in oklab, var(--c) 28%, transparent);
  }
  .swatch.off {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
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
  .chip-dot.off {
    box-shadow: inset 0 0 0 1.5px var(--line-strong);
  }
  .targets {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1);
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
    transition: transform var(--dur) var(--ease);
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
    .active .preview {
      animation: none;
    }
    .pending .preview,
    .mood.pending .mood-dot {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
