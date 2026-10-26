<script lang="ts">
  import { app, setSleepTimer } from "../../state.svelte";
  import { t } from "../../i18n";
  import Row from "../../ui/Row.svelte";
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

<!-- The timer is for bedtime: it puts the whole room out, whatever
     "Apply to" picks (the tray sets it too, with no pick to follow). -->
<section class="group">
  <div class="group-head">
    <h2 class="group-title">
      {t("sleep_timer")}<span class="for">{` · ${t("whole_room")}`}</span>
    </h2>
  </div>
  <div class="card rows">
    <Row label={sleepLabel} tip={t("sleep_timer_tip")}>
      {#if app.sleep}
        <button type="button" class="btn sm" use:focusIfFollowing={"cancel"} onclick={cancel}>{t("sleep_cancel")}</button>
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
            <button type="button" class="btn sm num" use:focusIfFollowing={i === 0 ? "presets" : null} onclick={() => start(m)}
              >{m} min</button
            >
          {/each}
          <button type="button" class="btn sm" use:focusIfFollowing={"other"} onclick={() => (typing = true)}
            >{t("sleep_other")}</button
          >
        </div>
      {/if}
    </Row>
    {#if app.sleep}
      <p class="note-line" role="status">
        {t("sleep_keep_on")}{#if staysOpen}{` ${t("sleep_stays_open")}`}{/if}
      </p>
    {/if}
  </div>
</section>

<style>
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
  .note-line {
    padding: var(--s-2) var(--s-4);
    font-size: var(--t-sm);
    color: var(--text-2);
    line-height: 1.4;
  }
</style>
