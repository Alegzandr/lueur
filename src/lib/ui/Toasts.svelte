<script lang="ts">
  import { app, dismiss } from "../state.svelte";
  import Icon from "./Icon.svelte";
  import { t as tr } from "../i18n";
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast" class:error={t.kind === "error"} role={t.kind === "error" ? "alert" : "status"}>
      {#if t.kind === "error"}<span class="ic"><Icon name="alert" size={15} /></span>{/if}
      <p title={t.detail}>{t.msg}</p>
      {#if t.action}
        {@const action = t.action}
        <button
          class="btn sm"
          onclick={() => {
            action.run();
            dismiss(t.id);
          }}>{action.label}</button
        >
      {/if}
      <button class="x" aria-label={tr("dismiss")} onclick={() => dismiss(t.id)}>
        <Icon name="close" size={13} />
      </button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: var(--s-5);
    bottom: var(--s-5);
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: var(--s-2);
    z-index: 50;
    pointer-events: none;
  }
  .toast {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    max-width: 400px;
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-4);
    border-radius: var(--r-lg);
    background: color-mix(in oklab, var(--surface) 88%, transparent);
    backdrop-filter: blur(16px) saturate(1.4);
    box-shadow: var(--float);
    pointer-events: auto;
    animation: rise 160ms var(--ease);
  }
  .ic {
    color: var(--danger);
  }
  p {
    flex: 1;
    text-wrap: pretty;
  }
  .x {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: var(--r-sm);
    color: var(--text-3);
  }
  .x:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
