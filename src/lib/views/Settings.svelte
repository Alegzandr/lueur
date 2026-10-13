<script lang="ts">
  import { onMount } from "svelte";
  import { app, edit, errMsg, notify } from "../state.svelte";
  import * as ipc from "../ipc";
  import { defaultConfig, type Appearance, type BehaviorSettings, type MonitorInfo } from "../types";
  import Row from "../ui/Row.svelte";
  import Slider from "../ui/Slider.svelte";
  import Switch from "../ui/Switch.svelte";
  import Segmented from "../ui/Segmented.svelte";

  const DEF = defaultConfig();
  const pct = (v: number) => `${Math.round(v * 100)}%`;

  let monitors = $state<MonitorInfo[]>([]);
  let version = $state("");

  onMount(async () => {
    try {
      monitors = await ipc.listMonitors();
    } catch (e) {
      notify("error", `Could not list displays: ${errMsg(e)}`);
    }
    version = await ipc.appVersion().catch(() => "");
  });

  const c = $derived(app.config);

  async function setStartup(v: boolean) {
    try {
      await ipc.setRunOnStartup(v);
      edit((x) => (x.behavior.run_on_startup = v));
    } catch (e) {
      notify("error", `Could not change the login item: ${errMsg(e)}`);
    }
  }

  function setBehavior(key: keyof BehaviorSettings, v: boolean) {
    if (key === "run_on_startup") void setStartup(v);
    else edit((x) => (x.behavior[key] = v));
  }

  const behaviour: { key: keyof BehaviorSettings; label: string; hint: string }[] = [
    { key: "run_on_startup", label: "Open at login", hint: "Start Lueur when you sign in." },
    { key: "start_minimized", label: "Start in the tray", hint: "Open without showing the window." },
    { key: "autostart_effect", label: "Start the ambilight on launch", hint: "No click needed once Lueur opens." },
    { key: "minimize_to_tray", label: "Keep running when closed", hint: "Closing the window hides it to the tray." },
  ];

  const appearanceOptions: { value: Appearance; label: string }[] = [
    { value: "system", label: "Automatic" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];
</script>

<div class="page">
  <header class="page-head">
    <div>
      <h1 class="page-title">Settings</h1>
      <p class="page-lede">Fine control over capture, colour and how Lueur behaves on your computer.</p>
    </div>
  </header>

  <section class="group">
    <div class="group-head"><h2 class="group-title">Picture</h2></div>
    <div class="card rows">
      <Row label="Display" hint="The screen whose picture the lights follow.">
        <select
          value={c.capture.monitor_index}
          onchange={(e) => edit((x) => (x.capture.monitor_index = +e.currentTarget.value))}
        >
          {#each monitors as m (m.index)}
            <option value={m.index}>{m.name} · {m.width}×{m.height}{m.is_primary ? " · main" : ""}</option>
          {/each}
          {#if monitors.length === 0}
            <option value={0}>Main display</option>
          {/if}
        </select>
      </Row>
      <Row label="Frame rate" hint="Higher is snappier but uses more processor. 40 to 60 suits most bulbs.">
        <Slider
          label="Frame rate"
          value={c.capture.target_fps}
          min={10}
          max={90}
          step={1}
          def={DEF.capture.target_fps}
          format={(v) => `${v} fps`}
          onchange={(v) => edit((x) => (x.capture.target_fps = v))}
        />
      </Row>
      <Row label="Sampling" hint="Reading fewer pixels is lighter on the processor and rarely visible.">
        <Slider
          label="Sampling"
          value={c.engine.sample_step}
          min={1}
          max={12}
          step={1}
          def={DEF.engine.sample_step}
          format={(v) => (v === 1 ? "All" : `1 in ${v}`)}
          onchange={(v) => edit((x) => (x.engine.sample_step = v))}
        />
      </Row>
    </div>
  </section>

  <section class="group">
    <div class="group-head"><h2 class="group-title">Colour</h2></div>
    <div class="card rows">
      <Row label="Smoothing" hint="Lower is calmer and trails the picture; higher reacts at once.">
        <Slider
          label="Smoothing"
          value={c.engine.smoothing}
          min={0.05}
          max={1}
          step={0.01}
          def={DEF.engine.smoothing}
          format={pct}
          ends={["Calm", "Snappy"]}
          onchange={(v) => edit((x) => (x.engine.smoothing = v))}
        />
      </Row>
      <Row label="Brightness">
        <Slider
          label="Brightness"
          value={c.engine.brightness}
          min={0.05}
          max={1}
          step={0.01}
          def={DEF.engine.brightness}
          format={pct}
          onchange={(v) => edit((x) => (x.engine.brightness = v))}
        />
      </Row>
      <Row label="Colour intensity" hint="How strongly hues are pushed beyond what is on screen.">
        <Slider
          label="Colour intensity"
          value={c.engine.saturation}
          min={0}
          max={1}
          step={0.01}
          def={DEF.engine.saturation}
          format={pct}
          onchange={(v) => edit((x) => (x.engine.saturation = v))}
        />
      </Row>
      <Row
        label="White for grey scenes"
        hint="Below this much colour the bulbs switch to a true white instead of guessing a pinkish hue."
      >
        <Slider
          label="White for grey scenes"
          value={c.engine.white_threshold}
          min={0}
          max={0.5}
          step={0.01}
          def={DEF.engine.white_threshold}
          format={(v) => (v === 0 ? "Off" : pct(v))}
          onchange={(v) => edit((x) => (x.engine.white_threshold = v))}
        />
      </Row>
      {#if c.engine.white_threshold > 0}
        <Row label="White warmth" hint="Lower is candlelight, higher is daylight.">
          <Slider
            label="White warmth"
            value={c.engine.white_kelvin}
            min={1700}
            max={6500}
            step={100}
            def={DEF.engine.white_kelvin}
            format={(v) => `${v} K`}
            ends={["Warm", "Cool"]}
            onchange={(v) => edit((x) => (x.engine.white_kelvin = v))}
          />
        </Row>
      {/if}
      <Row label="Dark scenes" hint="Below this level the bulbs dim right down instead of glowing muddy.">
        <Slider
          label="Dark scenes"
          value={c.engine.black_threshold}
          min={0}
          max={40}
          step={1}
          def={DEF.engine.black_threshold}
          format={(v) => (v === 0 ? "Off" : String(v))}
          onchange={(v) => edit((x) => (x.engine.black_threshold = v))}
        />
      </Row>
    </div>
  </section>

  <section class="group">
    <div class="group-head"><h2 class="group-title">Transitions</h2></div>
    <div class="card rows">
      <Row label="Fade between colours" hint="Off jumps instantly. Lowest latency, but it can look stepped.">
        <Switch
          checked={c.transition.effect === "smooth"}
          label="Fade between colours"
          onchange={(v) => edit((x) => (x.transition.effect = v ? "smooth" : "sudden"))}
        />
      </Row>
      {#if c.transition.effect === "smooth"}
        <Row label="Fade length">
          <Slider
            label="Fade length"
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
    <div class="group-head"><h2 class="group-title">Lueur</h2></div>
    <div class="card rows">
      <Row label="Appearance">
        <Segmented
          label="Appearance"
          value={c.interface?.appearance ?? "system"}
          options={appearanceOptions}
          onchange={(v) => edit((x) => (x.interface = { ...x.interface, appearance: v }))}
        />
      </Row>
      {#each behaviour as b (b.key)}
        <Row label={b.label} hint={b.hint}>
          <Switch checked={c.behavior[b.key]} label={b.label} onchange={(v) => setBehavior(b.key, v)} />
        </Row>
      {/each}
    </div>
  </section>

  <footer class="about">
    <p>
      Lueur {version}. Your setup is kept in a <code>lueur-data</code> folder beside the app, so it
      travels with it. Bulbs are only ever reached over your local network.
    </p>
  </footer>
</div>

<style>
  .about {
    margin-top: var(--s-6);
    padding: 0 var(--s-1);
    font-size: var(--t-sm);
    color: var(--text-3);
    max-width: 64ch;
  }
  select {
    max-width: 280px;
  }
</style>
