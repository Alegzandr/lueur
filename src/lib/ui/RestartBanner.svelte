<script lang="ts">
  // The running effect reads the room only when it starts. When the plan has
  // moved on since, say so and offer to apply it.
  import { app, needsRestart, restartEffect } from "../state.svelte";
  import { t } from "../i18n";
</script>

{#if needsRestart()}
  <div class="banner" role="status">
    <span class="lamp" aria-hidden="true"></span>
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
    box-shadow: var(--card-edge);
  }
  p {
    flex: 1;
  }
</style>
