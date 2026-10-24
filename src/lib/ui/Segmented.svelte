<script lang="ts" generics="T extends string">
  // Mutually exclusive choices. Segments share the width equally, so the
  // selection is a single thumb that slides between them. A radio group:
  // Tab lands on the chosen segment, the arrows move the choice.
  interface Props {
    value: T | null;
    options: { value: T; label: string; title?: string }[];
    label: string;
    onchange: (v: T) => void;
  }

  let { value, options, label, onchange }: Props = $props();

  const index = $derived(options.findIndex((o) => o.value === value));

  function onKey(e: KeyboardEvent & { currentTarget: HTMLElement }) {
    const last = options.length - 1;
    const from = Math.max(index, 0);
    let to: number;
    switch (e.key) {
      case "ArrowRight":
      case "ArrowDown":
        to = from === last ? 0 : from + 1;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        to = from === 0 ? last : from - 1;
        break;
      case "Home":
        to = 0;
        break;
      case "End":
        to = last;
        break;
      default:
        return;
    }
    e.preventDefault();
    onchange(options[to].value);
    e.currentTarget.querySelectorAll<HTMLButtonElement>("button")[to]?.focus();
  }
</script>

<div
  class="seg"
  role="radiogroup"
  aria-label={label}
  tabindex="-1"
  style="--n:{options.length};--i:{index}"
  onkeydown={onKey}
>
  {#if index >= 0}<span class="thumb" aria-hidden="true"></span>{/if}
  {#each options as o, i (o.value)}
    <button
      type="button"
      role="radio"
      tabindex={i === Math.max(index, 0) ? 0 : -1}
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
    box-shadow: inset 0 0 0 0.5px var(--line);
  }
  .seg:focus {
    outline: none;
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
    border-radius: 5px;
    font-size: var(--t-md);
    font-weight: 500;
    color: var(--text-2);
    white-space: nowrap;
    transition: color var(--dur) var(--ease);
  }
  button:focus-visible {
    outline-offset: 0;
  }
  button:hover:not(.active) {
    color: var(--text);
  }
  button.active {
    color: var(--text);
  }
</style>
