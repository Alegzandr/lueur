<script lang="ts" generics="T extends string">
  // A row of mutually exclusive choices, in the classic segmented style.
  interface Props {
    value: T | null;
    options: { value: T; label: string; title?: string }[];
    label: string;
    onchange: (v: T) => void;
  }

  let { value, options, label, onchange }: Props = $props();
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each options as o (o.value)}
    <button
      type="button"
      role="radio"
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
    display: inline-flex;
    padding: 2px;
    gap: 2px;
    border-radius: var(--r-md);
    background: var(--surface-2);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  button {
    height: 26px;
    padding: 0 var(--s-3);
    border-radius: 6px;
    font-size: var(--t-md);
    font-weight: 500;
    color: var(--text-2);
    transition:
      background var(--dur) var(--ease),
      color var(--dur) var(--ease);
  }
  button:hover:not(.active) {
    color: var(--text);
  }
  button.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-1);
  }
</style>
