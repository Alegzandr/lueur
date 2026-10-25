<script lang="ts">
  // A slider that knows its factory default: double-click, or the reset
  // button that appears once you drift, snaps back to it.
  import Icon from "./Icon.svelte";
  import { t } from "../i18n";

  interface Props {
    label: string;
    value: number;
    min: number;
    max: number;
    step: number;
    def?: number;
    format?: (v: number) => string;
    /** Words under the two ends of the track, e.g. "Calm" and "Snappy". */
    ends?: [string, string];
    onchange: (v: number) => void;
  }

  let {
    label,
    value,
    min,
    max,
    step,
    def,
    format = (v: number) => String(v),
    ends,
    onchange,
  }: Props = $props();

  const fill = $derived(max > min ? (value - min) / (max - min) : 0);
  const isDefault = $derived(def === undefined || Math.abs(value - def) < step / 2);

  function reset() {
    if (def !== undefined && !isDefault) onchange(def);
  }
</script>

<div class="slider">
  <div class="track-row">
    <input
      type="range"
      {min}
      {max}
      {step}
      {value}
      style="--fill:{fill}"
      aria-label={label}
      aria-valuetext={format(value)}
      oninput={(e) => onchange(+e.currentTarget.value)}
      ondblclick={reset}
    />
    <output class="value num">{format(value)}</output>
    {#if def !== undefined}
      <button
        type="button"
        class="reset"
        class:shown={!isDefault}
        tabindex={isDefault ? -1 : 0}
        title={t("reset_to", { v: format(def) })}
        aria-label={t("labelled", { name: label, what: t("reset_to", { v: format(def) }) })}
        onclick={reset}
      >
        <Icon name="reset" size={13} />
      </button>
    {/if}
  </div>
  {#if ends}
    <div class="ends"><span>{ends[0]}</span><span>{ends[1]}</span></div>
  {/if}
</div>

<style>
  .slider {
    width: 100%;
    min-width: 0;
  }
  .track-row {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .value {
    white-space: nowrap;
    min-width: 6.5ch;
    text-align: right;
    color: var(--text-2);
    font-size: var(--t-sm);
  }
  .reset {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    margin-left: calc(var(--s-2) * -1);
    border-radius: var(--r-sm);
    color: var(--text-3);
    visibility: hidden;
  }
  .reset.shown {
    visibility: visible;
  }
  .reset:hover {
    color: var(--text);
    background: var(--surface-hover);
  }
  .ends {
    display: flex;
    justify-content: space-between;
    margin-top: 2px;
    margin-right: calc(6.5ch + 20px + var(--s-3) + var(--s-1));
    font-size: var(--t-xs);
    color: var(--text-3);
  }
</style>
