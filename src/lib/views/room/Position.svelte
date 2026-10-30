<script lang="ts">
  import { t } from "../../i18n";
  import type { Vec2 } from "../../types";
  import { MARGIN, X_MAX, Y_MAX } from "./plan";

  // The keyboard's way to place things: X and Y fields, kept in the plan's
  // bounds. A value is taken while typing only once it is in bounds, so
  // typing "420" does not stop at 12 on the way; leaving the field clamps it.

  let {
    p,
    width,
    onset,
    legend = true,
  }: {
    /** Where the selected object is now. */
    p: Vec2;
    /** The screen's width, which the tip gives as a sense of scale. */
    width: number;
    /** Move it along one axis (the value is clamped to the plan). */
    onset: (axis: "x" | "y", v: number) => void;
    /** Show the "Position" legend; off where a heading above already says it. */
    legend?: boolean;
  } = $props();

  function onPosInput(axis: "x" | "y", e: Event & { currentTarget: HTMLInputElement }) {
    const v = e.currentTarget.valueAsNumber;
    if (Number.isFinite(v) && v >= MARGIN && v <= (axis === "x" ? X_MAX : Y_MAX)) onset(axis, v);
  }

  function onPosChange(axis: "x" | "y", e: Event & { currentTarget: HTMLInputElement }) {
    const v = e.currentTarget.valueAsNumber;
    if (Number.isFinite(v)) onset(axis, v);
    // Show what was kept: the clamped value, or the old one for no number.
    e.currentTarget.value = String(p[axis]);
  }
</script>

<fieldset class="place">
  <legend class="field-label" class:sr-only={!legend}>{t("position")}</legend>
  <div class="xy">
    {#each ["x", "y"] as const as axis}
      <label class="axis">
        <span aria-hidden="true">{axis.toUpperCase()}</span>
        <input
          type="number"
          class="num"
          min={MARGIN}
          max={axis === "x" ? X_MAX : Y_MAX}
          step="1"
          value={p[axis]}
          aria-label={t(axis === "x" ? "pos_x" : "pos_y")}
          aria-describedby="pos-tip"
          oninput={(e) => onPosInput(axis, e)}
          onchange={(e) => onPosChange(axis, e)}
        />
      </label>
    {/each}
  </div>
  <p class="hint" id="pos-tip">{t("pos_tip", { min: MARGIN, maxX: X_MAX, maxY: Y_MAX, w: width })}</p>
</fieldset>

<style>
  .place {
    margin: 0;
    padding: 0;
    border: none;
    min-width: 0;
  }
  .place legend {
    padding: 0;
  }
  .field-label {
    display: block;
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--text-2);
    margin-bottom: 2px;
  }
  .xy {
    display: flex;
    gap: var(--s-2);
  }
  .axis {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text-2);
  }
  .axis input {
    flex: 1;
    min-width: 0;
    width: 100%;
  }
  .hint {
    margin-top: var(--s-2);
    font-size: var(--t-xs);
    color: var(--text-3);
    line-height: 1.4;
    user-select: text;
  }
</style>
