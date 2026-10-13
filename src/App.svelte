<script lang="ts">
  import { onMount } from "svelte";
  import { app, bootstrap, refreshStatus, toggleEffect, plural, type View } from "./lib/state.svelte";
  import { setWindowTheme } from "./lib/ipc";
  import Icon, { type IconName } from "./lib/ui/Icon.svelte";
  import Toasts from "./lib/ui/Toasts.svelte";
  import Overview from "./lib/views/Overview.svelte";
  import Room from "./lib/views/Room.svelte";
  import Bulbs from "./lib/views/Bulbs.svelte";
  import Settings from "./lib/views/Settings.svelte";

  const nav: { id: View; label: string; icon: IconName; key: string }[] = [
    { id: "overview", label: "Overview", icon: "home", key: "1" },
    { id: "room", label: "Room", icon: "plan", key: "2" },
    { id: "bulbs", label: "Bulbs", icon: "bulb", key: "3" },
    { id: "settings", label: "Settings", icon: "sliders", key: "4" },
  ];

  onMount(() => {
    void bootstrap();
    const t = setInterval(refreshStatus, 4000);
    return () => clearInterval(t);
  });

  // Appearance: the stylesheet follows the OS by default; forcing a theme sets
  // an attribute on <html> and tells the native title bar to match.
  $effect(() => {
    const a = app.config.interface?.appearance ?? "system";
    const root = document.documentElement;
    if (a === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", a);
    setWindowTheme(a === "system" ? null : a).catch(() => {});
  });

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod || e.altKey || e.shiftKey) return;
    const item = nav.find((n) => n.key === e.key);
    if (item) {
      e.preventDefault();
      app.view = item.id;
    } else if (e.key === "Enter") {
      e.preventDefault();
      void toggleEffect();
    }
  }

  const unbound = $derived(
    app.config.room.lights.filter((l) => !app.config.bindings.some((b) => b.light_id === l.id)).length,
  );
  const shortcut = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <svg class="mark" viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
        <defs>
          <radialGradient id="mark-glow" cx="0.5" cy="0.45" r="0.6">
            <stop offset="0" stop-color="var(--glow)" stop-opacity="0.9" />
            <stop offset="1" stop-color="var(--glow)" stop-opacity="0" />
          </radialGradient>
        </defs>
        <ellipse cx="12" cy="10.5" rx="11" ry="9" fill="url(#mark-glow)" />
        <rect x="5" y="6" width="14" height="9.5" rx="1.6" fill="var(--sidebar)" stroke="currentColor" stroke-width="1.5" />
        <path d="M9.5 19h5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
      </svg>
      <span class="wordmark">Lueur</span>
    </div>

    <nav aria-label="Sections">
      {#each nav as n (n.id)}
        <button
          class="nav-item"
          class:active={app.view === n.id}
          aria-current={app.view === n.id ? "page" : undefined}
          title="{n.label} ({shortcut}{n.key})"
          onclick={() => (app.view = n.id)}
        >
          <Icon name={n.icon} size={17} />
          <span class="nav-label">{n.label}</span>
          {#if n.id === "bulbs" && unbound > 0}
            <span class="badge" title="{plural(unbound, 'light')} without a bulb">{unbound}</span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="power" class:live={app.status.running}>
      <div class="power-state" role="status">
        <span class="dot" aria-hidden="true"></span>
        <div>
          <div class="power-title">{app.status.running ? "Ambilight on" : "Ambilight off"}</div>
          <div class="power-sub">
            {#if app.status.running}
              {app.status.connected_lights === 0
                ? "No bulb answering"
                : `${plural(app.status.connected_lights, "bulb")} following`}
            {:else if app.config.bindings.length === 0}
              No bulbs set up yet
            {:else}
              {plural(app.config.bindings.length, "bulb")} ready
            {/if}
          </div>
        </div>
      </div>
      <button
        class="btn power-btn"
        class:primary={!app.status.running}
        disabled={app.busy || (!app.status.running && app.config.bindings.length === 0)}
        title="{app.status.running ? 'Stop' : 'Start'} ({shortcut}Enter)"
        onclick={toggleEffect}
      >
        <Icon name="power" size={15} />
        {app.busy ? "One moment" : app.status.running ? "Stop" : "Start"}
      </button>
    </div>
  </aside>

  <main class="content">
    {#if app.view === "overview"}
      <Overview />
    {:else if app.view === "room"}
      <Room />
    {:else if app.view === "bulbs"}
      <Bulbs />
    {:else}
      <Settings />
    {/if}
  </main>

  <Toasts />
</div>

<style>
  .shell {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 216px;
    flex: none;
    display: flex;
    flex-direction: column;
    padding: var(--s-4) var(--s-3);
    background: var(--sidebar);
    border-right: 1px solid var(--line);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: var(--s-2) var(--s-2) var(--s-5);
  }
  .mark {
    color: var(--text);
  }
  .wordmark {
    font-family: var(--font-display);
    font-size: 1.3rem;
    font-weight: 500;
    letter-spacing: 0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 var(--s-2) 0 10px;
    border-radius: var(--r-md);
    color: var(--text-2);
    font-weight: 500;
    text-align: left;
    transition:
      background var(--dur) var(--ease),
      color var(--dur) var(--ease);
  }
  .nav-item:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow-1);
  }
  .nav-label {
    flex: 1;
  }
  .badge {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: var(--r-full);
    background: var(--glow-soft);
    color: var(--glow-text);
    font-size: var(--t-xs);
    font-weight: 600;
    line-height: 18px;
    text-align: center;
  }

  .power {
    margin-top: auto;
    padding: var(--s-3);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--shadow-1);
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
  }
  .power-state {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .dot {
    width: 8px;
    height: 8px;
    margin-top: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--line-strong);
    transition:
      background var(--dur) var(--ease),
      box-shadow var(--dur) var(--ease);
  }
  .live .dot {
    background: var(--glow);
    box-shadow: 0 0 0 4px var(--glow-soft);
    animation: breathe 3.2s ease-in-out infinite;
  }
  .power-title {
    font-weight: 600;
  }
  .power-sub {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .power-btn {
    width: 100%;
  }

  .content {
    flex: 1;
    min-width: 0;
    overflow: auto;
  }

  @keyframes breathe {
    50% {
      box-shadow: 0 0 0 7px var(--glow-soft);
    }
  }
</style>
