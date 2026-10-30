<script lang="ts">
  import {
    app,
    adjustLight,
    brightOf,
    identifyLight,
    isOn,
    lightById,
    lightState,
    modeOf,
    moodTargets,
    setBrightness,
    showOnPlan,
  } from "../../state.svelte";
  import { t, pct } from "../../i18n";
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
  import type { Mode } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import Row from "../../ui/Row.svelte";
  import Segmented from "../../ui/Segmented.svelte";
  import Slider from "../../ui/Slider.svelte";

  // The light the lights picked give: one brightness for all of them, and
  // the white or colour of those set by hand.

  let {
    single,
  }: {
    /** The one light picked, if one is. */
    single: string | null;
  } = $props();

  const nameOf = (id: string) => lightById(id)?.name ?? id;

  /** Every light picked with a bulb. */
  const ids = $derived(moodTargets());

  // --- Brightness ---

  /** The lights on say how bright the selection is; with all of them off,
   *  the brightness they come back at. */
  const brightnesses = $derived.by(() => {
    const lit = ids.filter((id) => isOn(id));
    return (lit.length > 0 ? lit : ids).map(brightOf);
  });
  const brightness = $derived(
    brightnesses.length > 0 ? Math.round(brightnesses.reduce((a, b) => a + b, 0) / brightnesses.length) : 100,
  );
  /** The lights picked are not all as bright: the slider shows their average,
   *  and says so. Moving it sets them all to one value. */
  const mixed = $derived(new Set(brightnesses).size > 1);

  /** An effect has one brightness for every light playing it: the screen
   *  its own, the shows another. The lights outside the selection it moves too. */
  const pool = (m: Mode | null) => (m === null ? null : m === "ambilight" ? "screen" : "show");
  const sharedWith = $derived.by(() => {
    const pools = new Set(ids.map((id) => pool(modeOf(id))));
    pools.delete(null);
    if (pools.size === 0) return [];
    return app.config.bindings
      .map((b) => b.light_id)
      .filter((id) => !ids.includes(id) && pools.has(pool(modeOf(id))))
      .map(nameOf);
  });

  // --- White and colour, for the lights set by hand ---

  const byHand = $derived(ids.filter((id) => modeOf(id) === null));
  const tint = $derived(byHand.length > 0 ? lightState(byHand[0]).tint : null);

  const tintOptions = $derived([
    { value: "white" as const, label: t("white") },
    { value: "color" as const, label: t("tint_colour") },
  ]);

  // What the lights were last given in the other mode, so switching between
  // White and Colour comes back to it rather than to a default.
  let lastKelvin = 2700;
  let lastColour: RgbTuple = PALETTE[1];

  function setTintMode(to: "white" | "color") {
    if (!tint || tint.mode === to) return;
    if (tint.mode === "white") lastKelvin = tint.kelvin;
    else lastColour = [tint.r, tint.g, tint.b];
    if (to === "white") setKelvin(lastKelvin);
    else setColour(lastColour);
  }

  function setKelvin(kelvin: number) {
    for (const id of byHand) adjustLight(id, { tint: { mode: "white", kelvin } });
  }

  function setColour([r, g, b]: RgbTuple) {
    for (const id of byHand) adjustLight(id, { tint: { mode: "color", r, g, b } });
  }
</script>

{#if ids.length > 0}
  <!-- Its rows say what they set; the heading over the column, whose. -->
  <section class="group" aria-label={t("adjust")}>
    <div class="card rows">
      <div class="block">
        <Row label={t("brightness")}>
          <Slider
            label={t("brightness")}
            value={brightness}
            min={1}
            max={100}
            step={1}
            format={(v) => (mixed ? t("bright_mixed", { value: pct(v / 100) }) : pct(v / 100))}
            onchange={(v) => setBrightness(ids, v)}
          />
        </Row>
        {#if sharedWith.length > 0}
          <p class="note">{t("bright_shared", { names: sharedWith.join(", ") })}</p>
        {/if}
      </div>

      {#if tint}
        <div class="block">
          <Row label={t("tint")}>
            <Segmented
              label={t("tint")}
              value={tint.mode}
              options={tintOptions}
              onchange={(v) => setTintMode(v)}
            />
          </Row>
          <div class="tint">
            {#if tint.mode === "white"}
              <input
                class="kelvin"
                type="range"
                min={MIN_KELVIN}
                max={MAX_KELVIN}
                step="100"
                value={tint.kelvin}
                style="--warm:{rgbCss(kelvinRgb(MIN_KELVIN))};--mid:{rgbCss(kelvinRgb(4000))};--cool:{rgbCss(kelvinRgb(MAX_KELVIN))}"
                aria-label={t("warmth")}
                aria-valuetext="{tint.kelvin} K"
                oninput={(e) => setKelvin(+e.currentTarget.value)}
              />
              <div class="ends">
                <span>{t("warmth_warm")}</span>
                <span class="num">{tint.kelvin} K</span>
                <span>{t("warmth_cool")}</span>
              </div>
            {:else}
              <div class="palette" role="group" aria-label={t("tint_colour")}>
                {#each PALETTE as c, i}
                  <button
                    class="dot"
                    class:picked={sameRgb(tint, c)}
                    style="--c:{rgbCss(c)}"
                    title={t(`colour_${PALETTE_NAMES[i]}`)}
                    aria-label={t(`colour_${PALETTE_NAMES[i]}`)}
                    aria-pressed={sameRgb(tint, c)}
                    onclick={() => setColour(c)}
                  ></button>
                {/each}
                <label class="dot any" class:picked={!PALETTE.some((c) => sameRgb(tint, c))} title={t("custom_colour")}>
                  <input
                    type="color"
                    value={hexOf(tint)}
                    aria-label={t("custom_colour")}
                    oninput={(e) => {
                      const c = rgbOfHex(e.currentTarget.value);
                      if (c) setColour(c);
                    }}
                  />
                </label>
              </div>
            {/if}
          </div>
        </div>
      {/if}

      {#if single}
        <div class="foot">
          <button class="btn plain sm" onclick={() => identifyLight(single)} title={t("identify_tip")}>
            <Icon name="flash" size={13} />
            {t("identify")}
          </button>
          <button class="btn plain sm" onclick={() => showOnPlan(single)}>
            <Icon name="plan" size={13} />
            {t("show_on_plan")}
          </button>
        </div>
      {/if}
    </div>
  </section>
{/if}

<style>
  .note {
    margin: 0;
    padding: 0 var(--s-4) var(--s-3);
    font-size: var(--t-sm);
    color: var(--text-3);
    line-height: 1.4;
  }
  .tint {
    padding: 0 var(--s-4) var(--s-3);
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
  .kelvin {
    width: 100%;
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
  .foot {
    display: flex;
    gap: var(--s-1);
    padding: var(--s-1) var(--s-2);
  }
</style>
