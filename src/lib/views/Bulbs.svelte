<script lang="ts">
  import { onMount } from "svelte";
  import {
    app,
    addDevice,
    assignDevice,
    bindingFor,
    deviceName,
    identify,
    lightById,
    plural,
    scan,
    unassign,
  } from "../state.svelte";
  import type { DiscoveredDevice } from "../types";
  import Icon from "../ui/Icon.svelte";

  // First visit: look straight away rather than greeting with an empty list.
  onMount(() => {
    if (!app.hasScanned) void scan();
  });

  const lights = $derived(app.config.room.lights);
  const unboundLights = $derived(lights.filter((l) => !bindingFor(l.id)));

  function ownerOf(d: DiscoveredDevice) {
    const b = app.config.bindings.find((x) => x.device_id === d.id);
    return b ? lightById(b.light_id) : undefined;
  }

  function supportsMusic(d: DiscoveredDevice): boolean {
    return d.support.includes("set_music") && d.support.includes("set_rgb");
  }

  function onAssign(d: DiscoveredDevice, e: Event & { currentTarget: HTMLSelectElement }) {
    const id = e.currentTarget.value;
    e.currentTarget.value = "";
    if (id === "__new") addDevice(d);
    else if (id) assignDevice(id, d);
  }
</script>

<div class="page">
  <header class="page-head">
    <div>
      <h1 class="page-title">Bulbs</h1>
      <p class="page-lede">Lueur talks to your bulbs directly over the local network. Nothing leaves your home.</p>
    </div>
    <button class="btn primary" onclick={() => scan()} disabled={app.scanning}>
      <Icon name={app.scanning ? "refresh" : "search"} size={15} />
      {app.scanning ? "Searching" : app.hasScanned ? "Search again" : "Search network"}
    </button>
  </header>

  <section class="group">
    <div class="group-head">
      <h2 class="group-title">On your network</h2>
      {#if app.hasScanned && !app.scanning}
        <span class="group-note">{plural(app.discovered.length, "bulb")} found</span>
      {/if}
    </div>

    <div class="card rows">
      {#if app.scanning && app.discovered.length === 0}
        <div class="empty">
          <span class="spinner" aria-hidden="true"></span>
          <p>Listening for bulbs.</p>
        </div>
      {:else if app.discovered.length === 0}
        <div class="empty">
          <Icon name="bulb" size={28} />
          <p class="empty-title">{app.hasScanned ? "No bulbs answered" : "Find the bulbs around you"}</p>
          <p>
            {app.hasScanned
              ? "Check the tips below, then search again."
              : "Search the network to list the Yeelight bulbs it can reach."}
          </p>
        </div>
      {:else}
        {#each app.discovered as d (d.id)}
          {@const owner = ownerOf(d)}
          <div class="device">
            <span class="device-icon" class:used={owner}><Icon name="bulb" size={18} /></span>
            <div class="device-text">
              <div class="device-name">{deviceName(d)}</div>
              <div class="device-meta num">
                {d.model || "bulb"} · {d.ip}
                {#if !supportsMusic(d)}<span class="warn"> · cannot follow the screen</span>{/if}
              </div>
            </div>
            <button class="btn plain sm" title="Flash it warm white" onclick={() => identify(d.ip, d.port)}>
              <Icon name="flash" size={14} /> Identify
            </button>
            {#if owner}
              <span class="used-by"><Icon name="check" size={14} /> {owner.name}</span>
            {:else if unboundLights.length > 0}
              <select class="assign" aria-label="Use {deviceName(d)} for" onchange={(e) => onAssign(d, e)}>
                <option value="" selected disabled>Use for…</option>
                {#each unboundLights as l (l.id)}
                  <option value={l.id}>{l.name}</option>
                {/each}
                <option value="__new">A new light</option>
              </select>
            {:else}
              <button class="btn sm" onclick={() => addDevice(d)}>
                <Icon name="plus" size={14} /> Add to room
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  </section>

  {#if lights.length > 0}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title">In your room</h2>
        <button class="btn plain sm" onclick={() => (app.view = "room")}>Open the plan</button>
      </div>
      <div class="card rows">
        {#each lights as l (l.id)}
          {@const b = bindingFor(l.id)}
          <div class="device">
            <span class="device-icon" class:used={b}><Icon name={b ? "link" : "unlink"} size={16} /></span>
            <div class="device-text">
              <div class="device-name">{l.name}</div>
              <div class="device-meta num">
                {#if b}{b.name || "Yeelight"} · {b.ip}{:else}<span class="warn">No bulb assigned</span>{/if}
              </div>
            </div>
            {#if b}
              <button class="btn plain sm" onclick={() => identify(b.ip, b.port)}>
                <Icon name="flash" size={14} /> Identify
              </button>
              <button class="btn plain sm" title="Keep the light on the plan, forget the bulb" onclick={() => unassign(l.id)}>
                Unassign
              </button>
            {:else}
              <span class="faint hint">Pick one in the list above</span>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <section class="group">
    <details class="card tips">
      <summary>
        <Icon name="info" size={16} />
        <span>A bulb is missing?</span>
        <Icon name="chevron" size={14} />
      </summary>
      <ol>
        <li>Open the Yeelight app, pick the bulb and turn on <b>LAN Control</b>.</li>
        <li>Make sure this computer and the bulb share the same network (not a guest network).</li>
        <li>Allow Lueur through your firewall: discovery uses UDP port 1982.</li>
        <li>Supported today: Yeelight Color bulbs. Others will follow once they can be tested.</li>
      </ol>
    </details>
  </section>
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--s-1);
    padding: var(--s-7) var(--s-5);
    text-align: center;
    color: var(--text-3);
  }
  .empty :global(svg) {
    margin-bottom: var(--s-2);
  }
  .empty-title {
    color: var(--text);
    font-weight: 600;
    font-size: var(--t-lg);
  }
  .spinner {
    width: 22px;
    height: 22px;
    margin-bottom: var(--s-2);
    border-radius: 50%;
    border: 2px solid var(--line);
    border-top-color: var(--glow);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .device {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-3) var(--s-2) var(--s-4);
    min-height: 56px;
  }
  .device-icon {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border-radius: var(--r-md);
    background: var(--surface-2);
    color: var(--text-3);
  }
  .device-icon.used {
    background: var(--glow-soft);
    color: var(--glow-text);
  }
  .device-text {
    flex: 1;
    min-width: 0;
  }
  .device-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .device-meta {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .warn {
    color: var(--glow-text);
  }
  .used-by {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 180px;
    padding: 0 var(--s-2);
    color: var(--text-2);
    font-size: var(--t-sm);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .used-by :global(svg) {
    color: var(--ok);
  }
  .assign {
    height: 26px;
    font-size: var(--t-sm);
    max-width: 170px;
  }
  .hint {
    font-size: var(--t-sm);
    padding-right: var(--s-2);
  }

  .tips summary {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-3) var(--s-4);
    cursor: pointer;
    list-style: none;
    font-weight: 500;
    color: var(--text-2);
  }
  .tips summary::-webkit-details-marker {
    display: none;
  }
  .tips summary span {
    flex: 1;
  }
  .tips summary :global(svg:last-child) {
    transition: transform var(--dur) var(--ease);
  }
  .tips[open] summary :global(svg:last-child) {
    transform: rotate(90deg);
  }
  .tips ol {
    margin: 0;
    padding: 0 var(--s-5) var(--s-4) calc(var(--s-4) + 24px + var(--s-2));
    color: var(--text-2);
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
  }
  .tips b {
    color: var(--text);
  }
</style>
