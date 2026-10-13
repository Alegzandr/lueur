<script lang="ts">
  // The running effect reads the room only when it starts. When the plan has
  // moved on since, say so and offer to apply it.
  import { app, needsRestart, restartEffect } from "../state.svelte";
  import Icon from "./Icon.svelte";
  import { t } from "../i18n";
</script>

{#if needsRestart()}
  <div class="banner" role="status">
    <Icon name="info" size={16} />
    <p>{t("restart_text")}</p>
    <button class="btn sm primary" disabled={app.busy} onclick={restartEffect}>
      {t("apply")}
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
    background: var(--surface);
    border: 1px solid var(--line);
    border-left: 3px solid var(--glow);
    color: var(--text);
  }
  .banner :global(svg) {
    color: var(--glow-text);
  }
  p {
    flex: 1;
  }
</style>
