<script lang="ts">
  import {
    app,
    adjustLight,
    bindingFor,
    drivenElsewhere,
    isDriven,
    isOn,
    isReconnecting,
    lightState,
    modeOf,
    takeLight,
    toggleLightPower,
  } from "../../state.svelte";
  import { tick } from "svelte";
  import { t, pct } from "../../i18n";
  import { describe, mapLight } from "../../geometry";
  import { PALETTE, tintCss, type RgbTuple } from "../../scenes";
  import type { Light } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import LightPanel from "./LightPanel.svelte";
  import { modeName, modeSwatch, moodName, moodOf } from "./modes";

  // Every light in the room, set by hand: power, brightness, and white or
  // colour unfolded under its row.

  let {
    pickable,
    wholeRoom,
  }: {
    /** The lights that can be picked for effects and moods: those with a bulb. */
    pickable: Light[];
    /** True while effects and moods go to the whole room. */
    wholeRoom: boolean;
  } = $props();

  const lights = $derived(app.config.room.lights);

  /** A light's shortcut to the effects: they start from their tiles only,
   *  so this aims them at the light and brings the tiles into view. */
  async function pickEffectFor(id: string) {
    // A selection that already holds this light is kept: replacing it would
    // drop the other lights picked without a word.
    if (pickable.length > 1 && !app.target.includes(id)) app.target = [id];
    await tick();
    // The first tile that can be pressed, or the section title while all
    // wait on a switch in progress.
    const first =
      document.querySelector<HTMLElement>("#effects .tile:not(:disabled)") ??
      document.querySelector<HTMLElement>("#effects .group-title");
    const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
    first?.scrollIntoView({ block: "center", behavior: still ? "auto" : "smooth" });
    first?.focus({ preventScroll: true });
  }

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

  function showOnPlan(id: string) {
    app.selectedLight = id;
    app.view = "room";
  }
</script>

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
        {@const other = b ? drivenElsewhere(l.id) : null}
        {@const expanded = open === l.id && !!b}
        <div class="light" class:expanded class:targeted={!wholeRoom && app.target.includes(l.id)}>
          <div class="light-row">
            <span
              class="swatch"
              class:lit={b && isOn(l.id)}
              class:off={b && !isOn(l.id)}
              class:driven
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
                {#if !lm && other}
                  <span class="light-sub">{t("elsewhere_sub", { host: other })}</span>
                {:else if mood}<span class="light-sub">{moodName(mood)}</span>{/if}
                <span class="chev" aria-hidden="true"><Icon name="chevron" size={12} /></span>
              {/if}
            </button>

            {#if !b}
              <button class="btn sm" onclick={() => (app.view = "bulbs")}>{t("assign")}</button>
            {:else if lm && other !== null}
              <span class="live-tag held">
                <span class="held-by" title={other ? t("held_tip", { host: other }) : t("held_other_tip")}>
                  {other ? t("held_by", { host: other }) : t("held_by_other")}
                </span>
                <button class="btn sm" title={t("take_over_tip")} onclick={() => takeLight(l.id)}>
                  {t("take_over")}
                </button>
              </span>
            {:else if lm && isReconnecting(l.id)}
              <span class="live-tag" title={t("reconnecting_tip")}>{t("reconnecting")}</span>
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
                  aria-label={t("labelled", { name: l.name, what: t("brightness") })}
                  aria-valuetext={pct(s.bright / 100)}
                  oninput={(e) => adjustLight(l.id, { bright: +e.currentTarget.value })}
                />
                <output class="num">{pct(s.bright / 100)}</output>
              </div>
            {/if}

            {#if b}
              <!-- On shows as a filled button, not only an amber icon. -->
              <button
                class="btn plain icon sm power"
                class:on={isOn(l.id)}
                disabled={app.powerBusy[l.id]}
                title={isOn(l.id) ? t("turn_off") : t("turn_on")}
                aria-label={t("labelled", { name: l.name, what: isOn(l.id) ? t("turn_off") : t("turn_on") })}
                onclick={() => toggleLightPower(l.id)}
              >
                <Icon name="power" size={14} />
              </button>
            {/if}
          </div>

          {#if expanded}
            <LightPanel
              {l}
              ontint={(to) => setTintMode(l.id, to)}
              onpickeffect={() => pickEffectFor(l.id)}
              onshow={() => showOnPlan(l.id)}
            />
          {/if}
        </div>
      {/each}
    </div>
  </section>
{/if}

<style>
  .light.targeted {
    background: var(--select-soft);
    box-shadow: inset 2px 0 0 var(--select);
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
  /* A hairline keeps a near-white bulb visible on a white card. */
  .swatch.lit {
    box-shadow:
      inset 0 0 0 0.5px oklch(0 0 0 / 0.18),
      0 0 0 3px color-mix(in oklab, var(--c) 28%, transparent);
  }
  /* An effect's swatch is a gradient, with no one colour to haze: the halo
     is the amber of something running instead of a grey that reads as off. */
  .swatch.lit.driven {
    box-shadow:
      inset 0 0 0 0.5px oklch(0 0 0 / 0.18),
      0 0 0 3px var(--active-soft);
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
  .live-tag.held {
    justify-content: space-between;
    color: var(--text-3);
  }
  .held-by {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .live-tag.held .btn {
    flex: none;
  }
  .power {
    color: var(--text-3);
  }
  .power.on {
    background: var(--active-soft);
    box-shadow: inset 0 0 0 1px var(--active);
    color: var(--active-text);
  }
</style>
