<script lang="ts">
  // A range slider that always knows its factory default: double-click (or the
  // reset button, which only becomes visible once you drift) snaps back to it,
  // and a tick on the track shows where "default" sits.
  import Icon from "./Icon.svelte";

  interface Props {
    label: string;
    value: number;
    min: number;
    max: number;
    step: number;
    def: number;
    format?: (v: number) => string;
    tip?: string;
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
    tip,
    onchange,
  }: Props = $props();

  const isDefault = $derived(Math.abs(value - def) < step / 2);
  const defFrac = $derived(max > min ? (def - min) / (max - min) : 0);

  function reset() {
    if (!isDefault) onchange(def);
  }
</script>

<div class="field">
  <div class="lbl">
    <span class="grow">{label}</span>
    <b>{format(value)}</b>
    <!-- Kept in the layout at all times so the value never shifts sideways. -->
    <button
      class="reset"
      class:shown={!isDefault}
      type="button"
      tabindex={isDefault ? -1 : 0}
      onclick={reset}
      title="Reset to {format(def)}"
      aria-label="Reset {label} to {format(def)}"
    >
      <Icon name="reset" size={13} />
    </button>
  </div>
  <div class="slider" style="--def:{defFrac}">
    <input
      type="range"
      {min}
      {max}
      {step}
      {value}
      aria-label={label}
      title="Double-click to reset to {format(def)}"
      oninput={(e) => onchange(+(e.target as HTMLInputElement).value)}
      ondblclick={reset}
    />
  </div>
  {#if tip}
    <p class="tip faint">{tip}</p>
  {/if}
</div>

<style>
  .field {
    display: block;
  }
  .lbl {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    font-size: var(--t-2);
    color: var(--text-2);
    margin-bottom: var(--s-2);
  }
  .lbl b {
    color: var(--text);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .reset {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex: none;
    padding: 0;
    border-radius: var(--r-1);
    color: var(--text-3);
    visibility: hidden;
    transition: color var(--dur) var(--ease);
  }
  .reset.shown {
    visibility: visible;
  }
  .reset:hover {
    color: var(--text);
  }

  /* Tick marking the factory default on the track */
  .slider {
    position: relative;
    display: flex;
    align-items: center;
    height: 20px;
  }
  .slider::before {
    content: "";
    position: absolute;
    top: 50%;
    left: calc(7px + (100% - 14px) * var(--def));
    width: 2px;
    height: 8px;
    margin-left: -1px;
    transform: translateY(-50%);
    border-radius: var(--r-full);
    background: var(--border-strong);
    pointer-events: none;
  }
  .slider input[type="range"] {
    position: relative;
  }

  .tip {
    display: block;
    font-size: var(--t-1);
    line-height: 1.5;
    margin: var(--s-2) 0 0;
    max-width: 52ch;
  }
</style>
