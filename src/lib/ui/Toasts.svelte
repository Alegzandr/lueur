<script lang="ts">
  import { app, dismiss } from "../state.svelte";
  import Icon from "./Icon.svelte";
</script>

<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast" class:error={t.kind === "error"} role={t.kind === "error" ? "alert" : "status"}>
      <span class="ic"><Icon name={t.kind === "error" ? "alert" : "check"} size={15} /></span>
      <p>{t.msg}</p>
      <button class="x" aria-label="Dismiss" onclick={() => dismiss(t.id)}>
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
    align-items: flex-start;
    gap: var(--s-2);
    max-width: 380px;
    padding: var(--s-3) var(--s-3) var(--s-3) var(--s-4);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--shadow-2);
    pointer-events: auto;
    animation: rise 220ms var(--ease);
  }
  .ic {
    margin-top: 2px;
    color: var(--ok);
  }
  .error .ic {
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
