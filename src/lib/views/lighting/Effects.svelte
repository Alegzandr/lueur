<script lang="ts">
  import { app, edit, chooseMode, effectTargets, isPlaying, modeOf } from "../../state.svelte";
  import { t, pct, times } from "../../i18n";
  import { applyFeel, currentFeel, FEEL_ORDER, type Feel } from "../../presets";
  import { PACED_SHOWS, SHOWS, showPace } from "../../scenes";
  import { defaultConfig, type Mode } from "../../types";
  import { DETECTS_FULLSCREEN, HEARS_OUTPUTS } from "../../platform";
  import Slider from "../../ui/Slider.svelte";
  import Segmented from "../../ui/Segmented.svelte";
  import Row from "../../ui/Row.svelte";
  import Switch from "../../ui/Switch.svelte";
  import Status from "./Status.svelte";
  import { modeName } from "./modes";

  // The ambilight and show tiles, and the settings of what plays.

  let {
    targetName,
  }: {
    /** What the lights picked are called, or null for the whole room. */
    targetName: string | null;
  } = $props();

  const DEF = defaultConfig();

  const showPlaying = $derived(app.status.groups.some((g) => g.mode !== "ambilight"));
  const pacedPlaying = $derived(app.status.groups.some((g) => (PACED_SHOWS as Mode[]).includes(g.mode)));
  const musicPlaying = $derived(app.status.groups.some((g) => g.mode === "music"));

  const feelOptions = $derived(
    FEEL_ORDER.map((f) => ({ value: f, label: t(`feel_${f}`), title: t(`feel_${f}_tip`) })),
  );
  // What the music listens to, by the name the source had when picked.
  const soundName = $derived.by(() => {
    const a = app.config.audio;
    if (a.kind !== "default") return a.name;
    return t(HEARS_OUTPUTS ? "audio_default_output" : "audio_default_input");
  });
  /** The three feels, explained together behind the Response row's info mark. */
  const feelTip = $derived(FEEL_ORDER.map((f) => `${t(`feel_${f}`)}: ${t(`feel_${f}_tip`)}`).join("\n"));

  const targets = $derived(effectTargets());

  /** How many of the lights an effect would go to play `mode` already. */
  const shareOf = (mode: Mode) => targets.filter((id) => modeOf(id) === mode).length;
  const playsAll = (mode: Mode) => targets.length > 0 && shareOf(mode) === targets.length;
  const tileTitle = (mode: Mode) =>
    playsAll(mode) ? t("click_to_stop") : shareOf(mode) > 0 ? t("click_to_extend") : undefined;
</script>

<section class="group" id="effects">
  <div class="group-head">
    <h2 class="group-title" tabindex="-1">
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
        <span class="tile-name"
          >{t("mode_ambilight")}{#if app.status.hdr && shareOf("ambilight") > 0}{" "}<span class="hdr"
              >{t("hdr")}</span
            ><span class="sr-only">{t("hdr_tip")}</span>{/if}</span
        >
        <span class="tile-sub">
          {DETECTS_FULLSCREEN && app.config.behavior.auto_fullscreen ? t("mode_ambilight_auto") : t("mode_ambilight_sub")}
        </span>
      </span>
      <Status all={playsAll("ambilight")} share={shareOf("ambilight")} total={targets.length} />
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
          <span class="tile-state"><Status all={playsAll(s.id)} share={shareOf(s.id)} total={targets.length} /></span>
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
      <!-- The music keeps the track's own time: beside it, a speed
           would read as the music's. -->
      {#if pacedPlaying && !musicPlaying}
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
  /* Playing: an amber ring, with a lamp and "Playing" in words. Some of the
     lights picked play it: a dashed ring and how many. */
  .tile.partial {
    outline: 1.5px dashed var(--active);
    outline-offset: -1.5px;
  }
  .tile.active {
    box-shadow: inset 0 0 0 2px var(--active);
  }
  .screen :global(.status) {
    align-self: center;
  }
  /* One line kept for the state, so a tile does not grow when it plays. */
  .tile-state {
    display: flex;
    min-height: 18px;
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
      radial-gradient(60% 70% at 30% 50%, oklch(0.72 0.15 55 / 0.55), transparent 70%),
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
    background: linear-gradient(90deg, oklch(0.66 0.16 55), oklch(0.45 0.12 300), oklch(0.6 0.16 250));
    box-shadow:
      0 0 0 3px var(--bezel),
      var(--bezel-shadow);
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
  /* The screen is read in HDR: a plain outlined tag, not amber, since it
     says how the picture is read, not that a light is on. */
  .hdr {
    display: inline-block;
    margin-left: var(--s-1);
    padding: 0 5px;
    border-radius: var(--r-sm);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    line-height: 16px;
    vertical-align: 0.2em;
  }
  .tile-sub {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .tune {
    margin-top: var(--s-3);
  }
  /* Reduced motion: the loops stand still, and a pending choice dims
     instead of breathing. */
  @media (prefers-reduced-motion: reduce) {
    .active .preview {
      animation: none;
    }
    .pending .preview {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
