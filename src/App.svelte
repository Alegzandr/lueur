<script lang="ts">
  import { onMount } from "svelte";
  import { app, bootstrap, fail, notify, refreshStatus, reloadStates, sleepChanged, toggleEffect, type View } from "./lib/state.svelte";
  import { lang, t } from "./lib/i18n";
  import { onAutoStarted, onEffectChanged, onLightsChanged, onSleepChanged, onUpdateReady, setUiLanguage, setWindowTheme, updateReady } from "./lib/ipc";
  import { MOD_KEY, OWN_TITLEBAR } from "./lib/platform";
  import Icon, { type IconName } from "./lib/ui/Icon.svelte";
  import Logo from "./lib/ui/Logo.svelte";
  import TitleBar from "./lib/ui/TitleBar.svelte";
  import Toasts from "./lib/ui/Toasts.svelte";
  import Backdrop from "./lib/ui/Backdrop.svelte";
  import RoomBackdrop from "./lib/views/RoomBackdrop.svelte";
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

  // True while a view plays its entrance animation. Set before the new view
  // reaches the page: waiting for its animationstart would let one frame
  // paint the overhang with a scrollbar.
  let settling = $state(false);
  $effect.pre(() => {
    void app.view;
    settling = true;
  });

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
      // A game went fullscreen and the ambilight took over by itself.
      onAutoStarted(() => notify("info", t("auto_started"))),
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
  <!-- Behind the lighting, what the bulbs do; behind the other pages, the
       room as they make it. Under the whole window, the navigation too,
       and outside the scroll, so they stay put. -->
  {#if app.view === "lighting"}<Backdrop />{:else}<RoomBackdrop />{/if}

  <aside class="sidebar">
    <div class="brand" data-tauri-drag-region={OWN_TITLEBAR || undefined}>
      <Logo size={22} />
      <span class="brand-name">Lueur</span>
    </div>

    <!-- Settings sit apart at the foot, away from the views used day to day. -->
    <nav aria-label="Lueur">
      {#each nav as n (n.id)}
        <button
          class="nav-item"
          class:active={app.view === n.id}
          class:foot={n.id === "settings"}
          aria-current={app.view === n.id ? "page" : undefined}
          title="{n.label()} ({MOD_KEY}{n.key})"
          onclick={() => (app.view = n.id)}
        >
          <Icon name={n.icon} size={17} filled={app.view === n.id} />
          <span class="nav-label">{n.label()}</span>
          {#if n.id === "bulbs" && unbound > 0}
            <span class="badge" title={t("unbound_badge")}>{unbound}</span>
          {/if}
        </button>
      {/each}
    </nav>
  </aside>

  <main class="content">
    {#if OWN_TITLEBAR}
      <TitleBar />
    {:else if app.updateReady}
      <!-- Only there while an installed update waits for a restart, so the views keep their full height otherwise. -->
      <div class="topbar"><UpdateButton /></div>
    {/if}
    <div class="scroll" class:settling>
      {#key app.view}
        <div
          class="view view-enter"
          onanimationend={(e) => e.target === e.currentTarget && (settling = false)}
        >
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
    position: relative;
    isolation: isolate;
    display: flex;
    height: 100vh;
  }
  .shell > :global(.backdrop),
  .shell > :global(.room-backdrop) {
    z-index: -1;
  }

  .sidebar {
    width: 200px;
    flex: none;
    display: flex;
    flex-direction: column;
    padding: var(--s-3) var(--s-2);
    /* Frosted, like the cards: the room's light reaches the navigation too,
       softened, instead of stopping at a grey wall. */
    background: var(--sidebar-material);
    -webkit-backdrop-filter: var(--material-filter);
    backdrop-filter: var(--material-filter);
    box-shadow: 1px 0 0 var(--separator);
    transition: width 220ms var(--ease);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-1) var(--s-2) var(--s-4);
    font-size: var(--t-lg);
    font-weight: 600;
  }
  /* The mark is wider than the nav icons: centred on their column, it takes
     no more room than they do, so the name lines up with the labels. */
  .brand :global(svg) {
    margin-inline: -3px;
  }
  /* The whole row drags the window, logo included. */
  .brand[data-tauri-drag-region] :global(svg) {
    pointer-events: none;
  }

  nav {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--s-1);
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    height: 36px;
    padding: 0 var(--s-2);
    border-radius: var(--r-md);
    color: var(--text-2);
    text-align: left;
    transition:
      background-color var(--dur) var(--ease),
      color var(--dur) var(--ease);
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

  .nav-item.foot {
    margin-top: auto;
  }

  /* A narrow window keeps its width for the page: the navigation folds to
     its icons. Each keeps its name, in its tooltip and for screen readers,
     and the count of bulbs without a place becomes a dot on its icon. */
  @media (max-width: 1100px) {
    .sidebar {
      width: 60px;
    }
    .brand {
      justify-content: center;
      padding-inline: 0;
    }
    .brand-name,
    .nav-label {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .nav-item {
      position: relative;
      justify-content: center;
      padding: 0;
    }
    .badge {
      position: absolute;
      top: 3px;
      right: 4px;
      min-width: 15px;
      height: 15px;
      padding: 0 4px;
      font-size: 9.5px;
      line-height: 15px;
      box-shadow: 0 0 0 2px var(--sidebar);
    }
  }

  .content {
    position: relative;
    isolation: isolate;
    overflow: hidden;
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
  /* The entering view slides up from a few pixels low; without this, that
     brief overhang flashes a scrollbar. */
  .scroll.settling {
    overflow: hidden;
  }
  .view {
    height: 100%;
  }
</style>
