<script lang="ts">
  import { app, edit, bindingFor, identifyLight, renameLight, setLightEnabled } from "../../state.svelte";
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

  // What the inspector shows for the light, screen or corner selected.

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

  const room = $derived(app.config.room);
  const scr = $derived(room.screen);

  const heightLabel = (v: number) =>
    t(v < 0.18 ? "h_floor" : v < 0.42 ? "h_low" : v < 0.58 ? "h_level" : v < 0.82 ? "h_high" : "h_ceiling");
</script>

{#if light}
  {@const l = light}
  {@const m = mapLight(scr, l)}
  {@const b = bindingFor(l.id)}
  <button class="back" onclick={() => onback()}><Icon name="back" size={13} /> {t("all_objects")}</button>

  <input
    class="name"
    type="text"
    value={l.name}
    aria-label={t("light_name")}
    spellcheck="false"
    data-autofocus
    oninput={(e) => renameLight(l.id, e.currentTarget.value)}
  />

  <figure class="preview" style="--c:{EDGE_COLOR[m.edge]}">
    <svg viewBox="0 0 160 90" role="img" aria-label={t("mirrors")}>
      <rect x="0.5" y="0.5" width="159" height="89" rx="5" class="preview-bg" />
      <rect
        x={m.zone.x * 160 + 3}
        y={m.zone.y * 90 + 3}
        width={Math.max(m.zone.w * 160 - 6, 4)}
        height={Math.max(m.zone.h * 90 - 6, 4)}
        rx="3"
        class="preview-zone"
        fill-opacity={0.2 + 0.25 * (1 - m.spread)}
      />
    </svg>
    <figcaption><ZoneBadge edge={m.edge} off={!l.enabled} labelled />{t(describe(m))}</figcaption>
  </figure>

  <div class="fields">
    <Position p={l.pos} width={scr.width} {onset} />
    <div class="field">
      <span class="field-label">{t("height")}</span>
      <Slider
        label={t("height")}
        value={l.height}
        min={0}
        max={1}
        step={0.01}
        def={0.5}
        format={heightLabel}
        onchange={(v) =>
          edit((c) => {
            const x = c.room.lights.find((y) => y.id === l.id);
            if (x) x.height = v;
          })}
      />
    </div>
    <div class="field inline">
      <span class="field-label">{t("follows")}</span>
      <Switch checked={l.enabled} label={t("follows")} onchange={(v) => setLightEnabled(l.id, v)} />
    </div>
    <div class="field inline" title={t("facts_tip")}>
      <span class="field-label">{t("fact_brightness")}</span>
      <span class="fact num">{pct(m.gain)}</span>
    </div>
    <div class="field inline" title={t("facts_tip")}>
      <span class="field-label">{t("fact_response")}</span>
      <span class="fact num">{pct(m.responsiveness)}</span>
    </div>
  </div>

  <div class="bulb-box">
    <Icon name="bulb" size={15} />
    {#if b}
      <!-- The bulb by name; its address only in the tooltip, for whoever needs it. -->
      <span class="bulb-info" title={b.ip}>{bulbLabel(b, l.name)}</span>
      <button class="btn sm" title={t("identify_tip")} onclick={() => identifyLight(l.id)}>{t("identify")}</button>
    {:else}
      <span class="bulb-info warn">{t("no_bulb")}</span>
      <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
    {/if}
  </div>

  <button class="btn plain danger remove" onclick={() => onremovelight(l.id)}>{t("remove_light")}</button>
{:else if other?.kind === "screen"}
  <button class="back" onclick={() => onback()}><Icon name="back" size={13} /> {t("all_objects")}</button>
  <h2 class="name static" tabindex="-1" data-autofocus>{t("plan_screen")}</h2>
  <div class="fields">
    <Position p={scr.center} width={scr.width} {onset} />
    <div class="field">
      <span class="field-label">{t("facing")}</span>
      <Slider
        label={t("facing")}
        value={scr.facing_deg}
        min={-180}
        max={180}
        step={1}
        def={DEF.room.screen.facing_deg}
        format={(v) => `${v}°`}
        onchange={(v) => edit((c) => (c.room.screen.facing_deg = v))}
      />
    </div>
    <div class="field">
      <span class="field-label">{t("width")}</span>
      <Slider
        label={t("width")}
        value={scr.width}
        min={SCREEN_MIN}
        max={SCREEN_MAX}
        step={5}
        def={DEF.room.screen.width}
        format={(v) => String(Math.round(v))}
        onchange={(v) => edit((c) => (c.room.screen.width = v))}
      />
    </div>
  </div>
{:else if other?.kind === "corner"}
  {@const index = other.index}
  {@const p = room.walls[index]}
  <button class="back" onclick={() => onback()}><Icon name="back" size={13} /> {t("all_objects")}</button>
  <h2 class="name static" tabindex="-1" data-autofocus>{cornerName(index)}</h2>
  {#if p}
    <div class="fields">
      <Position {p} width={scr.width} {onset} />
    </div>
  {/if}
  <button class="btn plain danger remove" onclick={() => onremovecorner(index)}>{t("remove_corner")}</button>
{/if}

<style>
  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin: 0 0 var(--s-3) -6px;
    padding: 2px 6px 2px 2px;
    border-radius: var(--r-sm);
    color: var(--text-3);
    font-size: var(--t-sm);
  }
  .back:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .name {
    font-size: var(--t-lg);
    font-weight: 600;
    height: 32px;
    margin-left: -8px;
    width: calc(100% + 8px);
    background: transparent;
    box-shadow: none;
  }
  .name:hover {
    box-shadow: var(--control-edge);
  }
  .name.static {
    margin: 0 0 var(--s-4);
    height: auto;
    width: auto;
  }

  .preview {
    margin: var(--s-3) 0 var(--s-5);
  }
  .preview svg {
    display: block;
    width: 100%;
    height: auto;
    border-radius: 6px;
    box-shadow: 0 0 0 3px var(--plan-wall);
  }
  .preview-bg {
    fill: var(--sunken);
    stroke: none;
  }
  .preview-zone {
    fill: var(--c);
    stroke: var(--c);
    stroke-width: 1.25;
    transition:
      x 200ms var(--ease),
      y 200ms var(--ease),
      width 200ms var(--ease),
      height 200ms var(--ease);
  }
  figcaption {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin-top: var(--s-3);
    font-size: var(--t-sm);
    color: var(--text-2);
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
    padding: var(--s-4) 0;
    border-top: 1px solid var(--line);
  }
  .field-label {
    display: block;
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--text-2);
    margin-bottom: 2px;
  }
  .field.inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .field.inline .field-label {
    margin: 0;
  }
  .fact {
    font-size: var(--t-sm);
    color: var(--text-2);
  }

  .bulb-box {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-3);
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--card-edge);
    color: var(--text-3);
  }
  .bulb-info {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-2);
    font-size: var(--t-sm);
  }
  .warn {
    color: var(--glow-text);
  }
  .remove {
    align-self: flex-start;
    margin-top: var(--s-3);
    margin-left: -8px;
  }
</style>
