<script lang="ts">
  import {
    app,
    applyScene,
    chooseMode,
    edit,
    effectTargets,
    lightById,
    modeOf,
    moodTargets,
  } from "../../state.svelte";
  import { t, times } from "../../i18n";
  import { applyFeel, currentFeel, FEEL_ORDER, type Feel } from "../../presets";
  import { MOODS, NATURAL, PACED_SHOWS, SHOWS, kelvinRgb, rgbCss, showPace } from "../../scenes";
  import { defaultConfig, type Mode } from "../../types";
  import { HEARS_OUTPUTS } from "../../platform";
  import { describe, mapLight } from "../../geometry";
  import Slider from "../../ui/Slider.svelte";
  import Segmented from "../../ui/Segmented.svelte";
  import Row from "../../ui/Row.svelte";
  import Switch from "../../ui/Switch.svelte";
  import Icon from "../../ui/Icon.svelte";
  import ZoneBadge from "../../ui/ZoneBadge.svelte";
  import { modeName, moodName, moodOf, naturalSwatch } from "./modes";

  // What the lights picked do: one grid of modes, the fixed ones (moods) and
  // the animated ones (the screen and the shows), and under them the settings
  // of the animated ones playing there.

  const DEF = defaultConfig();
  const nameOf = (id: string) => lightById(id)?.name ?? id;

  /** Every light picked with a bulb. */
  const picked = $derived(moodTargets());

  // --- Fixed: the moods ---

  const moodIds = $derived(moodTargets());
  const showingMood = (mood: string) => moodIds.filter((id) => moodOf(id) === mood);
  const moodAll = (mood: string) => moodIds.length > 0 && showingMood(mood).length === moodIds.length;
  // Each mood's well is the pool of its white on a dark wall, as strong as
  // the mood is bright; the natural light's is its day, night to noon.
  const pool = (kelvin: number, bright: number) =>
    `radial-gradient(110% 150% at 50% 110%, color-mix(in oklab, ${rgbCss(kelvinRgb(kelvin))} ${25 + 0.75 * bright}%, transparent), transparent 80%)`;
  const fixed = $derived([
    { id: NATURAL, name: moodName(NATURAL), preview: naturalSwatch, tip: t("mood_natural_tip") },
    ...MOODS.map((m) => ({ id: m.id, name: moodName(m.id), preview: pool(m.kelvin, m.bright), tip: undefined })),
  ]);

  // --- Animated: the screen and the shows ---

  const targets = $derived(effectTargets());
  const playing = (mode: Mode) => targets.filter((id) => modeOf(id) === mode);
  const playsAll = (mode: Mode) => targets.length > 0 && playing(mode).length === targets.length;
  // Played by some of the lights picked: who, said on hover rather than in
  // the tile, which only carries a dashed ring.
  const tileTitle = (mode: Mode) => {
    if (playsAll(mode)) return t("click_to_stop");
    const names = playing(mode).map(nameOf);
    return names.length > 0 ? `${names.join(", ")}. ${t("click_to_extend")}` : undefined;
  };
  const animated = $derived([
    { id: "ambilight" as Mode, preview: null },
    ...SHOWS.map((s) => ({ id: s.id as Mode, preview: s.preview })),
  ]);

  // --- The settings of what plays on the selection ---

  const modesHere = $derived(new Set(picked.map(modeOf)));
  /** The lights picked that follow the screen, with what of it each mirrors. */
  const screenLights = $derived(
    picked
      .filter((id) => modeOf(id) === "ambilight")
      .flatMap((id) => {
        const l = lightById(id);
        return l ? [{ l, m: mapLight(app.config.room.screen, l) }] : [];
      }),
  );
  const pacedHere = $derived((PACED_SHOWS as (Mode | null)[]).some((m) => modesHere.has(m)));
  const musicHere = $derived(modesHere.has("music"));

  const feelOptions = $derived(
    FEEL_ORDER.map((f) => ({ value: f, label: t(`feel_${f}`), title: t(`feel_${f}_tip`) })),
  );
  /** The three feels, explained together behind the Response row's info mark. */
  const feelTip = $derived(FEEL_ORDER.map((f) => `${t(`feel_${f}`)}: ${t(`feel_${f}_tip`)}`).join("\n"));
  // What the music listens to, by the name the source had when picked.
  const soundName = $derived.by(() => {
    const a = app.config.audio;
    if (a.kind !== "default") return a.name;
    return t(HEARS_OUTPUTS ? "audio_default_output" : "audio_default_input");
  });

  function showOnPlan(id: string) {
    app.selectedLight = id;
    app.view = "room";
  }
