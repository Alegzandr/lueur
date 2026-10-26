<script lang="ts">
  import {
    app,
    adjustLight,
    drivenElsewhere,
    identifyLight,
    isDriven,
    lightState,
    modeOf,
    setLightMode,
  } from "../../state.svelte";
  import { t } from "../../i18n";
  import {
    MAX_KELVIN,
    MIN_KELVIN,
    PALETTE,
    PALETTE_NAMES,
    hexOf,
    kelvinRgb,
    rgbCss,
    rgbOfHex,
    sameRgb,
    type RgbTuple,
  } from "../../scenes";
  import type { Light } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import Segmented from "../../ui/Segmented.svelte";
  import { modeName } from "./modes";

  // A bound light's white and colour controls, unfolded under its row; or,
  // while an effect drives it, a way to take it back by hand.

  let {
    l,
    ontint,
    onpickeffect,
    onshow,
  }: {
    l: Light;
    /** Switch between white and colour. */
    ontint: (to: "white" | "color") => void;
    /** Aim the effects at this light. */
    onpickeffect: () => void;
    /** Show this light on the plan. */
    onshow: () => void;
  } = $props();

  const s = $derived(lightState(l.id));
  const driven = $derived(isDriven(l.id));
  const lm = $derived(modeOf(l.id));
  const other = $derived(drivenElsewhere(l.id));

  const tintOptions = $derived([
    { value: "white" as const, label: t("white") },
    { value: "color" as const, label: t("tint_colour") },
  ]);

  function setColour([r, g, b]: RgbTuple) {
    adjustLight(l.id, { tint: { mode: "color", r, g, b } });
  }
</script>

<div class="panel">
  {#if driven}
    <div class="plays">
      <p class="hint">{t("driven_hint", { mode: lm ? modeName(lm) : t("mode_mixed") })}</p>
      {#if other !== null}
        <p class="hint">{other ? t("held_tip", { host: other }) : t("held_other_tip")}</p>
      {/if}
      <button class="btn sm" disabled={app.busy || app.modeBusy[l.id]} onclick={() => setLightMode(l.id, null)}>
        {t("set_by_hand")}
      </button>
    </div>
  {:else}
    <div class="tint-mode">
      <Segmented
        label="{l.name}: {t('white')} / {t('tint_colour')}"
        value={s.tint.mode}
        options={tintOptions}
        onchange={(v) => ontint(v)}
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
          aria-label={t("labelled", { name: l.name, what: t("warmth") })}
          aria-valuetext="{s.tint.kelvin} K"
          oninput={(e) => adjustLight(l.id, { tint: { mode: "white", kelvin: +e.currentTarget.value } })}
        />
        <div class="ends">
          <span>{t("warmth_warm")}</span>
          <span>{t("warmth_cool")}</span>
        </div>
      </div>
    {:else}
      <div class="palette" role="group" aria-label={t("labelled", { name: l.name, what: t("tint_colour") })}>
        {#each PALETTE as c, i}
          <button
            class="dot"
            class:picked={sameRgb(s.tint, c)}
            style="--c:{rgbCss(c)}"
            title={t(`colour_${PALETTE_NAMES[i]}`)}
            aria-label={t(`colour_${PALETTE_NAMES[i]}`)}
            aria-pressed={sameRgb(s.tint, c)}
            onclick={() => setColour(c)}
          ></button>
        {/each}
        <label class="dot any" class:picked={!PALETTE.some((c) => sameRgb(s.tint, c))} title={t("custom_colour")}>
          <input
            type="color"
            value={hexOf(s.tint)}
            aria-label={t("labelled", { name: l.name, what: t("custom_colour") })}
            oninput={(e) => {
              const c = rgbOfHex(e.currentTarget.value);
              if (c) setColour(c);
            }}
          />
        </label>
      </div>
    {/if}
  {/if}
  <div class="panel-foot">
    <button class="btn plain sm" title={t("pick_effect_tip")} onclick={() => onpickeffect()}>
      <Icon name="screen" size={13} />
      {driven ? t("change_effect") : t("pick_effect")}
    </button>
    <button class="btn plain sm" onclick={() => identifyLight(l.id)} title={t("identify_tip")}>
      <Icon name="flash" size={13} />
      {t("identify")}
    </button>
    <button class="btn plain sm" onclick={() => onshow()}>
      <Icon name="plan" size={13} />
      {t("show_on_plan")}
    </button>
  </div>
</div>

<style>
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
  .plays {
    display: flex;
    align-items: center;
    gap: var(--s-3);
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
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.12);
  }
  .kelvin::-webkit-slider-thumb {
    margin-top: -5px;
  }
  .kelvin::-moz-range-track {
    height: 8px;
    background: linear-gradient(to right, var(--warm), var(--mid), var(--cool));
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.12);
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
</style>
