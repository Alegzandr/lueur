<script lang="ts">
  import {
    config,
    updateConfig,
    notify,
    errMsg,
    discovered,
    hasScanned,
    scanning,
    scanDevices,
    powerState,
    powerBusy,
    toggleLightPower,
    setAllPower,
    busy,
  } from "../stores";
  import * as ipc from "../ipc";
  import Icon from "./Icon.svelte";
  import type { DiscoveredDevice, Light } from "../types";

  // Warm white, so an identify flash reads as "this is the bulb" rather than as
  // a colour the effect would ever pick on its own.
  const IDENTIFY_RGB: [number, number, number] = [255, 190, 120];

  // Device ids already bound to a room light.
  const boundIds = $derived(new Set($config.bindings.map((b) => b.device_id)));

  function prettyName(d: DiscoveredDevice): string {
    if (d.name && d.name.trim()) return d.name;
    return `Yeelight ${d.model || "bulb"}`;
  }

  function supportsMusic(d: DiscoveredDevice): boolean {
    return d.support.includes("set_music") && d.support.includes("set_rgb");
  }

  function addAsLight(d: DiscoveredDevice) {
    updateConfig((c) => {
      // Stagger new lights across the top of the room so they don't stack.
      const n = c.room.lights.length;
      const light: Light = {
        id: crypto.randomUUID(),
        name: prettyName(d),
        pos: { x: 250 + ((n * 160) % 600), y: 110 + (n % 2) * 60 },
        height: 0.5,
        zone_override: null,
        enabled: true,
      };
      c.room.lights.push(light);
      c.bindings.push({
        light_id: light.id,
        device_id: d.id,
        ip: d.ip,
        port: d.port,
        model: d.model,
        name: prettyName(d),
      });
      return c;
    });
    notify("info", `${prettyName(d)} added. Position it in the Room tab.`);
  }

  async function flash(ip: string, port: number) {
    try {
      await ipc.testDevice(ip, port, ...IDENTIFY_RGB);
      notify("info", "Sent identify flash.");
    } catch (e) {
      notify("error", `Could not reach bulb: ${errMsg(e)}`);
    }
  }

  function bindingFor(lightId: string) {
    return $config.bindings.find((b) => b.light_id === lightId);
  }

  function renameLight(id: string, name: string) {
    updateConfig((c) => {
      const l = c.room.lights.find((x) => x.id === id);
      if (l) l.name = name;
      const b = c.bindings.find((x) => x.light_id === id);
      if (b) b.name = name;
      return c;
    });
  }

  function toggleLight(id: string) {
    updateConfig((c) => {
      const l = c.room.lights.find((x) => x.id === id);
      if (l) l.enabled = !l.enabled;
      return c;
    });
  }

  // Optimistic: we have no push channel from the bulbs, so an unknown light is
  // assumed on (they are on whenever the effect has driven them).
  function isOn(id: string): boolean {
    return $powerState[id] ?? true;
  }

  function removeLight(id: string) {
    updateConfig((c) => {
      c.room.lights = c.room.lights.filter((x) => x.id !== id);
      c.bindings = c.bindings.filter((x) => x.light_id !== id);
      return c;
    });
  }
</script>

