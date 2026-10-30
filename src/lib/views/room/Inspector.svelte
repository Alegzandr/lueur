<script lang="ts">
  import {
    app,
    edit,
    bindingFor,
    identifyLight,
    MAX_LIGHT_NAME,
    renameLight,
    setLightEnabled,
    settleName,
  } from "../../state.svelte";
  import { describe, mapLight, EDGE_COLOR } from "../../geometry";
  import { t, pct } from "../../i18n";
  import { bulbLabel } from "../../brands";
  import { defaultConfig, type Light } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import Slider from "../../ui/Slider.svelte";
  import Switch from "../../ui/Switch.svelte";
  import ZoneBadge from "../../ui/ZoneBadge.svelte";
  import Position from "./Position.svelte";
  import { SCREEN_MAX, SCREEN_MIN, cornerName, type Other } from "./plan";

  // What the inspector shows for the light, screen or corner selected. The
  // things set by hand come first, in one card; the exact position, which the
  // plan already gives by dragging, stays folded under them.

  let {
    light,
    other,
    onback,
    onremovelight,
    onremovecorner,
    onset,
  }: {
    /** The light selected, if any; otherwise `other` says what is. */
    light: Light | undefined;
    other: Other;
    /** Back to the list of every object. */
    onback: () => void;
    onremovelight: (id: string) => void;
    onremovecorner: (index: number) => void;
    /** Move the selected object along one axis. */
    onset: (axis: "x" | "y", v: number) => void;
  } = $props();

  const DEF = defaultConfig();
  /** A light's name as its field was entered, to give back if it is left empty. */
  let nameBefore = "";

  const room = $derived(app.config.room);
  const scr = $derived(room.screen);

  const heightLabel = (v: number) =>
    t(v < 0.18 ? "h_floor" : v < 0.42 ? "h_low" : v < 0.58 ? "h_level" : v < 0.82 ? "h_high" : "h_ceiling");
</script>

