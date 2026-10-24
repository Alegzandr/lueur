<script lang="ts">
  import { onMount } from "svelte";
  import { app, edit, exportConfig, fail, importConfig } from "../state.svelte";
  import { t, pct } from "../i18n";
  import * as ipc from "../ipc";
  import {
    defaultConfig,
    type Appearance,
    type BehaviorSettings,
    type Language,
    type MonitorInfo,
  } from "../types";
  import Row from "../ui/Row.svelte";
  import Slider from "../ui/Slider.svelte";
  import Switch from "../ui/Switch.svelte";
  import Segmented from "../ui/Segmented.svelte";

  const DEF = defaultConfig();

  let monitors = $state<MonitorInfo[]>([]);
  let version = $state("");

  onMount(async () => {
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
    { key: "autostart_effect", label: t("autostart") },
    { key: "minimize_to_tray", label: t("keep_running") },
  ]);

  const appearanceOptions = $derived<{ value: Appearance; label: string }[]>([
    { value: "system", label: t("automatic") },
    { value: "light", label: t("theme_light") },
    { value: "dark", label: t("theme_dark") },
  ]);

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
    <div class="group-head"><h2 class="group-title">{t("picture")}</h2></div>
    <div class="card rows">
      <Row label={t("display")}>
        <select
          value={c.capture.monitor_index}
          onchange={(e) => edit((x) => (x.capture.monitor_index = +e.currentTarget.value))}
        >
          {#each monitors as m (m.index)}
            <option value={m.index}>{m.name} · {m.width}×{m.height}{m.is_primary ? ` · ${t("display_main")}` : ""}</option>
          {/each}
          {#if monitors.length === 0}
            <option value={0}>{t("main_display")}</option>
          {/if}
        </select>
      </Row>
      <Row label={t("fps")} tip={t("fps_tip")}>
        <Slider
          label={t("fps")}
          value={c.capture.target_fps}
          min={10}
          max={90}
          step={1}
          def={DEF.capture.target_fps}
          format={(v) => `${v} fps`}
          onchange={(v) => edit((x) => (x.capture.target_fps = v))}
        />
      </Row>
      <Row label={t("sampling")} tip={t("sampling_tip")}>
        <Slider
          label={t("sampling")}
          value={c.engine.sample_step}
          min={1}
          max={12}
          step={1}
          def={DEF.engine.sample_step}
          format={(v) => (v === 1 ? t("sampling_all") : t("sampling_n", { n: v }))}
          onchange={(v) => edit((x) => (x.engine.sample_step = v))}
        />
      </Row>
    </div>
  </section>

  <section class="group">
    <div class="group-head"><h2 class="group-title">{t("colour")}</h2></div>
    <div class="card rows">
      <Row label={t("smoothing")} tip={t("smoothing_tip")}>
        <Slider
          label={t("smoothing")}
          value={c.engine.smoothing}
          min={0.05}
          max={1}
          step={0.01}
          def={DEF.engine.smoothing}
          format={pct}
          onchange={(v) => edit((x) => (x.engine.smoothing = v))}
        />
      </Row>
      <Row label={t("brightness")}>
        <Slider
          label={t("brightness")}
          value={c.engine.brightness}
          min={0.05}
          max={1}
          step={0.01}
          def={DEF.engine.brightness}
          format={pct}
          onchange={(v) => edit((x) => (x.engine.brightness = v))}
        />
      </Row>
      <Row label={t("intensity")} tip={t("intensity_tip")}>
        <Slider
          label={t("intensity")}
          value={c.engine.saturation}
          min={0}
          max={1}
          step={0.01}
          def={DEF.engine.saturation}
          format={pct}
          onchange={(v) => edit((x) => (x.engine.saturation = v))}
        />
      </Row>
      <Row label={t("white_grey")} tip={t("white_grey_tip")}>
        <Slider
          label={t("white_grey")}
          value={c.engine.white_threshold}
          min={0}
          max={0.5}
          step={0.01}
          def={DEF.engine.white_threshold}
          format={(v) => (v === 0 ? t("off") : pct(v))}
          onchange={(v) => edit((x) => (x.engine.white_threshold = v))}
        />
      </Row>
      {#if c.engine.white_threshold > 0}
        <Row label={t("warmth")}>
          <Slider
            label={t("warmth")}
            value={c.engine.white_kelvin}
            min={1700}
            max={6500}
            step={100}
            def={DEF.engine.white_kelvin}
            format={(v) => `${v} K`}
            onchange={(v) => edit((x) => (x.engine.white_kelvin = v))}
          />
        </Row>
      {/if}
      <Row label={t("dark_scenes")} tip={t("dark_scenes_tip")}>
        <Slider
          label={t("dark_scenes")}
          value={c.engine.black_threshold}
          min={0}
          max={40}
          step={1}
          def={DEF.engine.black_threshold}
          format={(v) => (v === 0 ? t("off") : String(v))}
          onchange={(v) => edit((x) => (x.engine.black_threshold = v))}
        />
      </Row>
    </div>
  </section>

  <section class="group">
    <div class="group-head"><h2 class="group-title">{t("transitions")}</h2></div>
    <div class="card rows">
      <Row label={t("fade")} tip={t("fade_tip")}>
        <Switch
          checked={c.transition.effect === "smooth"}
          label={t("fade")}
          onchange={(v) => edit((x) => (x.transition.effect = v ? "smooth" : "sudden"))}
        />
      </Row>
      {#if c.transition.effect === "smooth"}
        <Row label={t("fade_len")}>
          <Slider
            label={t("fade_len")}
            value={c.transition.duration_ms}
            min={30}
            max={300}
            step={5}
            def={DEF.transition.duration_ms}
            format={(v) => `${v} ms`}
            onchange={(v) => edit((x) => (x.transition.duration_ms = v))}
          />
        </Row>
      {/if}
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
    </div>
  </section>

  <footer class="about">
    <p>Lueur {version}</p>
  </footer>
</div>

<style>
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
