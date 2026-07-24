<script lang="ts">
  import { onMount } from "svelte";
  import { bootstrap, status, busy, toast, toggleEffect, refreshStatus } from "./lib/stores";
  import DevicesPanel from "./lib/components/DevicesPanel.svelte";
  import RoomEditor from "./lib/components/RoomEditor.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";

  type Tab = "room" | "devices" | "settings";
  let tab = $state<Tab>("room");

  onMount(() => {
    void bootstrap();
    const t = setInterval(refreshStatus, 4000);
    return () => clearInterval(t);
  });

  const tabs: { id: Tab; label: string }[] = [
    { id: "room", label: "Room" },
    { id: "devices", label: "Lights" },
    { id: "settings", label: "Settings" },
  ];
</script>

<div class="app">
  <header class="topbar">
    <div class="brand">
      <div class="logo" aria-hidden="true">
        <svg viewBox="0 0 48 48" width="34" height="34">
          <defs>
            <linearGradient id="lg" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stop-color="#38e8ff" />
              <stop offset="0.55" stop-color="#9b6cff" />
              <stop offset="1" stop-color="#ff5c9d" />
            </linearGradient>
          </defs>
          <rect x="8" y="12" width="32" height="22" rx="5" fill="none" stroke="url(#lg)" stroke-width="3" />
          <rect x="19" y="34" width="10" height="4" rx="2" fill="url(#lg)" />
        </svg>
      </div>
      <div class="titles">
        <h1>Lueur</h1>
        <span class="tagline faint">ambilight for cosy nights</span>
      </div>
    </div>

    <div class="controls">
      <div class="status-pill" class:on={$status.running}>
        <span class="dot"></span>
        {#if $status.running}
          Running · {$status.connected_lights} light{$status.connected_lights === 1 ? "" : "s"}
        {:else}
          Idle
        {/if}
      </div>
      <button class="btn primary power" onclick={toggleEffect} disabled={$busy}>
        {#if $busy}
          Working…
        {:else if $status.running}
          ■ Stop
        {:else}
          ▶ Start ambilight
        {/if}
      </button>
    </div>
  </header>

  <div class="body">
    <nav class="sidenav">
      {#each tabs as t}
        <button class="navbtn" class:active={tab === t.id} onclick={() => (tab = t.id)}>
          {#if t.id === "room"}
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M3 10.5 12 4l9 6.5" /><path d="M5 9.5V20h14V9.5" /><rect x="9" y="13" width="6" height="7" /></svg>
          {:else if t.id === "devices"}
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M9 18h6" /><path d="M10 21h4" /><path d="M12 3a6 6 0 0 0-4 10.5c.8.8 1 1.3 1 2.5h6c0-1.2.2-1.7 1-2.5A6 6 0 0 0 12 3Z" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="3.2" /><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2" /></svg>
          {/if}
          <span>{t.label}</span>
        </button>
      {/each}
      <div class="nav-foot faint">v0.1 · portable</div>
    </nav>

    <main class="content">
      {#if tab === "room"}
        <RoomEditor />
      {:else if tab === "devices"}
        <DevicesPanel />
      {:else}
        <SettingsPanel />
      {/if}
    </main>
  </div>

  {#if $toast}
    <div class="toast" class:error={$toast.kind === "error"}>{$toast.msg}</div>
  {/if}
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 22px;
    border-bottom: 1px solid var(--border);
    background: rgba(9, 8, 18, 0.5);
    backdrop-filter: blur(10px);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .logo {
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 13px;
    background: rgba(20, 18, 40, 0.8);
    border: 1px solid var(--border);
    box-shadow: var(--glow);
  }
  .titles h1 {
    font-size: 19px;
    line-height: 1;
  }
  .tagline {
    font-size: 11.5px;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .status-pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--text-dim);
    padding: 6px 12px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: rgba(10, 9, 20, 0.5);
  }
  .status-pill .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-faint);
  }
  .status-pill.on {
    color: var(--green);
    border-color: rgba(87, 227, 154, 0.4);
  }
  .status-pill.on .dot {
    background: var(--green);
    box-shadow: 0 0 10px var(--green);
  }
  .power {
    min-width: 150px;
    justify-content: center;
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .sidenav {
    width: 148px;
    flex: none;
    padding: 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-right: 1px solid var(--border);
  }
  .navbtn {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    color: var(--text-dim);
    font-size: 14px;
    font-weight: 550;
    transition: background 0.15s, color 0.15s;
  }
  .navbtn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .navbtn.active {
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border-strong);
  }
  .nav-foot {
    margin-top: auto;
    font-size: 11px;
    padding: 8px 12px;
  }

  .content {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: 22px;
  }

  .toast {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    padding: 11px 18px;
    border-radius: 12px;
    background: rgba(25, 23, 48, 0.95);
    border: 1px solid var(--border-strong);
    color: var(--text);
    font-size: 13.5px;
    box-shadow: var(--shadow);
    max-width: 70vw;
  }
  .toast.error {
    border-color: rgba(255, 90, 110, 0.5);
    color: #ffd7dc;
  }
</style>
