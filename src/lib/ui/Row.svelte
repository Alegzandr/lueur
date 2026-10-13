<script lang="ts">
  // One line of a preferences group: label and explanation on the left, the
  // control on the right. With `stacked`, the control drops below the label,
  // which suits wide controls such as sliders.
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    hint?: string;
    stacked?: boolean;
    children: Snippet;
  }

  let { label, hint, stacked = false, children }: Props = $props();
</script>

<div class="row" class:stacked>
  <div class="text">
    <div class="label">{label}</div>
    {#if hint}<p class="hint">{hint}</p>{/if}
  </div>
  <div class="control">
    {@render children()}
  </div>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--s-5);
    padding: var(--s-3) var(--s-4);
    min-height: 52px;
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .label {
    font-weight: 500;
  }
  .hint {
    margin-top: 1px;
    font-size: var(--t-sm);
    color: var(--text-3);
    max-width: 56ch;
    text-wrap: pretty;
  }
  .control {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .row:not(.stacked) .control {
    max-width: 50%;
  }
  .stacked {
    flex-direction: column;
    align-items: stretch;
    gap: var(--s-2);
  }
  .stacked .control {
    flex: auto;
  }
</style>
