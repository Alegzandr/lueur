<script lang="ts">
  // One line of a preferences group: the label on the left, the control on
  // the right. An explanation, when the label needs one, lives in a tooltip
  // behind a small info mark rather than in the layout.
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    label: string;
    tip?: string;
    children: Snippet;
  }

  let { label, tip, children }: Props = $props();
</script>

<div class="row">
  <div class="label">
    {label}
    {#if tip}
      <span class="tip" title={tip} aria-label={tip} role="img"><Icon name="info" size={13} /></span>
    {/if}
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
    padding: 0 var(--s-4);
    min-height: 44px;
  }
  .label {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tip {
    display: inline-grid;
    color: var(--text-3);
    opacity: 0.6;
    transition: opacity var(--dur) var(--ease);
  }
  .row:hover .tip {
    opacity: 1;
  }
  .control {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--s-2);
    width: min(320px, 50%);
  }
</style>
