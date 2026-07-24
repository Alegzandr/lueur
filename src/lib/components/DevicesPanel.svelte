<script lang="ts">
  import { config, updateConfig, notify, errMsg } from "../stores";
  import * as ipc from "../ipc";
  import type { DiscoveredDevice, Light } from "../types";

  let scanning = $state(false);
  let discovered = $state<DiscoveredDevice[]>([]);
  let hasScanned = $state(false);

  // Device ids already bound to a room light.
  const boundIds = $derived(new Set($config.bindings.map((b) => b.device_id)));

  async function scan() {
    scanning = true;
    try {
      discovered = await ipc.discoverDevices(3000);
      hasScanned = true;
      if (discovered.length === 0) {
        notify("info", "No bulbs found. Make sure LAN Control is enabled in the Yeelight app.");
      }
    } catch (e) {
      notify("error", `Scan failed: ${errMsg(e)}`);
    } finally {
      scanning = false;
    }
  }

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
    notify("info", `${prettyName(d)} added — position it in the Room tab.`);
  }

  async function flash(ip: string, port: number, rgb: [number, number, number]) {
    try {
      await ipc.testDevice(ip, port, rgb[0], rgb[1], rgb[2]);
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

  function removeLight(id: string) {
    updateConfig((c) => {
      c.room.lights = c.room.lights.filter((x) => x.id !== id);
      c.bindings = c.bindings.filter((x) => x.light_id !== id);
      return c;
    });
  }
</script>

<div class="panel">
  <div class="col main">
    <div class="head row spread">
      <div>
        <div class="section-title">Discover bulbs</div>
        <p class="muted intro">
          Lueur controls bulbs over your local network — nothing leaves the house. Enable
          <b>LAN Control</b> in the Yeelight app first. We currently support Yeelight Color (v2);
          more models are on the way.
        </p>
      </div>
      <button class="btn primary" onclick={scan} disabled={scanning}>
        {scanning ? "Scanning…" : "Scan network"}
      </button>
    </div>

    <div class="card list">
      {#if scanning}
        <div class="empty faint">Listening for bulbs on the LAN…</div>
      {:else if discovered.length === 0}
        <div class="empty faint">
          {hasScanned ? "No bulbs found yet — try scanning again." : "Scan to find bulbs on your network."}
        </div>
      {:else}
        {#each discovered as d (d.id)}
          <div class="devrow">
            <span class="swatch"></span>
            <div class="grow">
              <div class="name">{prettyName(d)}</div>
              <div class="meta faint">
                {d.model || "bulb"} · {d.ip}:{d.port}
                {#if !supportsMusic(d)}· <span class="warn">no music mode</span>{/if}
              </div>
            </div>
            <button class="btn ghost" onclick={() => flash(d.ip, d.port, [56, 232, 255])}>Identify</button>
            {#if boundIds.has(d.id)}
              <span class="added">Added ✓</span>
            {:else}
              <button class="btn" onclick={() => addAsLight(d)}>+ Add as light</button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  </div>

  <div class="col side">
    <div class="section-title">Your lights</div>
    <div class="card list">
      {#if $config.room.lights.length === 0}
        <div class="empty faint">No lights yet. Add bulbs from the list on the left.</div>
      {:else}
        {#each $config.room.lights as l (l.id)}
          {@const b = bindingFor(l.id)}
          <div class="lightrow">
            <label class="switch" title="Enable / disable">
              <input type="checkbox" checked={l.enabled} onchange={() => toggleLight(l.id)} />
              <span class="track"></span>
            </label>
            <div class="grow">
              <input
                class="rename"
                type="text"
                value={l.name}
                oninput={(e) => renameLight(l.id, (e.target as HTMLInputElement).value)}
              />
              <div class="meta faint">
                {#if b}{b.ip}{:else}<span class="warn">not bound</span>{/if}
              </div>
            </div>
            {#if b}
              <button
                class="btn ghost small"
                title="Identify"
                onclick={() => flash(b.ip, b.port, [155, 108, 255])}>◉</button
              >
            {/if}
            <button class="btn ghost small danger" title="Remove" onclick={() => removeLight(l.id)}
              >✕</button
            >
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .panel {
    display: grid;
    grid-template-columns: 1.5fr 1fr;
    gap: 22px;
    height: 100%;
  }
  .col {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .head {
    margin-bottom: 14px;
    align-items: flex-start;
    gap: 16px;
  }
  .intro {
    font-size: 13px;
    max-width: 52ch;
    line-height: 1.5;
    margin: 0;
  }
  .list {
    padding: 8px;
    overflow: auto;
    flex: 1;
  }
  .empty {
    padding: 40px 16px;
    text-align: center;
    font-size: 13.5px;
  }
  .devrow,
  .lightrow {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 11px 12px;
    border-radius: var(--radius-sm);
  }
  .devrow:hover,
  .lightrow:hover {
    background: var(--surface-hover);
  }
  .swatch {
    width: 26px;
    height: 26px;
    border-radius: 8px;
    flex: none;
    background: var(--accent-grad);
    box-shadow: 0 0 12px rgba(155, 108, 255, 0.5);
  }
  .name {
    font-size: 14px;
    font-weight: 550;
  }
  .meta {
    font-size: 12px;
    margin-top: 2px;
  }
  .warn {
    color: var(--amber);
  }
  .added {
    color: var(--green);
    font-size: 13px;
    font-weight: 550;
    padding: 0 8px;
  }
  .rename {
    width: 100%;
    background: transparent;
    border: 1px solid transparent;
    padding: 4px 6px;
    font-size: 14px;
    font-weight: 550;
    border-radius: 8px;
  }
  .rename:hover {
    border-color: var(--border);
  }
  .btn.small {
    padding: 6px 10px;
    font-size: 13px;
  }
  .danger:hover {
    color: var(--red);
  }
</style>
