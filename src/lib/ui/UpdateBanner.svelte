<script lang="ts">
  // An update installed in the background applies at the next launch. Say
  // which version and offer to restart into it now.
  import { app } from "../state.svelte";
  import { restartApp } from "../ipc";
  import { t } from "../i18n";

  let restarting = $state(false);

  function restart() {
    restarting = true;
    restartApp().catch(() => (restarting = false));
  }
</script>

{#if app.updateReady}
  <div class="update" role="status">
    <p>{t("update_ready", { version: app.updateReady })}</p>
    <button class="btn sm" disabled={restarting} onclick={restart}>
      {t("update_restart")}
    </button>
  </div>
{/if}

<style>
  .update {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--s-2);
    padding: var(--s-3);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
  }
  p {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    font-size: var(--t-sm);
    color: var(--text-2);
  }
  p::before {
    content: "";
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--glow);
    box-shadow: 0 0 8px var(--glow);
  }
  .btn {
    width: 100%;
  }
</style>
