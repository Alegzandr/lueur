<script lang="ts">
  import { onMount } from "svelte";
  import { config, updateConfig, notify, errMsg } from "../stores";
  import * as ipc from "../ipc";
  import type { MonitorInfo } from "../types";

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
  function setBehavior(key: keyof typeof $config.behavior, v: boolean) {
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
      notify("error", `Couldn't change startup setting: ${errMsg(e)}`);
    }
  }

  const pct = (v: number) => `${Math.round(v * 100)}%`;
</script>

<div class="settings">
  <div class="card pad">
    <div class="section-title">Capture</div>
    <label class="field">
      <span class="lbl">Monitor</span>
      <select value={$config.capture.monitor_index} onchange={(e) => setMonitor(+(e.target as HTMLSelectElement).value)}>
        {#each monitors as m}
          <option value={m.index}>
            {m.name} — {m.width}×{m.height}{m.is_primary ? " (primary)" : ""}
          </option>
        {/each}
        {#if monitors.length === 0}
          <option value={0}>Primary display</option>
        {/if}
      </select>
    </label>
    <label class="field">
      <span class="lbl">Frame rate <b>{$config.capture.target_fps} fps</b></span>
      <input type="range" min="10" max="90" step="1" value={$config.capture.target_fps}
        oninput={(e) => setFps(+(e.target as HTMLInputElement).value)} />
      <span class="tip faint">Higher = snappier, but heavier on CPU. 40–60 is the sweet spot.</span>
    </label>
  </div>

  <div class="card pad">
    <div class="section-title">Look &amp; feel</div>
    <label class="field">
      <span class="lbl">Smoothing <b>{pct($config.engine.smoothing)}</b></span>
      <input type="range" min="0.05" max="1" step="0.01" value={$config.engine.smoothing}
        oninput={(e) => setEngine("smoothing", +(e.target as HTMLInputElement).value)} />
      <span class="tip faint">Higher reacts faster (less lag); lower is dreamy and calm.</span>
    </label>
    <label class="field">
      <span class="lbl">Brightness <b>{pct($config.engine.brightness)}</b></span>
      <input type="range" min="0.05" max="1" step="0.01" value={$config.engine.brightness}
        oninput={(e) => setEngine("brightness", +(e.target as HTMLInputElement).value)} />
    </label>
    <label class="field">
      <span class="lbl">Saturation boost <b>{pct($config.engine.saturation)}</b></span>
      <input type="range" min="0" max="1" step="0.01" value={$config.engine.saturation}
        oninput={(e) => setEngine("saturation", +(e.target as HTMLInputElement).value)} />
    </label>
    <label class="field">
      <span class="lbl">Black cutoff <b>{$config.engine.black_threshold}</b></span>
      <input type="range" min="0" max="40" step="1" value={$config.engine.black_threshold}
        oninput={(e) => setEngine("black_threshold", +(e.target as HTMLInputElement).value)} />
      <span class="tip faint">Dark scenes below this fade the bulb fully off instead of a muddy glow.</span>
    </label>
    <label class="field">
      <span class="lbl">Quality vs. CPU <b>step {$config.engine.sample_step}px</b></span>
      <input type="range" min="1" max="12" step="1" value={$config.engine.sample_step}
        oninput={(e) => setEngine("sample_step", +(e.target as HTMLInputElement).value)} />
      <span class="tip faint">Lower samples more pixels (higher quality); higher is lighter.</span>
    </label>
  </div>

  <div class="card pad">
    <div class="section-title">Transition</div>
    <div class="toggle-row">
      <div>
        <div class="t-name">Smooth transitions</div>
        <div class="t-sub faint">Off = instant jumps (lowest latency, can look steppy)</div>
      </div>
      <label class="switch">
        <input type="checkbox" checked={$config.transition.effect === "smooth"}
          onchange={(e) => setEffect((e.target as HTMLInputElement).checked)} />
        <span class="track"></span>
      </label>
    </div>
    {#if $config.transition.effect === "smooth"}
      <label class="field" style="margin-top:14px">
        <span class="lbl">Transition time <b>{$config.transition.duration_ms} ms</b></span>
        <input type="range" min="30" max="300" step="5" value={$config.transition.duration_ms}
          oninput={(e) => setDuration(+(e.target as HTMLInputElement).value)} />
      </label>
    {/if}
  </div>

  <div class="card pad">
    <div class="section-title">Startup &amp; window</div>
    {#each [
      { key: "run_on_startup", name: "Launch at login", sub: "Start Lueur when you sign in" },
      { key: "start_minimized", name: "Start in tray", sub: "Open hidden in the system tray" },
      { key: "autostart_effect", name: "Auto-start ambilight", sub: "Begin the effect as soon as Lueur launches" },
      { key: "minimize_to_tray", name: "Close to tray", sub: "Closing the window hides it instead of quitting" },
    ] as opt}
      <div class="toggle-row">
        <div>
          <div class="t-name">{opt.name}</div>
          <div class="t-sub faint">{opt.sub}</div>
        </div>
        <label class="switch">
          <input
            type="checkbox"
            checked={$config.behavior[opt.key as keyof typeof $config.behavior]}
            onchange={(e) => {
              const v = (e.target as HTMLInputElement).checked;
              if (opt.key === "run_on_startup") toggleStartup(v);
              else setBehavior(opt.key as keyof typeof $config.behavior, v);
            }}
          />
          <span class="track"></span>
        </label>
      </div>
    {/each}
  </div>

  <div class="card pad about">
    <div class="section-title">About</div>
    <p class="muted">
      Lueur is portable — your setup lives in a <code>lueur-data</code> folder next to the app.
      Bulbs are controlled only over your local network. Currently supported: <b>Yeelight Color (v2)</b>.
    </p>
  </div>
</div>

<style>
  .settings {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
    gap: 18px;
    align-items: start;
  }
  .pad {
    padding: 18px;
  }
  .field {
    display: block;
    margin-bottom: 18px;
  }
  .field:last-child {
    margin-bottom: 0;
  }
  .lbl {
    display: block;
    font-size: 13px;
    color: var(--text-dim);
    margin-bottom: 9px;
  }
  .lbl b {
    color: var(--text);
    font-weight: 600;
  }
  .tip {
    display: block;
    font-size: 11.5px;
    margin-top: 7px;
    line-height: 1.45;
  }
  select {
    width: 100%;
  }
  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 10px 0;
    border-bottom: 1px solid var(--border);
  }
  .toggle-row:last-child {
    border-bottom: none;
  }
  .t-name {
    font-size: 14px;
    font-weight: 550;
  }
  .t-sub {
    font-size: 12px;
    margin-top: 2px;
  }
  .about p {
    font-size: 13px;
    line-height: 1.55;
    margin: 0;
  }
  code {
    background: rgba(10, 9, 20, 0.6);
    padding: 1px 6px;
    border-radius: 6px;
    font-size: 12px;
  }
</style>
