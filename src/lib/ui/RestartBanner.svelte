<script lang="ts">
  // The running effect reads the room only when it starts. When the plan has
  // moved on since, say so and offer to apply it.
  import { app, needsRestart, restartEffect } from "../state.svelte";
  import { t } from "../i18n";
</script>

{#if needsRestart()}
  <div class="banner" role="status">
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
  .banner::before {
    content: "";
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--glow);
    box-shadow: 0 0 8px var(--glow);
  }
  p {
    flex: 1;
  }
</style>
