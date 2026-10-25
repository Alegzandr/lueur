<script lang="ts">
  import { onMount } from "svelte";
  import { app, bootstrap, fail, refreshStatus, reloadStates, sleepChanged, toggleEffect, type View } from "./lib/state.svelte";
  import { lang, t, tn } from "./lib/i18n";
  import { onEffectChanged, onLightsChanged, onSleepChanged, onUpdateReady, setUiLanguage, setWindowTheme, updateReady } from "./lib/ipc";
  import { MOD_KEY } from "./lib/platform";
  import Icon, { type IconName } from "./lib/ui/Icon.svelte";
  import Logo from "./lib/ui/Logo.svelte";
  import Toasts from "./lib/ui/Toasts.svelte";
  import UpdateButton from "./lib/ui/UpdateButton.svelte";
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
    // Hidden in the tray, nobody reads the status: skip the poll, and catch
    // up the moment the window shows again.
    const poll = setInterval(() => {
      if (!document.hidden) void refreshStatus();
    }, 4000);
    const onVisible = () => {
      if (!document.hidden) void refreshStatus();
    };
    document.addEventListener("visibilitychange", onVisible);
    updateReady()
      .then((v) => (app.updateReady = v))
      .catch(() => {});
    const subscriptions = [
      // Started or stopped from the tray: update now rather than at the next poll.
      onEffectChanged((error) => {
        if (error) fail(error);
        void refreshStatus();
      }),
      // The tray or automatic mode changed what the lights show.
      onLightsChanged(() => void reloadStates()),
      // The sleep timer was set from the tray, or ran out.
      onSleepChanged(sleepChanged),
      // An update installed in the background waits for a restart.
      onUpdateReady((v) => (app.updateReady = v)),
    ];
    return () => {
      clearInterval(poll);
      document.removeEventListener("visibilitychange", onVisible);
      // Also unsubscribes a listener still being set up.
      for (const s of subscriptions) void s.then((unlisten) => unlisten());
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

  const unbound = $derived(
    app.config.room.lights.filter((l) => !app.config.bindings.some((b) => b.light_id === l.id)).length,
  );
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <Logo size={22} />
      Lueur
    </div>

    <nav aria-label="Lueur">
      {#each nav as n (n.id)}
        <button
          class="nav-item"
          class:active={app.view === n.id}
          aria-current={app.view === n.id ? "page" : undefined}
          title="{n.label()} ({MOD_KEY}{n.key})"
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
      <!-- Effects start from the Lighting view; this only shows one running, from any view. -->
      {#if app.status.running}
        <div class="power">
          <div class="state" role="status">
            <span class="lamp" aria-hidden="true"></span>
            <div>
              <div class="state-title">{t(`mode_${app.status.mode ?? "mixed"}`)}</div>
              <div class="state-sub">
                {app.status.connected_lights === 0
                  ? t("state_no_answer")
                  : tn("n_bulbs_following", app.status.connected_lights)}
              </div>
            </div>
          </div>
          <button
            class="btn power-btn"
            disabled={app.busy}
            title="{t('stop')} ({MOD_KEY}Enter)"
            onclick={toggleEffect}
          >
            <Icon name="power" size={14} />
            {app.busy ? t("wait") : t("stop")}
          </button>
        </div>
      {/if}
    </div>
  </aside>

  <main class="content">
    <!-- Only there while an installed update waits for a restart, so the views keep their full height otherwise. -->
    {#if app.updateReady}
      <div class="topbar"><UpdateButton /></div>
    {/if}
    <div class="scroll">
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
    </div>
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
  }
  .state {
    display: flex;
    align-items: flex-start;
    gap: var(--s-2);
  }
  .lamp {
    --lamp: 8px;
    margin-top: 5px;
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
    display: flex;
    flex-direction: column;
  }
  .topbar {
    display: flex;
    justify-content: flex-end;
    flex: none;
    padding: var(--s-2) var(--s-3) 0;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
    scrollbar-gutter: stable;
  }
  .view {
    height: 100%;
  }
</style>
