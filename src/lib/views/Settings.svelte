<script lang="ts">
  import { onMount } from "svelte";
  import { app, edit, exportConfig, fail, importConfig } from "../state.svelte";
  import { t } from "../i18n";
  import * as ipc from "../ipc";
  import {
    type Appearance,
    type AudioSource,
    type AudioSources,
    type BehaviorSettings,
    type Language,
    type MonitorInfo,
  } from "../types";
  import { DETECTS_FULLSCREEN } from "../platform";
  import Row from "../ui/Row.svelte";
  import Switch from "../ui/Switch.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Slider from "../ui/Slider.svelte";

  let monitors = $state<MonitorInfo[]>([]);
  let sound = $state<AudioSources | null>(null);
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
    void refreshSound();
    version = await ipc.appVersion().catch(() => "");
  });

  // Applications come and go: the list is asked again whenever the picker
  // is about to open.
  async function refreshSound() {
    try {
      sound = await ipc.listAudioSources();
    } catch (e) {
      fail(e);
    }
  }

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

  // How dark scenes light the room: the engine's brightness curve, read as
  // the nearest of three choices when the file holds another value.
  const CURVES = [
    { value: 1, key: "dark_lit" },
    { value: 1.6, key: "dark_balanced" },
    { value: 2.2, key: "dark_true" },
  ] as const;
  const curveOptions = $derived(CURVES.map((o) => ({ value: String(o.value), label: t(o.key) })));
  const curveValue = $derived.by(() => {
    const at = c.engine.brightness_curve;
    return String(CURVES.reduce((a, b) => (Math.abs(b.value - at) < Math.abs(a.value - at) ? b : a)).value);
  });

  // The sound source picker's values: one string per choice.
  function soundKey(s: AudioSource): string {
    return s.kind === "device" ? `device:${s.id}` : s.kind === "app" ? `app:${s.exe}` : "default";
  }

  const soundDevices = $derived.by(() => {
    const list = (sound?.devices ?? []).map((d) => ({ key: `device:${d.id}`, label: d.name, id: d.id }));
    const a = c.audio;
    // The one picked, unplugged, stays shown as picked.
    if (sound && a.kind === "device" && !list.some((d) => d.id === a.id)) {
      list.push({ key: soundKey(a), label: `${a.name} · ${t("audio_missing")}`, id: a.id });
    }
    return list;
  });

  const soundApps = $derived.by(() => {
    const list = (sound?.apps ?? []).map((p) => ({ key: `app:${p.exe}`, label: p.name, exe: p.exe }));
    const a = c.audio;
    if (sound && a.kind === "app" && !list.some((p) => p.exe.toLowerCase() === a.exe.toLowerCase())) {
      list.push({ key: soundKey(a), label: `${a.name} · ${t("audio_closed")}`, exe: a.exe });
    }
    return list;
  });

  // The default device by name, so it reads apart from the same device
  // picked for good (which stays on it when the default moves).
  const soundDefault = $derived.by(() => {
    const label = t(sound?.loopback ? "audio_default_output" : "audio_default_input");
    const device = sound?.devices.find((d) => d.is_default);
    return device ? `${label} (${device.name})` : label;
  });

  // An executable's case may differ between two sessions: match the one listed.
  const soundValue = $derived.by(() => {
    const a = c.audio;
    if (a.kind !== "app") return soundKey(a);
    return soundApps.find((p) => p.exe.toLowerCase() === a.exe.toLowerCase())?.key ?? soundKey(a);
  });

  function setSound(key: string) {
    let next: AudioSource = { kind: "default" };
    const device = soundDevices.find((d) => d.key === key);
    const app = soundApps.find((p) => p.key === key);
    if (device) next = { kind: "device", id: device.id, name: device.label };
    else if (app) next = { kind: "app", exe: app.exe, name: app.label };
    // A choice gone missing keeps the name it was saved with.
    if (soundKey(next) === soundKey(c.audio)) return;
    edit((x) => (x.audio = next));
  }

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
      <Row label={t("crop_bars")} tip={t("crop_bars_tip")}>
        <Switch
          checked={c.engine.crop_bars}
          label={t("crop_bars")}
          onchange={(v) => edit((x) => (x.engine.crop_bars = v))}
        />
      </Row>
      <Row label={t("dark_scenes")} tip={t("dark_scenes_tip")}>
        <Segmented
          label={t("dark_scenes")}
          value={curveValue}
          options={curveOptions}
          onchange={(v: string) => edit((x) => (x.engine.brightness_curve = +v))}
        />
      </Row>
    </div>
  </section>

  {#if sound}
    <section class="group">
      <div class="group-head"><h2 class="group-title">{t("mode_music")}</h2></div>
      <div class="card rows">
        <Row label={t("sound_source")} tip={sound.loopback ? t("sound_source_tip") : undefined}>
          <select value={soundValue} onfocus={refreshSound} onchange={(e) => setSound(e.currentTarget.value)}>
            <option value="default">{soundDefault}</option>
            {#if soundDevices.length > 0}
              <optgroup label={t(sound.loopback ? "audio_outputs" : "audio_inputs")}>
                {#each soundDevices as d (d.key)}
                  <option value={d.key}>{d.label}</option>
                {/each}
              </optgroup>
            {/if}
            {#if soundApps.length > 0}
              <optgroup label={t("audio_apps")}>
                {#each soundApps as p (p.key)}
                  <option value={p.key}>{p.label}</option>
                {/each}
              </optgroup>
            {/if}
          </select>
        </Row>
        <Row label={t("sound_delay")} tip={t("sound_delay_tip")}>
          <Slider
            label={t("sound_delay")}
            value={c.show.delay_ms}
            min={-150}
            max={500}
            step={10}
            def={0}
            format={(v) => t("ms", { n: v > 0 ? `+${v}` : String(v) })}
            ends={[t("sound_delay_early"), t("sound_delay_late")]}
            onchange={(v) => edit((x) => (x.show.delay_ms = v))}
          />
        </Row>
      </div>
    </section>
  {/if}

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
