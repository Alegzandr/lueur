<script lang="ts">
  import { app, setSleepTimer } from "../../state.svelte";
  import { t } from "../../i18n";
  import Segmented from "../../ui/Segmented.svelte";
  import Row from "../../ui/Row.svelte";

  /** The timers offered, in minutes; the tray offers the same. */
  const SLEEP_PRESETS = [15, 30, 60, 90];
  const sleepOptions = $derived([
    { value: "off", label: t("sleep_off") },
    ...SLEEP_PRESETS.map((m) => ({ value: String(m), label: `${m} min` })),
  ]);

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
      <Segmented
        label={t("sleep_timer")}
        value={app.sleep ? String(app.sleep.status.minutes) : "off"}
        options={sleepOptions}
        onchange={(v) => setSleepTimer(v === "off" ? null : +v)}
      />
    </Row>
  </div>
</section>
