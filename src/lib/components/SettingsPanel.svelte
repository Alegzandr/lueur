<script lang="ts">
  import { onMount } from "svelte";
  import { config, updateConfig, notify, errMsg } from "../stores";
  import * as ipc from "../ipc";
  import { defaultConfig, type BehaviorSettings, type MonitorInfo } from "../types";
  import Slider from "./Slider.svelte";

  const DEF = defaultConfig();

  let monitors = $state<MonitorInfo[]>([]);

  onMount(async () => {
    try {
      monitors = await ipc.listMonitors();
    } catch (e) {
      notify("error", `Could not list monitors: ${errMsg(e)}`);
    }
  });

  function setMonitor(i: number) {
    updateConfig((c) => {
      c.capture.monitor_index = i;
      return c;
    });
  }
  function setFps(v: number) {
    updateConfig((c) => {
      c.capture.target_fps = v;
      return c;
    });
  }
  function setEngine(key: keyof typeof $config.engine, v: number) {
    updateConfig((c) => {
      (c.engine[key] as number) = v;
      return c;
    });
  }
  function setEffect(smooth: boolean) {
    updateConfig((c) => {
      c.transition.effect = smooth ? "smooth" : "sudden";
      return c;
    });
  }
  function setDuration(v: number) {
    updateConfig((c) => {
      c.transition.duration_ms = v;
      return c;
    });
  }
  function setBehavior(key: keyof BehaviorSettings, v: boolean) {
    updateConfig((c) => {
      c.behavior[key] = v;
      return c;
    });
  }
  async function toggleStartup(v: boolean) {
    try {
      await ipc.setRunOnStartup(v);
      setBehavior("run_on_startup", v);
    } catch (e) {
      notify("error", `Could not change the startup setting: ${errMsg(e)}`);
    }
  }

  const pct = (v: number) => `${Math.round(v * 100)}%`;

  const behaviorOptions: { key: keyof BehaviorSettings; name: string; sub: string }[] = [
    { key: "run_on_startup", name: "Launch at login", sub: "Start Lueur when you sign in" },
    { key: "start_minimized", name: "Start in tray", sub: "Open hidden in the system tray" },
    {
      key: "autostart_effect",
      name: "Auto-start ambilight",
      sub: "Begin the effect as soon as Lueur launches",
    },
    {
      key: "minimize_to_tray",
      name: "Close to tray",
      sub: "Closing the window hides it instead of quitting",
    },
  ];
</script>

