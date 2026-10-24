<script lang="ts">
  import { onMount } from "svelte";
  import { app, bootstrap, fail, refreshStatus, reloadStates, toggleEffect, type View } from "./lib/state.svelte";
  import { lang, t, tn } from "./lib/i18n";
  import { onEffectChanged, onLightsChanged, onUpdateReady, setUiLanguage, setWindowTheme, updateReady } from "./lib/ipc";
  import Icon, { type IconName } from "./lib/ui/Icon.svelte";
  import Toasts from "./lib/ui/Toasts.svelte";
  import UpdateBanner from "./lib/ui/UpdateBanner.svelte";
  import Lighting from "./lib/views/Lighting.svelte";
  import Room from "./lib/views/Room.svelte";
  import Bulbs from "./lib/views/Bulbs.svelte";
  import Settings from "./lib/views/Settings.svelte";

  const nav: { id: View; label: () => string; icon: IconName; key: string }[] = [
    { id: "lighting", label: () => t("nav_lighting"), icon: "home", key: "1" },
    { id: "room", label: () => t("nav_room"), icon: "plan", key: "2" },
    { id: "bulbs", label: () => t("nav_bulbs"), icon: "bulb", key: "3" },
    { id: "settings", label: () => t("nav_settings"), icon: "sliders", key: "4" },
  ];

  onMount(() => {
    void bootstrap();
    const poll = setInterval(refreshStatus, 4000);
    // Started or stopped from the tray: update now rather than at the next poll.
    let unlisten = () => {};
    onEffectChanged((error) => {
      if (error) fail(error);
      void refreshStatus();
    }).then((u) => (unlisten = u));
    // The tray or automatic mode changed what the lights show.
    let unlistenLights = () => {};
    onLightsChanged(() => void reloadStates()).then((u) => (unlistenLights = u));
    // An update installed in the background waits for a restart.
    let unlistenUpdate = () => {};
    updateReady()
      .then((v) => (app.updateReady = v))
      .catch(() => {});
    onUpdateReady((v) => (app.updateReady = v)).then((u) => (unlistenUpdate = u));
    return () => {
      clearInterval(poll);
      unlisten();
      unlistenLights();
      unlistenUpdate();
    };
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

  $effect(() => {
    const l = lang();
    document.documentElement.lang = l;
    setUiLanguage(l).catch(() => {});
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

  const bound = $derived(app.config.bindings.length);
  const unbound = $derived(
    app.config.room.lights.filter((l) => !app.config.bindings.some((b) => b.light_id === l.id)).length,
  );
  // Automatic mode only exists where fullscreen can be detected (Windows).
  const waitingForFullscreen = $derived(
    /windows/i.test(navigator.userAgent) && app.config.behavior.auto_fullscreen && bound > 0,
  );
  const mod = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true">
        <rect x="2.5" y="4" width="15" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5" />
        <path d="M7.5 17h5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        <rect x="5" y="6.5" width="10" height="5" rx="0.5" fill="var(--glow)" />
      </svg>
      Lueur
    </div>

    <nav aria-label="Lueur">
      {#each nav as n (n.id)}
        <button
          class="nav-item"
          class:active={app.view === n.id}
          aria-current={app.view === n.id ? "page" : undefined}
          title="{n.label()} ({mod}{n.key})"
          onclick={() => (app.view = n.id)}
        >
          <Icon name={n.icon} size={16} />
          <span class="nav-label">{n.label()}</span>
          {#if n.id === "bulbs" && unbound > 0}
            <span class="badge" title={t("unbound_badge")}>{unbound}</span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="foot">
      <UpdateBanner />

      <div class="power" class:live={app.status.running}>
        <div class="state" role="status">
          <span class="dot" aria-hidden="true"></span>
          <div>
            <div class="state-title">
              {app.status.running ? t(`mode_${app.status.mode ?? "ambilight"}`) : t("state_off")}
            </div>
            <div class="state-sub">
              {#if app.status.running}
                {app.status.connected_lights === 0
                  ? t("state_no_answer")
                  : tn("n_bulbs_following", app.status.connected_lights)}
              {:else if bound === 0}
                {t("state_none")}
              {:else if waitingForFullscreen}
                {t("auto_waiting")}
              {:else}
                {tn("n_bulbs_ready", bound)}
              {/if}
            </div>
          </div>
        </div>
        <button
          class="btn power-btn"
          class:primary={!app.status.running}
          disabled={app.busy || (!app.status.running && bound === 0)}
          title="{app.status.running ? t('stop') : t('start')} ({mod}Enter)"
          onclick={toggleEffect}
        >
          <Icon name="power" size={14} />
          {app.busy ? t("wait") : app.status.running ? t("stop") : t("start")}
        </button>
      </div>
    </div>
  </aside>

  <main class="content">
    {#key app.view}
      <div class="view view-enter">
        {#if app.view === "lighting"}
          <Lighting />
        {:else if app.view === "room"}
          <Room />
        {:else if app.view === "bulbs"}
          <Bulbs />
        {:else}
          <Settings />
        {/if}
      </div>
    {/key}
  </main>

  <Toasts />
</div>

<style>
  .shell {
    display: flex;
    height: 100vh;
  }

  .sidebar {
    width: 200px;
    flex: none;
    display: flex;
    flex-direction: column;
    padding: var(--s-3) var(--s-2);
    background: var(--sidebar);
    border-right: 1px solid var(--line);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-1) var(--s-2) var(--s-4);
    font-size: var(--t-lg);
    font-weight: 600;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    height: 30px;
    padding: 0 var(--s-2);
    border-radius: var(--r-md);
    color: var(--text-2);
    text-align: left;
  }
  .nav-item:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--surface);
    box-shadow: var(--card-edge);
    color: var(--text);
    font-weight: 500;
  }
  .nav-item.active :global(svg) {
    color: var(--glow-text);
  }
  .nav-label {
    flex: 1;
  }
  .badge {
    min-width: 18px;
    height: 16px;
    padding: 0 5px;
    border-radius: var(--r-full);
    background: var(--line-strong);
    color: var(--text);
    font-size: var(--t-xs);
    font-weight: 600;
    line-height: 16px;
    text-align: center;
  }

  .foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
  }

  .power {
    position: relative;
    padding: var(--s-3);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
    overflow: hidden;
    isolation: isolate;
  }
  /* When the light is on, a little of it spills into the panel. */
  .power::before {
    content: "";
    position: absolute;
    inset: -40% -20% auto;
    height: 120%;
    z-index: -1;
    background: radial-gradient(closest-side, var(--glow-soft), transparent);
    opacity: 0;
    transition: opacity 600ms var(--ease);
  }
  .power.live::before {
    opacity: 1;
  }
  .state {
    display: flex;
    align-items: flex-start;
    gap: var(--s-2);
  }
  .dot {
    width: 8px;
    height: 8px;
    margin-top: 5px;
    flex: none;
    border-radius: 50%;
    background: var(--line-strong);
  }
  .live .dot {
    background: var(--glow);
    box-shadow: 0 0 6px var(--glow);
  }
  .state-title {
    font-weight: 600;
  }
  .state-sub {
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
    scrollbar-gutter: stable;
  }
  .view {
    height: 100%;
  }
</style>