</script>

<!-- The heading over the column names the lights; the two kinds of mode
     need no title of their own above them. -->
<section class="group modes" aria-label={t("mode")}>
  <h3 class="group-title kind">{t("modes_fixed")}</h3>
  <div class="grid">
    {#each fixed as m (m.id)}
      {@const names = showingMood(m.id).map(nameOf)}
      <button
        class="tile still"
        class:natural={m.id === NATURAL}
        class:active={moodAll(m.id)}
        class:partial={names.length > 0 && !moodAll(m.id)}
        class:pending={app.pending === m.id}
        aria-pressed={moodAll(m.id) ? true : names.length > 0 ? "mixed" : false}
        title={names.length > 0 && !moodAll(m.id) ? names.join(", ") : m.tip}
        disabled={app.pending !== null}
        onclick={() => applyScene(m.id)}
      >
        <span class="preview" style="--p:{m.preview}" aria-hidden="true"></span>
        <!-- Showing: an amber ring and a lamp, not the colour alone. -->
        <span class="tile-name">
          {#if moodAll(m.id)}<span class="lamp" aria-hidden="true"></span>{/if}{m.name}
        </span>
      </button>
    {/each}
  </div>

  <h3 class="group-title kind">{t("modes_animated")}</h3>
  <div class="grid">
    {#each animated as a (a.id)}
      {@const some = playing(a.id).length > 0}
      <button
        class="tile"
        class:screen={a.id === "ambilight"}
        class:active={playsAll(a.id)}
        class:partial={some && !playsAll(a.id)}
        class:pending={app.pending === a.id}
        aria-pressed={playsAll(a.id) ? true : some ? "mixed" : false}
        title={tileTitle(a.id)}
        disabled={app.busy && app.pending !== a.id}
        onclick={() => chooseMode(a.id)}
      >
        {#if a.preview}
          <span class="preview" style="--p:{a.preview}" aria-hidden="true"></span>
        {:else}
          <span class="preview" aria-hidden="true"><span class="monitor"></span></span>
        {/if}
        <span class="tile-name">
          {#if playsAll(a.id)}<span class="lamp" aria-hidden="true"></span>{/if}{modeName(a.id)}
        </span>
      </button>
    {/each}
  </div>

  {#if screenLights.length > 0 || pacedHere || musicHere}
    <div class="card rows tune">
      {#if screenLights.length > 0}
        {#if app.status.screen_lost}
          <p class="line warn" role="status">
            <Icon name="alert" size={14} />{t("screen_lost")}
          </p>
        {/if}
        <!-- What of the picture each light mirrors, changed on the plan. -->
        {#each screenLights as { l, m }, i (l.id)}
          <div class="zone">
            <span class="zone-label">{picked.length === 1 ? t("mirrors") : l.name}</span>
            <span class="zone-what">
              <ZoneBadge edge={m.edge} labelled />{t(describe(m))}
              {#if i === 0 && app.status.hdr}
                <span class="hdr" title={t("hdr_tip")}>{t("hdr")}</span><span class="sr-only">{t("hdr_tip")}</span>
              {/if}
            </span>
            <button
              class="btn plain icon sm"
              title={t("change_on_plan")}
              aria-label={t("labelled", { name: l.name, what: t("change_on_plan") })}
              onclick={() => showOnPlan(l.id)}
            >
              <Icon name="plan" size={14} />
            </button>
          </div>
        {/each}
        <Row label={t("response")} tip={feelTip}>
          <Segmented
            label={t("response")}
            value={currentFeel(app.config)}
            options={feelOptions}
            onchange={(f: Feel) => edit((c) => applyFeel(c, f))}
          />
        </Row>
      {/if}
      <!-- The music keeps the track's own time: beside it, a speed
           would read as the music's. -->
      {#if pacedHere && !musicHere}
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
      {#if musicHere}
        <Row label={t("flashes")} tip={t("flashes_tip")}>
          <Switch
            checked={app.config.show.flashes}
            label={t("flashes")}
            onchange={(v) => edit((c) => (c.show.flashes = v))}
          />
        </Row>
        <Row label={t("listens_to")}>
          <div class="listen">
            <span class="listen-name" class:waiting={app.status.music_waiting}>
              {app.status.music_waiting ? t("sound_waiting", { name: soundName }) : soundName}
            </span>
            <button class="btn plain sm" onclick={() => (app.view = "settings")}>{t("change")}</button>
          </div>
        </Row>
      {/if}
    </div>
  {/if}
</section>

<style>
  /* The grids count their columns from the room they have, beside the
     lights or under them. */
  .modes {
    container-type: inline-size;
  }
  .kind {
    padding: 0 var(--s-1) var(--s-2);
  }
  .grid + .kind {
    margin-top: var(--s-4);
  }
  /* Three columns beside the lights, six with the room for them: the
     natural light takes two, so neither row is left with a gap. */
  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--s-2);
  }
  .natural {
    grid-column: span 2;
  }
  @container (min-width: 560px) {
    .grid {
      grid-template-columns: repeat(6, minmax(0, 1fr));
    }
  }

  /* --- Animated --- */
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--s-2);
    min-width: 0;
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
  .preview {
    height: 38px;
    border-radius: var(--r-md);
    background: var(--p);
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.12);
  }
  /* A mood's pool of light needs a wall to fall on: the same dark well in
     both themes, as a room at night. */
  .still .preview {
    background: var(--p), var(--well-night);
  }
  .natural .preview {
    background: var(--p);
  }
  .still:not(.active) .preview {
    filter: saturate(0.85);
  }
  .tile:not(.still).active .preview {
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
  /* The screen: a monitor whose glow spills onto the wall behind. */
  .screen .preview {
    display: grid;
    place-items: center;
    background:
      radial-gradient(60% 70% at 25% 50%, oklch(0.72 0.15 55 / 0.55), transparent 70%),
      radial-gradient(60% 70% at 75% 50%, oklch(0.65 0.15 250 / 0.55), transparent 70%),
      var(--sunken);
  }
  .screen.active .preview {
    animation: none;
  }
  .screen:not(.active) .preview {
    filter: saturate(0.5);
  }
  /* Sized from the well's height, so it fits however narrow the tile. */
  .monitor {
    height: 62%;
    aspect-ratio: 16 / 9;
    border-radius: 3px;
    background: linear-gradient(90deg, oklch(0.66 0.16 55), oklch(0.45 0.12 300), oklch(0.6 0.16 250));
    box-shadow:
      0 0 0 2px var(--bezel),
      var(--bezel-shadow);
  }
  /* Names wrap rather than truncate: French ones run long. */
  .tile-name {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 2px 2px;
    font-weight: 600;
    line-height: 1.25;
    text-wrap: balance;
  }
  .tile-name .lamp {
    --lamp: 6px;
  }

  /* Playing: an amber ring, with a lamp and "Playing" in words. Some of the
     lights picked play it: a dashed ring and their names. */
  .tile.partial {
    outline: 1.5px dashed var(--active);
    outline-offset: -1.5px;
  }
  .tile.active {
    box-shadow: inset 0 0 0 2px var(--active);
  }
  .tile.pending .preview {
    animation: breathe 800ms var(--ease) infinite alternate;
  }
  @keyframes breathe {
    to {
      opacity: 0.45;
    }
  }

  /* --- The settings of what plays --- */
  .tune {
    margin-top: var(--s-3);
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-2) var(--s-4);
    font-size: var(--t-sm);
  }
  /* The screen cannot be read: said in words, the colour only marks it. */
  .line.warn {
    color: var(--danger);
  }
  .zone {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    min-height: 44px;
    padding: 0 var(--s-2) 0 var(--s-4);
  }
  .zone-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .zone-what {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    color: var(--text-2);
  }
  /* The screen is read in HDR: a plain outlined tag, not amber, since it
     says how the picture is read, not that a light is on. */
  .hdr {
    margin-left: var(--s-1);
    padding: 0 5px;
    border-radius: var(--r-sm);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    line-height: 16px;
  }
  .listen {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--s-2);
    min-width: 0;
  }
  .listen-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .listen-name.waiting {
    color: var(--text-3);
  }

  /* Reduced motion: the loops stand still, and a pending choice dims
     instead of breathing. */
  @media (prefers-reduced-motion: reduce) {
    .active .preview {
      animation: none;
    }
    .tile.pending .preview {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
