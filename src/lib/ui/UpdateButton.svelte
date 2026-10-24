<script lang="ts">
  // An update installed in the background applies at the next launch. Offer
  // to restart into it now; the tooltip says which version.
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
  <button
    class="btn primary sm update"
    title={t("update_ready", { version: app.updateReady })}
    disabled={restarting}
    onclick={restart}
  >
    <span class="lamp" aria-hidden="true"></span>
    {t("update_restart")}
  </button>
{/if}

<style>
  .lamp {
    --lamp: 6px;
  }
</style>
