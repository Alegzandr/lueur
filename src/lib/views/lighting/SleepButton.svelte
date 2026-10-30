<script lang="ts">
  import { app, setSleepTimer } from "../../state.svelte";
  import { t } from "../../i18n";
  import Icon from "../../ui/Icon.svelte";

  /** The durations offered at a click, in minutes. Any other is typed. */
  const SLEEP_PRESETS = [15, 30, 60];
  /** The longest timer the backend takes (`sleep::MAX_MINUTES`). */
  const MAX_MINUTES = 240;

  let now = $state(Date.now());
  $effect(() => {
    if (!app.sleep) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  /** Seconds until the lights go off. */
  const sleepLeft = $derived(
    // `now` may predate the timer until the next tick.
    app.sleep ? Math.max(0, app.sleep.status.left - Math.max(0, now - app.sleep.at) / 1000) : null,
  );
  const sleepLabel = $derived(
    sleepLeft === null
      ? t("sleep_in")
      : sleepLeft <= 60
        ? t("sleep_fading")
        : t("sleep_left", { n: Math.ceil(sleepLeft / 60) }),
  );

  // "Other…" swaps the presets for a field, until a timer starts or the
  // field is closed (its × or Escape).
  let typing = $state(false);
  let typed = $state<number | null>(45);
  const typedMinutes = $derived(
    typed !== null && Number.isInteger(typed) && typed >= 1 && typed <= MAX_MINUTES ? typed : null,
  );

  // Where the keyboard goes once the control under it has been swapped out.
  // Only after a click or key here: a timer set from the tray steals nothing.
  let follow: "cancel" | "presets" | "other" | null = null;

  function start(minutes: number) {
    typing = false;
    follow = "cancel";
    void setSleepTimer(minutes);
  }

  function closeField() {
    typing = false;
    follow = "other";
  }

  function cancel() {
    follow = "presets";
    void setSleepTimer(null);
  }

  // Lueur keeps the computer from going to sleep by itself while a timer
  // runs, and stays open even when closing the window would quit it (see
  // `awake.rs` and `lib.rs`). Shutting the computer down still stops it.
  const staysOpen = $derived(!app.config.behavior.minimize_to_tray);

  let open = $state(false);
  let root = $state<HTMLElement>();
  let trigger = $state<HTMLButtonElement>();
  const panelId = $props.id();

  /** Close the panel and give the keyboard back to the clock. */
  function close() {
    open = false;
    typing = false;
    trigger?.focus();
  }

  // The panel's first control takes the keyboard as it opens, and only
  // then: a timer set from the tray while it is open steals nothing.
  let opening = false;

  function toggle() {
    opening = !open;
    open = !open;
  }

  function focusOnOpen(node: HTMLElement, first = true) {
    if (first && opening) {
      opening = false;
      node.focus();
    }
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function focusIfFollowing(node: HTMLElement, which: typeof follow) {
    if (which && follow === which) {
      node.focus();
      follow = null;
    }
  }
</script>

<!-- The timer is for bedtime: it puts the whole room out, whatever is
     picked (the tray sets it too, with no pick to follow). An occasional
     tool, so it waits behind a clock in the page's head. -->
<div class="sleep" bind:this={root}>
  <button
    type="button"
    class="btn plain sm trigger"
    aria-expanded={open}
    aria-controls={panelId}
    title={t("sleep_timer")}
    aria-label={sleepLeft === null ? t("sleep_timer") : `${t("sleep_timer")}: ${sleepLabel}`}
    bind:this={trigger}
    onclick={toggle}
  >
    <Icon name="clock" size={15} />
    {#if sleepLeft !== null}
      <span class="num left" aria-hidden="true">{sleepLeft <= 60 ? "<1" : Math.ceil(sleepLeft / 60)} min</span>
    {/if}
  </button>

  {#if open}
    <div
      class="panel"
      id={panelId}
      role="dialog"
      aria-label={t("sleep_timer")}
      tabindex="-1"
      onkeydown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          close();
        }
      }}
    >
      <p class="title">{sleepLabel}</p>
      {#if app.sleep}
        <button type="button" class="btn sm" use:focusIfFollowing={"cancel"} use:focusOnOpen onclick={cancel}
          >{t("sleep_cancel")}</button
        >
      {:else if typing}
        <form
          class="custom"
          onsubmit={(e) => {
            e.preventDefault();
            if (typedMinutes !== null) start(typedMinutes);
          }}
        >
          <input
            type="number"
            class="num"
            min="1"
            max={MAX_MINUTES}
            step="1"
            bind:value={typed}
            aria-label={t("sleep_minutes")}
            use:focusOnMount
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.stopPropagation();
                closeField();
              }
            }}
          />
          <span class="unit" aria-hidden="true">min</span>
          <button type="submit" class="btn sm" disabled={typedMinutes === null}>{t("sleep_start")}</button>
          <button
            type="button"
            class="btn sm plain icon"
            aria-label={t("sleep_back")}
            title={t("sleep_back")}
            onclick={closeField}
          >
            <Icon name="close" size={14} />
          </button>
        </form>
      {:else}
        <div class="btn-group" role="group" aria-label={t("sleep_timer")}>
          {#each SLEEP_PRESETS as m, i (m)}
            <button
              type="button"
              class="btn sm num"
              use:focusIfFollowing={i === 0 ? "presets" : null}
              use:focusOnOpen={i === 0}
              onclick={() => start(m)}>{m} min</button
            >
          {/each}
          <button type="button" class="btn sm" use:focusIfFollowing={"other"} onclick={() => (typing = true)}
            >{t("sleep_other")}</button
          >
        </div>
      {/if}
      <p class="note" role={app.sleep ? "status" : undefined}>
        {#if app.sleep}
          {t("sleep_keep_on")}{#if staysOpen}{` ${t("sleep_stays_open")}`}{/if}
        {:else}
          {t("sleep_timer_tip")}
        {/if}
      </p>
    </div>
  {/if}
</div>

<svelte:window
  onpointerdown={(e) => {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }}
/>

<style>
  .sleep {
    position: relative;
  }
  .trigger {
    gap: 5px;
    color: var(--text-2);
  }
  .left {
    font-size: var(--t-sm);
    color: var(--text);
  }
  .panel {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    width: 300px;
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
    padding: var(--s-3) var(--s-4) var(--s-4);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--float);
    animation: unfold 140ms var(--ease);
  }
  @keyframes unfold {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .panel {
      animation: none;
    }
  }
  .title {
    margin: 0;
    font-weight: 600;
  }
  /* The panel stacks its parts; a button or a group of them keeps its own
     width rather than stretching its edge across the panel. */
  .panel > .btn,
  .panel > .btn-group {
    align-self: flex-start;
  }
  .custom {
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .custom input {
    width: 64px;
    height: 24px;
  }
  .unit {
    color: var(--text-2);
    font-size: var(--t-sm);
  }
  .note {
    margin: 0;
    font-size: var(--t-sm);
    color: var(--text-3);
    line-height: 1.4;
  }
</style>
