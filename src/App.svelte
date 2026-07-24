<script lang="ts">
  import { onMount } from "svelte";
  import { bootstrap, status, busy, toast, toggleEffect, refreshStatus, plural } from "./lib/stores";
  import DevicesPanel from "./lib/components/DevicesPanel.svelte";
  import RoomEditor from "./lib/components/RoomEditor.svelte";
  import SettingsPanel from "./lib/components/SettingsPanel.svelte";
  import Icon, { type IconName } from "./lib/components/Icon.svelte";

  type Tab = "room" | "devices" | "settings";
  let tab = $state<Tab>("room");

  onMount(() => {
    void bootstrap();
    const t = setInterval(refreshStatus, 4000);
    return () => clearInterval(t);
  });

  const tabs: { id: Tab; label: string; icon: IconName }[] = [
    { id: "room", label: "Room", icon: "room" },
    { id: "devices", label: "Lights", icon: "bulb" },
    { id: "settings", label: "Settings", icon: "settings" },
  ];
</script>

<div class="app">
  <header class="topbar">
    <div class="brand">
      <svg class="mark" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
        <rect
          x="3.5"
          y="5"
          width="17"
          height="11.5"
          rx="2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.6"
        />
        <path d="M9.5 19.5h5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
        <rect x="6.5" y="8" width="11" height="5.5" rx="1" fill="currentColor" opacity="0.32" />
      </svg>
      <h1>Lueur</h1>
    </div>

    <div class="controls">
      <!-- No idle indicator: the button already reads "Start ambilight" when
           nothing is running. Only the live light count adds anything, and a
           running effect driving zero bulbs is worth flagging. -->
      {#if $status.running}
        <span class="status" class:none={$status.connected_lights === 0} role="status">
          {$status.connected_lights === 0
            ? "no bulbs connected"
            : plural($status.connected_lights, "light")}
        </span>
      {/if}
      <button class="btn primary power" onclick={toggleEffect} disabled={$busy}>
        {#if $busy}
          Working
        {:else if $status.running}
          <Icon name="stop" /> Stop
        {:else}
          <Icon name="play" /> Start ambilight
        {/if}
      </button>
    </div>
  </header>

  <div class="body">
    <nav class="sidenav" aria-label="Sections">
      {#each tabs as t}
        <button
          class="navbtn"
          class:active={tab === t.id}
          aria-current={tab === t.id ? "page" : undefined}
          onclick={() => (tab = t.id)}
        >
          <Icon name={t.icon} size={18} />
          <span>{t.label}</span>
        </button>
      {/each}
      <div class="nav-foot faint">v0.1</div>
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
    <div class="toast" class:error={$toast.kind === "error"} role="status" aria-live="polite">
      {$toast.msg}
    </div>
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
    gap: var(--s-4);
    padding: var(--s-3) var(--s-4);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  /* The mark sits below the wordmark in weight: it should not outshout content. */
  .mark {
    color: var(--text-2);
  }
  .brand h1 {
    font-size: var(--t-5);
    line-height: 1;
    letter-spacing: -0.01em;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .status {
    font-size: var(--t-2);
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
  }
  .status.none {
    color: var(--danger);
  }
  .power {
    min-width: 148px;
  }

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .sidenav {
    width: 152px;
    flex: none;
    padding: var(--s-3) var(--s-2);
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-right: 1px solid var(--border);
  }
  .navbtn {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    min-height: 34px;
    padding: 0 var(--s-3);
    border-radius: var(--r-1);
    color: var(--text-3);
    font-size: var(--t-3);
    font-weight: 500;
    text-align: left;
    transition:
      background var(--dur) var(--ease),
      color var(--dur) var(--ease);
  }
  .navbtn:hover {
    background: var(--surface);
    color: var(--text);
  }
  .navbtn.active {
    background: var(--surface-raised);
    color: var(--text);
  }
  .nav-foot {
    margin-top: auto;
    font-size: var(--t-1);
    padding: var(--s-2) var(--s-3);
  }

  .content {
    flex: 1;
    min-width: 0;
    overflow: auto;
    padding: var(--s-5);
  }

  .toast {
    position: fixed;
    bottom: var(--s-5);
    left: 50%;
    transform: translateX(-50%);
    padding: var(--s-2) var(--s-4);
    border-radius: var(--r-1);
    background: var(--surface-raised);
    border: 1px solid var(--border-strong);
    color: var(--text);
    font-size: var(--t-3);
    max-width: 60ch;
  }
  .toast.error {
    border-color: var(--danger);
    color: var(--danger);
  }
</style>