{#snippet back()}
  <button class="back" onclick={() => onback()}><Icon name="back" size={13} />{t("nav_room")}</button>
{/snippet}

{#snippet exact(p: { x: number; y: number })}
  <details class="exact">
    <summary class="field-head">
      <span>{t("exact_position")}</span>
      <span class="where num" aria-hidden="true">{p.x}, {p.y}</span>
      <span class="fold" aria-hidden="true"><Icon name="chevron" size={12} /></span>
    </summary>
    <div class="exact-body">
      <Position {p} width={scr.width} {onset} legend={false} />
    </div>
  </details>
{/snippet}

{#if light}
  {@const l = light}
  {@const m = mapLight(scr, l)}
  {@const b = bindingFor(l.id)}
  {@render back()}

  <input
    class="name"
    type="text"
    value={l.name}
    aria-label={t("light_name")}
    spellcheck="false"
    data-autofocus
    maxlength={MAX_LIGHT_NAME}
    onfocus={(e) => (nameBefore = e.currentTarget.value)}
    oninput={(e) => renameLight(l.id, e.currentTarget.value)}
    onchange={() => settleName(l.id, nameBefore)}
  />

  <!-- The screen as it sits on a desk, with the part this light mirrors. -->
  <figure class="preview" class:off={!l.enabled} style="--c:{EDGE_COLOR[m.edge]}">
    <svg viewBox="0 0 168 106" role="img" aria-label={t("mirrors")}>
      <rect x="0" y="0" width="168" height="98" rx="7" class="bezel" />
      <rect x="4" y="4" width="160" height="90" rx="4" class="glass" />
      <rect
        x={4 + m.zone.x * 160 + 3}
        y={4 + m.zone.y * 90 + 3}
        width={Math.max(m.zone.w * 160 - 6, 4)}
        height={Math.max(m.zone.h * 90 - 6, 4)}
        rx="3"
        class="zone"
        fill-opacity={0.22 + 0.28 * (1 - m.spread)}
      />
      <rect x="70" y="98" width="28" height="5" class="stand" />
      <rect x="58" y="103" width="52" height="3" rx="1.5" class="stand" />
    </svg>
    <figcaption>
      <ZoneBadge edge={m.edge} off={!l.enabled} labelled />
      <span>{l.enabled ? t(describe(m)) : t("left_out")}</span>
    </figcaption>
  </figure>

  <div class="card">
    <div class="field">
      <span class="field-head" aria-hidden="true">{t("height")}</span>
      <Slider
        label={t("height")}
        value={l.height}
        min={0}
        max={1}
        step={0.01}
        def={0.5}
        format={heightLabel}
        ends={[t("h_floor"), t("h_ceiling")]}
        onchange={(v) =>
          edit((c) => {
            const x = c.room.lights.find((y) => y.id === l.id);
            if (x) x.height = v;
          })}
      />
    </div>
    <div class="field line">
      <span class="field-head">{t("follows")}</span>
      <Switch checked={l.enabled} label={t("follows")} onchange={(v) => setLightEnabled(l.id, v)} />
    </div>
    {#if l.enabled}
      <!-- What the distance to the screen does, said once, under its numbers. -->
      <div class="field facts">
        <dl>
          <div>
            <dt>{t("fact_brightness")}</dt>
            <dd class="num">{pct(m.gain)}</dd>
          </div>
          <div>
            <dt>{t("fact_response")}</dt>
            <dd class="num">{pct(m.responsiveness)}</dd>
          </div>
        </dl>
        <p class="hint">{t("facts_tip")}</p>
      </div>
    {/if}
    {@render exact(l.pos)}
  </div>

  <div class="card bulb" class:missing={!b}>
    {#if b}
      <span class="bulb-mark" aria-hidden="true"><Icon name="bulb" size={15} /></span>
      <!-- The bulb by name; its address only in the tooltip, for whoever needs it. -->
      <span class="bulb-info" title={b.ip}>{bulbLabel(b, l.name)}</span>
      <button class="btn sm" title={t("identify_tip")} onclick={() => identifyLight(l.id)}>
        <Icon name="flash" size={12} />{t("identify")}
      </button>
    {:else}
      <span class="bulb-mark" aria-hidden="true"><span class="lamp"></span></span>
      <span class="bulb-info">{t("no_bulb")}</span>
      <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
    {/if}
  </div>

  <button class="btn plain danger remove" onclick={() => onremovelight(l.id)}>
    <Icon name="trash" size={14} />{t("remove_light")}
  </button>
{:else if other?.kind === "screen"}
  {@render back()}
  <h2 class="name static" tabindex="-1" data-autofocus>{t("plan_screen")}</h2>
  <div class="card">
    <div class="field">
      <span class="field-head" aria-hidden="true">{t("facing")}</span>
      <Slider
        label={t("facing")}
        value={scr.facing_deg}
        min={-180}
        max={180}
        step={1}
        def={DEF.room.screen.facing_deg}
        format={(v) => `${v}°`}
        onchange={(v) => edit((c) => { c.room.screen.facing_deg = v; })}
      />
    </div>
    <div class="field">
      <span class="field-head" aria-hidden="true">{t("width")}</span>
      <Slider
        label={t("width")}
        value={scr.width}
        min={SCREEN_MIN}
        max={SCREEN_MAX}
        step={5}
        def={DEF.room.screen.width}
        format={(v) => String(Math.round(v))}
        onchange={(v) => edit((c) => { c.room.screen.width = v; })}
      />
    </div>
    {@render exact(scr.center)}
  </div>
  <p class="aside-hint">{t("screen_hint")}</p>
{:else if other?.kind === "corner"}
  {@const index = other.index}
  {@const p = room.walls[index]}
  {@render back()}
  <h2 class="name static" tabindex="-1" data-autofocus>{cornerName(index)}</h2>
  {#if p}
    <div class="card">
      <div class="field">
        <Position {p} width={scr.width} {onset} />
      </div>
    </div>
  {/if}
  <button class="btn plain danger remove" onclick={() => onremovecorner(index)}>
    <Icon name="trash" size={14} />{t("remove_corner")}
  </button>
{/if}

<style>
  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin: 0 0 var(--s-2);
    padding: 3px 8px 3px 3px;
    border-radius: var(--r-sm);
    color: var(--text-2);
    font-size: var(--t-sm);
    font-weight: 500;
    transition:
      color var(--dur) var(--ease),
      background var(--dur) var(--ease);
  }
  .back:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  /* What sits on the page's ground, over the room's photograph, carries the
     same halo as the titles. */
  .back,
  .name,
  figcaption {
    text-shadow:
      0 0 2px var(--bg),
      0 0 6px var(--bg),
      0 0 14px var(--bg);
  }

  /* The name is the title, and changes in place: a field only once you
     reach for it. It stays inside the column, whose scroll box would cut
     its focus ring. */
  .name {
    font-size: var(--t-lg);
    font-weight: 700;
    letter-spacing: -0.01em;
    height: 32px;
    margin: 0;
    width: 100%;
    background: transparent;
    box-shadow: none;
    transition: box-shadow var(--dur) var(--ease);
  }
  .name:hover {
    box-shadow: var(--control-edge);
  }
  .name.static {
    margin: 0 0 var(--s-4) var(--s-2);
    height: auto;
    width: auto;
  }

  .preview {
    width: 72%;
    margin: var(--s-3) auto var(--s-4);
  }
  .preview svg {
    display: block;
    width: 100%;
    height: auto;
    overflow: visible;
    filter: drop-shadow(0 6px 10px oklch(0 0 0 / 0.12));
  }
  .bezel,
  .stand {
    fill: var(--bezel);
  }
  .bezel {
    stroke: var(--line-strong);
    stroke-width: 0.75;
  }
  .glass {
    fill: var(--surface-2);
  }
  .zone {
    fill: var(--c);
    stroke: var(--c);
    stroke-width: 1.25;
    transition:
      x 200ms var(--ease),
      y 200ms var(--ease),
      width 200ms var(--ease),
      height 200ms var(--ease);
  }
  .preview.off .zone {
    fill: var(--line-strong);
    stroke: var(--line-strong);
  }
  figcaption {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin-top: var(--s-3);
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--text-2);
  }

  .card {
    padding: 0 var(--s-3);
  }
  .card + .card {
    margin-top: var(--s-3);
  }
  .field {
    padding: var(--s-3) 0;
  }
  .card > * + * {
    border-top: 1px solid var(--separator);
  }
  .field-head {
    display: block;
    margin-bottom: 4px;
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--text-2);
  }
  .field.line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-3);
    min-height: 44px;
    padding: 0;
  }
  .field.line .field-head {
    margin: 0;
    color: var(--text);
    font-size: var(--t-md);
  }

  .facts dl {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-3);
    margin: 0;
  }
  .facts dt {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .facts dd {
    margin: 0;
    font-size: var(--t-lg);
    font-weight: 600;
    color: var(--text);
  }
  .hint,
  .aside-hint {
    margin-top: var(--s-2);
    font-size: var(--t-xs);
    color: var(--text-3);
    line-height: 1.45;
    text-wrap: pretty;
  }
  .aside-hint {
    margin: var(--s-3) var(--s-1) 0;
  }

  /* The exact position, folded: the plan is the way to place things, the
     numbers are there for the keyboard and for fine work. */
  .exact summary {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    min-height: 40px;
    margin: 0;
    list-style: none;
    cursor: default;
  }
  .exact summary::-webkit-details-marker {
    display: none;
  }
  .exact summary > span:first-child {
    flex: 1;
  }
  .exact summary:hover {
    color: var(--text);
  }
  .where {
    color: var(--text-3);
  }
  .exact[open] .where {
    visibility: hidden;
  }
  .fold {
    display: grid;
    color: var(--text-3);
    transform: rotate(90deg);
    transition: transform 220ms var(--ease);
  }
  .exact[open] .fold {
    transform: rotate(-90deg);
  }
  .exact-body {
    padding: 0 0 var(--s-3);
  }

  .bulb {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    min-height: 48px;
    padding: 0 var(--s-2) 0 var(--s-3);
  }
  .bulb-mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    color: var(--text-3);
  }
  .bulb-info {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-size: var(--t-sm);
    font-weight: 500;
  }
  .missing .bulb-info {
    color: var(--glow-text);
  }

  .remove {
    align-self: flex-start;
    margin-top: var(--s-4);
  }
</style>
