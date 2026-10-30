<script lang="ts">
  // One line of a preferences group: the label on the left, the control on
  // the right. An explanation, when the label needs one, lives behind a small
  // info mark rather than in the layout. It opens on hover and on keyboard
  // focus, and Escape closes it.
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import { t } from "../i18n";

  interface Props {
    label: string;
    tip?: string;
    children: Snippet;
  }

  let { label, tip, children }: Props = $props();

  let shown = $state(false);
  const tipId = $props.id();
</script>

<div class="row">
  <div class="label">
    {label}
    {#if tip}
      <span class="tip-wrap">
        <button
          type="button"
          class="tip"
          aria-label={t("about", { name: label })}
          aria-describedby={tipId}
          onpointerenter={() => (shown = true)}
          onpointerleave={() => (shown = false)}
          onfocus={() => (shown = true)}
          onblur={() => (shown = false)}
          onclick={() => (shown = !shown)}
          onkeydown={(e) => {
            if (e.key === "Escape" && shown) {
              e.stopPropagation();
              shown = false;
            }
          }}
        >
          <Icon name="info" size={13} />
        </button>
        <span id={tipId} role="tooltip" class="bubble" class:shown>{tip}</span>
      </span>
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
  .tip-wrap {
    position: relative;
    display: inline-grid;
  }
  .tip {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: var(--text-3);
    transition: color var(--dur) var(--ease);
  }
  .tip:hover,
  .tip:focus-visible {
    color: var(--text);
  }
  .bubble {
    position: absolute;
    left: -10px;
    bottom: calc(100% + 6px);
    z-index: 20;
    width: max-content;
    max-width: 280px;
    padding: 6px 10px;
    border-radius: var(--r-md);
    background: var(--surface);
    box-shadow: var(--float);
    color: var(--text-2);
    font-size: var(--t-sm);
    line-height: 1.4;
    white-space: pre-line;
    text-wrap: pretty;
    pointer-events: none;
    opacity: 0;
    visibility: hidden;
    transform: translateY(2px);
    transition:
      opacity var(--dur) var(--ease),
      transform var(--dur) var(--ease),
      visibility 0s linear var(--dur);
  }
  .bubble.shown {
    opacity: 1;
    visibility: visible;
    transform: none;
    transition:
      opacity var(--dur) var(--ease),
      transform var(--dur) var(--ease);
  }
  .control {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--s-2);
    width: min(320px, 50%);
    /* Never narrower than what it holds: three long French choices take a
       few pixels from the label rather than spill out of their control. */
    min-width: max-content;
  }
</style>
