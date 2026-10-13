<script lang="ts">
  // The running effect reads the room only when it starts. When the plan has
  // moved on since, say so and offer to apply it.
  import { app, needsRestart, restartEffect } from "../state.svelte";
  import Icon from "./Icon.svelte";
</script>

{#if needsRestart()}
  <div class="banner" role="status">
    <Icon name="info" size={16} />
    <p>The room has changed since the ambilight started.</p>
    <button class="btn sm primary" disabled={app.busy} onclick={restartEffect}>
      <Icon name="refresh" size={13} /> Apply now
    </button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-4);
    border-radius: var(--r-lg);
    background: var(--glow-soft);
    color: var(--text);
  }
  .banner :global(svg) {
    color: var(--glow-text);
  }
  p {
    flex: 1;
  }
</style>
