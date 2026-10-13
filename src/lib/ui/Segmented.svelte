<script lang="ts" generics="T extends string">
  // Mutually exclusive choices. Segments share the width equally, so the
  // selection is a single thumb that slides between them.
  interface Props {
    value: T | null;
    options: { value: T; label: string; title?: string }[];
    label: string;
    onchange: (v: T) => void;
  }

  let { value, options, label, onchange }: Props = $props();

  const index = $derived(options.findIndex((o) => o.value === value));

  function onKey(e: KeyboardEvent) {
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = options[(Math.max(index, 0) + step + options.length) % options.length];
    onchange(next.value);
  }
</script>

<div
  class="seg"
  role="radiogroup"
  aria-label={label}
  tabindex="0"
  style="--n:{options.length};--i:{index}"
  onkeydown={onKey}
>
  {#if index >= 0}<span class="thumb" aria-hidden="true"></span>{/if}
  {#each options as o (o.value)}
    <button
      type="button"
      role="radio"
      tabindex="-1"
      aria-checked={value === o.value}
      class:active={value === o.value}
      title={o.title}
      onclick={() => onchange(o.value)}
    >
      {o.label}
    </button>
  {/each}
</div>

<style>
  .seg {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    padding: 2px;
    border-radius: var(--r-md);
    background: var(--surface-hover);
    box-shadow: inset 0 0 0 0.5px oklch(0 0 0 / 0.08);
  }
  .thumb {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: calc((100% - 4px) / var(--n));
    transform: translateX(calc(var(--i) * 100%));
    border-radius: 5px;
    background: var(--control-bg);
    box-shadow: var(--control-edge), var(--control-hi);
    transition: transform 220ms var(--ease);
  }
  button {
    position: relative;
    height: 24px;
    padding: 0 var(--s-3);
    font-size: var(--t-md);
    font-weight: 500;
    color: var(--text-2);
    white-space: nowrap;
    transition: color var(--dur) var(--ease);
  }
  button:hover:not(.active) {
    color: var(--text);
  }
  button.active {
    color: var(--text);
  }
</style>