<div class="settings">
  <section class="group">
    <h2 class="section-title">Capture</h2>
    <div class="fields">
      <label class="field">
        <span class="lbl">Monitor</span>
        <select
          value={$config.capture.monitor_index}
          onchange={(e) => setMonitor(+(e.target as HTMLSelectElement).value)}
        >
          {#each monitors as m}
            <option value={m.index}>
              {m.name} · {m.width}×{m.height}{m.is_primary ? " (primary)" : ""}
            </option>
          {/each}
          {#if monitors.length === 0}
            <option value={0}>Primary display</option>
          {/if}
        </select>
      </label>
      <Slider
        label="Frame rate"
        value={$config.capture.target_fps}
        min={10}
        max={90}
        step={1}
        def={DEF.capture.target_fps}
        format={(v) => `${v} fps`}
        tip="Higher is snappier and heavier on CPU. 40 to 60 works well."
        onchange={setFps}
      />
      <Slider
        label="Sampling"
        value={$config.engine.sample_step}
        min={1}
        max={12}
        step={1}
        def={DEF.engine.sample_step}
        format={(v) => (v === 1 ? "every pixel" : `every ${v}px`)}
        tip="Lower reads more pixels for steadier colour; higher is lighter on CPU."
        onchange={(v) => setEngine("sample_step", v)}
      />
    </div>
  </section>

  <section class="group">
    <h2 class="section-title">Colour</h2>
    <div class="fields">
      <Slider
        label="Smoothing"
        value={$config.engine.smoothing}
        min={0.05}
        max={1}
        step={0.01}
        def={DEF.engine.smoothing}
        format={pct}
        tip="Higher reacts faster; lower is calmer and lags behind the picture."
        onchange={(v) => setEngine("smoothing", v)}
      />
      <Slider
        label="Brightness"
        value={$config.engine.brightness}
        min={0.05}
        max={1}
        step={0.01}
        def={DEF.engine.brightness}
        format={pct}
        onchange={(v) => setEngine("brightness", v)}
      />
      <Slider
        label="Saturation"
        value={$config.engine.saturation}
        min={0}
        max={1}
        step={0.01}
        def={DEF.engine.saturation}
        format={pct}
        onchange={(v) => setEngine("saturation", v)}
      />
      <Slider
        label="Whiten neutral scenes"
        value={$config.engine.white_threshold}
        min={0}
        max={0.5}
        step={0.01}
        def={DEF.engine.white_threshold}
        format={(v) => (v === 0 ? "off" : pct(v))}
        tip="Greyish scenes carry no real colour, so below this the bulbs use a true white instead of guessing a hue, which is what makes them look pink."
        onchange={(v) => setEngine("white_threshold", v)}
      />
      {#if $config.engine.white_threshold > 0}
        <Slider
          label="White warmth"
          value={$config.engine.white_kelvin}
          min={1700}
          max={6500}
          step={100}
          def={DEF.engine.white_kelvin}
          format={(v) => `${v} K`}
          tip="Lower is candle-warm, higher is daylight-cool."
          onchange={(v) => setEngine("white_kelvin", v)}
        />
      {/if}
      <Slider
        label="Black cutoff"
        value={$config.engine.black_threshold}
        min={0}
        max={40}
        step={1}
        def={DEF.engine.black_threshold}
        format={(v) => (v === 0 ? "off" : String(v))}
        tip="Dark scenes below this drop the bulb to its dimmest instead of a muddy glow."
        onchange={(v) => setEngine("black_threshold", v)}
      />
    </div>
  </section>

  <section class="group">
    <h2 class="section-title">Transition</h2>
    <div class="rows">
      <div class="toggle-row">
        <div class="grow">
          <div class="t-name">Smooth transitions</div>
          <p class="t-sub faint">Off jumps instantly: lowest latency, can look steppy</p>
        </div>
        <label class="switch">
          <input
            type="checkbox"
            checked={$config.transition.effect === "smooth"}
            aria-label="Smooth transitions"
            onchange={(e) => setEffect((e.target as HTMLInputElement).checked)}
          />
          <span class="track"></span>
        </label>
      </div>
      {#if $config.transition.effect === "smooth"}
        <Slider
          label="Transition time"
          value={$config.transition.duration_ms}
          min={30}
          max={300}
          step={5}
          def={DEF.transition.duration_ms}
          format={(v) => `${v} ms`}
          onchange={setDuration}
        />
      {/if}
    </div>
  </section>

  <section class="group">
    <h2 class="section-title">Startup and window</h2>
    <div class="rows">
      {#each behaviorOptions as opt}
        <div class="toggle-row">
          <div class="grow">
            <div class="t-name">{opt.name}</div>
            <p class="t-sub faint">{opt.sub}</p>
          </div>
          <label class="switch">
            <input
              type="checkbox"
              checked={$config.behavior[opt.key]}
              aria-label={opt.name}
              onchange={(e) => {
                const v = (e.target as HTMLInputElement).checked;
                if (opt.key === "run_on_startup") toggleStartup(v);
                else setBehavior(opt.key, v);
              }}
            />
            <span class="track"></span>
          </label>
        </div>
      {/each}
    </div>
  </section>

  <section class="group">
    <h2 class="section-title">About</h2>
    <p class="about muted">
      Your setup lives in a <code>lueur-data</code> folder next to the app, so Lueur stays portable.
      Bulbs are reached only over your local network.
    </p>
  </section>
</div>

<style>
  .settings {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: var(--s-6) var(--s-7);
    align-items: start;
    max-width: 1100px;
  }

  .group {
    min-width: 0;
  }
  .group .section-title {
    display: block;
    padding-bottom: var(--s-2);
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--s-4);
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
  }

  .field {
    display: block;
  }
  .lbl {
    display: block;
    font-size: var(--t-2);
    color: var(--text-2);
    margin-bottom: var(--s-2);
  }
  select {
    width: 100%;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: var(--s-4);
  }
  .t-name {
    font-size: var(--t-3);
    font-weight: 500;
  }
  .t-sub {
    font-size: var(--t-1);
    line-height: 1.5;
    margin: 2px 0 0;
  }

  .about {
    font-size: var(--t-2);
    line-height: 1.6;
    max-width: 62ch;
    margin: 0;
  }
  code {
    font-size: var(--t-1);
    background: var(--surface-raised);
    padding: 1px 5px;
    border-radius: var(--r-1);
  }
</style>
