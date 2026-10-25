<script lang="ts">
  import { onMount } from "svelte";
  import { app, edit, exportConfig, fail, importConfig } from "../state.svelte";
  import { t } from "../i18n";
  import * as ipc from "../ipc";
  import {
    type Appearance,
    type BehaviorSettings,
    type Language,
    type MonitorInfo,
  } from "../types";
  import { DETECTS_FULLSCREEN } from "../platform";
  import Row from "../ui/Row.svelte";
  import Switch from "../ui/Switch.svelte";
  import Segmented from "../ui/Segmented.svelte";

  let monitors = $state<MonitorInfo[]>([]);
  let version = $state("");
  // False when no keyring is available: the credentials are then kept
  // unencrypted in the config, and the page says so.
  let sealed = $state(true);

  onMount(async () => {
    sealed = await ipc.credentialsSealed().catch(() => false);
    try {
      monitors = await ipc.listMonitors();
    } catch (e) {
      fail(e, "err_displays");
    }
    version = await ipc.appVersion().catch(() => "");
  });

  const c = $derived(app.config);

  async function setStartup(v: boolean) {
    try {
      await ipc.setRunOnStartup(v);
      edit((x) => (x.behavior.run_on_startup = v));
    } catch (e) {
      fail(e);
    }
  }

  function setBehavior(key: keyof BehaviorSettings, v: boolean) {
    if (key === "run_on_startup") void setStartup(v);
    else edit((x) => (x.behavior[key] = v));
  }

  const behaviour = $derived<{ key: keyof BehaviorSettings; label: string }[]>([
    { key: "run_on_startup", label: t("open_login") },
    { key: "start_minimized", label: t("start_tray") },
    { key: "minimize_to_tray", label: t("keep_running") },
  ]);

  const appearanceOptions = $derived<{ value: Appearance; label: string }[]>([
    { value: "system", label: t("automatic") },
    { value: "light", label: t("theme_light") },
    { value: "dark", label: t("theme_dark") },
  ]);

  // Updates per second of the ambilight. A rate set by hand in the config
  // file is offered too, so the control never shows nothing chosen.
  const FPS_CHOICES = [30, 45, 60];
  const fpsOptions = $derived(
    [...new Set([...FPS_CHOICES, c.capture.target_fps])]
      .sort((a, b) => a - b)
      .map((n) => ({ value: String(n), label: String(n) })),
  );

  // Language names are written in their own language, so anyone can find theirs.
  const languageOptions = $derived<{ value: Language; label: string }[]>([
    { value: "system", label: t("automatic") },
    { value: "en", label: "English" },
    { value: "fr", label: "Français" },
  ]);
</script>

<div class="page">
  <header class="page-head">
    <h1 class="page-title">{t("nav_settings")}</h1>
  </header>

  <section class="group">
    <div class="group-head"><h2 class="group-title">{t("mode_ambilight")}</h2></div>
    <div class="card rows">
      {#if DETECTS_FULLSCREEN}
        <Row label={t("auto_mode")} tip={t("auto_mode_tip")}>
          <Switch
            checked={c.behavior.auto_fullscreen}
            label={t("auto_mode")}
            onchange={(v) => edit((x) => (x.behavior.auto_fullscreen = v))}
          />
        </Row>
      {/if}
      {#if monitors.length > 1}
        <Row label={t("display")}>
          <select
            value={c.capture.monitor_index}
            onchange={(e) => edit((x) => (x.capture.monitor_index = +e.currentTarget.value))}
          >
            {#each monitors as m (m.index)}
              <option value={m.index}>{m.name} · {m.width}×{m.height}{m.is_primary ? ` · ${t("display_main")}` : ""}</option>
            {/each}
          </select>
        </Row>
      {/if}
      <Row label={t("frame_rate")} tip={t("frame_rate_tip")}>
        <Segmented
          label={t("frame_rate")}
          value={String(c.capture.target_fps)}
          options={fpsOptions}
          onchange={(v: string) => edit((x) => (x.capture.target_fps = +v))}
        />
      </Row>
    </div>
  </section>

  <section class="group">
    <div class="group-head"><h2 class="group-title">{t("accessibility")}</h2></div>
    <div class="card rows">
      <Row label={t("flash_guard")} tip={t("flash_guard_tip")}>
        <Switch
          checked={c.safety.flash_guard}
          label={t("flash_guard")}
          onchange={(v) => edit((x) => (x.safety.flash_guard = v))}
        />
      </Row>
    </div>
  </section>

  <section class="group">
    <div class="group-head"><h2 class="group-title">{t("application")}</h2></div>
    <div class="card rows">
      <Row label={t("language")}>
        <Segmented
          label={t("language")}
          value={c.interface.language}
          options={languageOptions}
          onchange={(v: Language) => edit((x) => (x.interface.language = v))}
        />
      </Row>
      <Row label={t("appearance")}>
        <Segmented
          label={t("appearance")}
          value={c.interface.appearance}
          options={appearanceOptions}
          onchange={(v: Appearance) => edit((x) => (x.interface.appearance = v))}
        />
      </Row>
      {#each behaviour as b (b.key)}
        <Row label={b.label}>
          <Switch checked={c.behavior[b.key]} label={b.label} onchange={(v) => setBehavior(b.key, v)} />
        </Row>
      {/each}
      <Row label={t("config_file")} tip={t("config_file_tip")}>
        <div class="file-actions">
          <button class="btn sm" disabled={!ipc.inTauri()} onclick={exportConfig}>{t("export")}</button>
          <button class="btn sm" disabled={!ipc.inTauri()} onclick={importConfig}>{t("import")}</button>
        </div>
      </Row>
      <Row label={t("logs")} tip={t("logs_tip")}>
        <button class="btn sm" disabled={!ipc.inTauri()} onclick={() => ipc.openLogsFolder().catch((e) => fail(e))}>
          {t("open_logs")}
        </button>
      </Row>
    </div>
    {#if !sealed}
      <p class="unsealed" role="status">{t("credentials_unsealed")}</p>
    {/if}
  </section>

  <footer class="about">
    <p>Lueur {version}</p>
  </footer>
</div>

<style>
  .unsealed {
    margin-top: var(--s-2);
    padding: 0 var(--s-4);
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .about {
    margin-top: var(--s-6);
    text-align: center;
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .file-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--s-2);
  }
  select {
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