<div class="panels">
  <section class="col">
    <header class="head">
      <div class="grow">
        <h2 class="section-title">Discover</h2>
        <p class="intro muted">
          Bulbs are controlled over your local network. Enable LAN Control in the Yeelight app
          before scanning. Supported today: Yeelight Color (v2).
        </p>
      </div>
      <button class="btn primary" onclick={() => scanDevices()} disabled={$scanning}>
        <Icon name="search" />
        {$scanning ? "Scanning" : "Scan network"}
      </button>
    </header>

    <div class="panel list">
      {#if $scanning}
        <p class="empty faint">Listening for bulbs on the network.</p>
      {:else if $discovered.length === 0}
        <p class="empty faint">
          {$hasScanned
            ? "No bulbs found. Check that LAN Control is on in the Yeelight app, then scan again."
            : "Scan to find bulbs on your network."}
        </p>
      {:else}
        {#each $discovered as d (d.id)}
          <div class="devrow">
            <Icon name="bulb" size={18} />
            <div class="grow">
              <div class="name">{prettyName(d)}</div>
              <div class="meta faint">
                {d.model || "bulb"} · {d.ip}:{d.port}{#if !supportsMusic(d)}
                  · <span class="alert">no music mode</span>
                {/if}
              </div>
            </div>
            <button class="btn ghost" onclick={() => flash(d.ip, d.port)}>Identify</button>
            {#if boundIds.has(d.id)}
              <span class="added"><Icon name="check" size={14} /> Added</span>
            {:else}
              <button class="btn" onclick={() => addAsLight(d)}>
                <Icon name="plus" size={14} /> Add
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  </section>

  <section class="col">
    <header class="head">
      <h2 class="section-title grow">Your lights</h2>
      {#if $config.bindings.length > 0}
        <div class="bulk">
          <button class="btn ghost small" disabled={$busy} onclick={() => setAllPower(true)}>
            All on
          </button>
          <button
            class="btn ghost small"
            title="Turns every bulb off and stops the effect"
            disabled={$busy}
            onclick={() => setAllPower(false)}
          >
            All off
          </button>
        </div>
      {/if}
    </header>

    <div class="panel list">
      {#if $config.room.lights.length === 0}
        <p class="empty faint">No lights yet. Add bulbs from the list on the left.</p>
      {:else}
        {#each $config.room.lights as l (l.id)}
          {@const b = bindingFor(l.id)}
          <div class="lightrow" class:off={!l.enabled}>
            <label
              class="switch"
              title={l.enabled ? "Exclude from the effect" : "Include in the effect"}
            >
              <input
                type="checkbox"
                checked={l.enabled}
                aria-label="Include {l.name} in the effect"
                onchange={() => toggleLight(l.id)}
              />
              <span class="track"></span>
            </label>
            <div class="grow">
              <input
                class="rename"
                type="text"
                value={l.name}
                aria-label="Light name"
                oninput={(e) => renameLight(l.id, (e.target as HTMLInputElement).value)}
              />
              <div class="meta faint">
                {#if b}{b.ip}{:else}<span class="alert">not bound to a bulb</span>{/if}
              </div>
            </div>
            {#if b}
              <button
                class="btn ghost icon power"
                class:on={isOn(l.id)}
                title={isOn(l.id) ? "Turn bulb off" : "Turn bulb on"}
                aria-label={isOn(l.id) ? `Turn ${l.name} off` : `Turn ${l.name} on`}
                disabled={$powerBusy.has(l.id)}
                onclick={() => toggleLightPower(l.id)}
              >
                <Icon name="power" />
              </button>
              <button
                class="btn ghost icon"
                title="Identify"
                aria-label="Flash {l.name} to identify it"
                onclick={() => flash(b.ip, b.port)}
              >
                <Icon name="target" />
              </button>
            {/if}
            <button
              class="btn ghost icon danger"
              title="Remove"
              aria-label="Remove {l.name}"
              onclick={() => removeLight(l.id)}
            >
              <Icon name="trash" />
            </button>
          </div>
        {/each}
      {/if}
    </div>
  </section>
</div>

<style>
  .panels {
    display: grid;
    grid-template-columns: 1.4fr 1fr;
    gap: var(--s-5);
    height: 100%;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: var(--s-4);
    min-height: 32px;
    margin-bottom: var(--s-3);
  }
  .intro {
    font-size: var(--t-2);
    max-width: 62ch;
    margin: var(--s-2) 0 0;
  }
  .bulk {
    display: flex;
    gap: var(--s-1);
    flex: none;
  }
  .btn.small {
    min-height: 28px;
    padding: 0 var(--s-2);
    font-size: var(--t-2);
  }

  .list {
    padding: var(--s-1);
    overflow: auto;
    flex: 1;
  }
  .empty {
    padding: var(--s-7) var(--s-4);
    text-align: center;
    margin: 0;
    text-wrap: balance;
  }

  .devrow,
  .lightrow {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-3);
    border-radius: var(--r-1);
    color: var(--text-3);
    transition: background var(--dur) var(--ease);
  }
  .devrow:hover,
  .lightrow:hover {
    background: var(--surface-raised);
  }
  .lightrow.off .rename {
    color: var(--text-3);
  }

  .name,
  .rename {
    font-size: var(--t-3);
    font-weight: 500;
    color: var(--text);
  }
  .meta {
    font-size: var(--t-1);
    font-variant-numeric: tabular-nums;
  }
  .alert {
    color: var(--danger);
  }

  .added {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    color: var(--ok);
    font-size: var(--t-2);
    font-weight: 500;
    padding: 0 var(--s-2);
  }

  .rename {
    width: 100%;
    min-height: 22px;
    background: transparent;
    border-color: transparent;
    padding: 0 var(--s-1);
    margin-left: calc(var(--s-1) * -1);
  }
  .rename:hover {
    border-color: var(--border);
  }

  /* The power glyph reads as the bulb's current state at a glance. */
  .power {
    color: var(--text-3);
  }
  .power.on {
    color: var(--ok);
  }
  .power:disabled {
    color: var(--border-strong);
  }
</style>
