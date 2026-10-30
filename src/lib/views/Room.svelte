<script lang="ts">
  import { app, edit, addLight, lightById, notify, markRoomArranged, removeLight } from "../state.svelte";
  import { tick } from "svelte";
  import { history, observe, planKey, redo, track, undo } from "../history.svelte";
  import { mapLight, EDGE_KIND, EDGE_LETTER } from "../geometry";
  import { t } from "../i18n";
  import { MOD_KEY } from "../platform";
  import { defaultConfig, type Vec2 } from "../types";
  import Icon from "../ui/Icon.svelte";
  import RestartBanner from "../ui/RestartBanner.svelte";
  import Inspector from "./room/Inspector.svelte";
  import Objects from "./room/Objects.svelte";
  import Plan from "./room/Plan.svelte";
  import { clampP, cornerName, type Other } from "./room/plan";

  const DEF = defaultConfig();

  // Selection: a light lives in shared state (other views link to it); the
  // screen and corners are only selectable here.
  let other = $state<Other>(null);

  const selLight = $derived(app.selectedLight ? lightById(app.selectedLight) : undefined);

  /** True while something on the plan is being dragged. */
  let dragging = $state(false);

  /** The list of corners stays unfolded while one of them is selected, so
   *  going back lands on its row. */
  let cornersOpen = $state(false);

  // --- Focus ---
  // Picking from the list, or adding, moves the focus to the inspector; going
  // back returns it to where it came from, or to the object's row in the list.
  // A pick on the plan with the pointer leaves the focus alone.

  let inspectorEl = $state<HTMLElement>();
  /** The control the inspector was opened from, when it outlives it. */
  let origin: HTMLElement | null = null;

  /** The list row of what is selected now, e.g. "light:<id>" or "corner:2". */
  function selectedKey(): string | null {
    if (selLight) return `light:${selLight.id}`;
    if (other?.kind === "screen") return "screen";
    if (other?.kind === "corner") return `corner:${other.index}`;
    return null;
  }

  async function focusInspector() {
    await tick();
    inspectorEl?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }

  /** Only take the focus back from the inspector or from nowhere, never
   *  from a control the user has since moved to. */
  function focusIsOurs(): boolean {
    const a = document.activeElement;
    return !a || a === document.body || !!inspectorEl?.contains(a);
  }

  /** After the inspector closes: back to the origin, else to the row `key`,
   *  else to the row now at `index` (what took a removed object's place). */
  async function focusList(key: string | null, index = 0) {
    const from = origin;
    origin = null;
    await tick();
    if (from?.isConnected) return from.focus();
    const rows = [...(inspectorEl?.querySelectorAll<HTMLElement>("[data-key]") ?? [])];
    const row = rows.find((r) => r.dataset.key === key) ?? rows[Math.min(Math.max(index, 0), rows.length - 1)];
    row?.focus();
  }

  function selectLight(id: string, focus = false) {
    app.selectedLight = id;
    other = null;
    if (focus) focusInspector();
  }
  function selectOther(o: Other, focus = false) {
    app.selectedLight = null;
    other = o;
    if (o?.kind === "corner") cornersOpen = true;
    if (focus) focusInspector();
  }
  /** A pick from the list or an add button: the focus follows it. */
  function pick(e: Event, o: { light: string } | Other) {
    const el = e.currentTarget as HTMLElement;
    // List rows go away with the list; they are found again by their key.
    origin = el.dataset.key ? null : el;
    if (o && "light" in o) selectLight(o.light, true);
    else selectOther(o, true);
  }
  function clearSelection(restore = true) {
    const key = selectedKey();
    const ours = restore && key !== null && focusIsOurs();
    app.selectedLight = null;
    other = null;
    if (ours) focusList(key);
    else origin = null;
  }

  // --- Editing ---

  function newLight(e: Event) {
    const l = addLight();
    pick(e, { light: l.id });
  }

  /** Toolbar version of a double-click on a wall: split the longest wall in two. */
  function addCorner(e: Event) {
    const w = app.config.room.walls;
    let best = 0;
    let bestLen = -1;
    for (let i = 0; i < w.length; i++) {
      const a = w[i];
      const b = w[(i + 1) % w.length];
      const len = Math.hypot(b.x - a.x, b.y - a.y);
      if (len > bestLen) {
        bestLen = len;
        best = i;
      }
    }
    edit((c) => {
      const a = c.room.walls[best];
      const b = c.room.walls[(best + 1) % c.room.walls.length];
      c.room.walls.splice(best + 1, 0, { x: Math.round((a.x + b.x) / 2), y: Math.round((a.y + b.y) / 2) });
    });
    pick(e, { kind: "corner", index: best + 1 });
  }

  function removeCorner(index: number) {
    if (app.config.room.walls.length <= 3) {
      notify("info", t("min_corners"));
      return;
    }
    const ours = focusIsOurs();
    const row = rowIndex(`corner:${index}`);
    edit((c) => {
      c.room.walls.splice(index, 1);
    });
    other = null;
    if (ours) focusList(null, row);
  }

  function removeSelectedLight(id: string) {
    const ours = focusIsOurs();
    const row = rowIndex(`light:${id}`);
    removeLight(id);
    if (ours) focusList(null, row);
  }

  function resetShape() {
    edit((c) => {
      c.room.walls = structuredClone(DEF.room.walls);
      c.room.screen = structuredClone(DEF.room.screen);
    });
    clearSelection(false);
    notify("info", t("reset_done"));
  }

  function removeSelected() {
    if (selLight) removeSelectedLight(selLight.id);
    else if (other?.kind === "corner") removeCorner(other.index);
  }

  function nudge(dx: number, dy: number) {
    edit((c) => {
      if (selLight) {
        const l = c.room.lights.find((x) => x.id === selLight.id);
        if (l) l.pos = clampP({ x: l.pos.x + dx, y: l.pos.y + dy });
      } else if (other?.kind === "screen") {
        c.room.screen.center = clampP({ x: c.room.screen.center.x + dx, y: c.room.screen.center.y + dy });
      } else if (other?.kind === "corner") {
        const i = other.index;
        const p = c.room.walls[i];
        if (p) c.room.walls[i] = clampP({ x: p.x + dx, y: p.y + dy });
      }
    });
    announceSoon();
  }

  // --- Typed positions (see `Position`) ---

  function selectedPos(): Vec2 | undefined {
    if (selLight) return lightById(selLight.id)?.pos;
    if (other?.kind === "screen") return app.config.room.screen.center;
    if (other?.kind === "corner") return app.config.room.walls[other.index];
  }

  function setPos(axis: "x" | "y", v: number) {
    edit((c) => {
      const put = (p: Vec2): Vec2 => clampP({ ...p, [axis]: v });
      if (selLight) {
        const l = c.room.lights.find((x) => x.id === selLight.id);
        if (l) l.pos = put(l.pos);
      } else if (other?.kind === "screen") {
        c.room.screen.center = put(c.room.screen.center);
      } else if (other?.kind === "corner") {
        const p = c.room.walls[other.index];
        if (p) c.room.walls[other.index] = put(p);
      }
    });
    // Placing a light or the screen is arranging the room, as a drag is.
    if (selLight || other?.kind === "screen") markRoomArranged();
  }

  // --- Announcements ---
  // Moves made with the arrows are read out once the keys rest, so holding
  // an arrow gives one message, not one per step.

  let announcement = $state("");
  let announceTimer: ReturnType<typeof setTimeout> | null = null;

  function announceSoon() {
    if (announceTimer) clearTimeout(announceTimer);
    announceTimer = setTimeout(() => {
      announceTimer = null;
      announcement = whereNow();
    }, 600);
  }
  $effect(() => () => {
    if (announceTimer) clearTimeout(announceTimer);
  });

  function whereNow(): string {
    const p = selectedPos();
    if (!p) return "";
    if (selLight) {
      const m = mapLight(app.config.room.screen, selLight);
      return t("moved_zone", {
        name: selLight.name,
        x: p.x,
        y: p.y,
        letter: t(EDGE_LETTER[m.edge]),
        zone: t(EDGE_KIND[m.edge]),
      });
    }
    const name = other?.kind === "corner" ? cornerName(other.index) : t("plan_screen");
    return t("moved", { name, x: p.x, y: p.y });
  }

  /** Where an object's row sits in the list, to land on its neighbour. */
  function rowIndex(key: string): number {
    const { lights, walls } = app.config.room;
    return [...lights.map((l) => `light:${l.id}`), "screen", ...walls.map((_, i) => `corner:${i}`)].indexOf(key);
  }

  // --- History ---

  track();
  // Every change to the plan feeds the history; a drag is one step.
  $effect(() => observe(planKey(), dragging));

  /** Undo or redo, then let go of what the step took away. */
  function travel(dir: "undo" | "redo") {
    if (dir === "undo") undo();
    else redo();
    if (app.selectedLight && !lightById(app.selectedLight)) app.selectedLight = null;
    if (other?.kind === "corner" && other.index >= app.config.room.walls.length) other = null;
  }

  function onKey(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    // Text fields keep their own undo and arrows; Escape still closes the
    // inspector they sit in.
    if (t.closest("input, select, textarea")) {
      if (e.key === "Escape" && inspectorEl?.contains(t) && (selLight || other)) {
        e.preventDefault();
        clearSelection();
      }
      return;
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !dragging) {
      const key = e.key.toLowerCase();
      if (key === "z" || key === "y") {
        e.preventDefault();
        travel(key === "y" || e.shiftKey ? "redo" : "undo");
      }
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const step = e.shiftKey ? 25 : 5;
    switch (e.key) {
      case "Escape":
        clearSelection();
        break;
      case "Delete":
      case "Backspace":
        if (!selLight && other?.kind !== "corner") return;
        removeSelected();
        break;
      case "ArrowLeft":
        nudge(-step, 0);
        break;
      case "ArrowRight":
        nudge(step, 0);
        break;
      case "ArrowUp":
        nudge(0, -step);
        break;
      case "ArrowDown":
        nudge(0, step);
        break;
      default:
        return;
    }
    if (selLight || other) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="room">
  <header class="bar">
    <h1 class="page-title">{t("nav_room")}</h1>
    <!-- Adding and resetting live in the column beside the plan, next to
         what they act on; only the history stays up here. -->
    <div class="btn-group">
      <button
        class="btn icon"
        title="{t('undo')} ({MOD_KEY}Z)"
        aria-label={t("undo")}
        disabled={!history.canUndo}
        onclick={() => travel("undo")}><Icon name="undo" size={15} /></button
      >
      <button
        class="btn icon"
        title="{t('redo')} ({MOD_KEY === '⌘' ? '⇧⌘Z' : 'Ctrl+Y'})"
        aria-label={t("redo")}
        disabled={!history.canRedo}
        onclick={() => travel("redo")}><Icon name="redo" size={15} /></button
      >
    </div>
  </header>

  <section class="stage-col">
    {#if !app.config.interface.room_arranged && app.config.room.lights.length > 0}
      <div class="coach" role="status">
        <span class="lamp" aria-hidden="true"></span>
        <p>{t("coach")}</p>
        <button class="btn primary" onclick={markRoomArranged}>{t("coach_done")}</button>
      </div>
    {/if}
    <div class="banner-slot"><RestartBanner /></div>

    <Plan
      {other}
      bind:dragging
      onselectlight={(id) => selectLight(id)}
      onselectother={(o) => selectOther(o)}
      onclear={() => clearSelection(false)}
      ongrab={() => (origin = null)}
    />
  </section>

  <aside class="inspector" bind:this={inspectorEl}>
    {#if selLight || other}
      <Inspector
        light={selLight}
        {other}
        onback={() => clearSelection()}
        onremovelight={removeSelectedLight}
        onremovecorner={removeCorner}
        onset={setPos}
      />
    {:else}
      <Objects onpick={pick} onaddlight={newLight} onaddcorner={addCorner} onreset={resetShape} bind:cornersOpen />
    {/if}
  </aside>

  <!-- Where a light moved with the arrows went, for screen readers. -->
  <p class="sr-only" aria-live="polite">{announcement}</p>
</div>

<style>
  /* The plan and the column beside it sit in the page like the cards of the
     other pages, under one title and on one top line: the column is not a
     panel against the window's edge. The title and both edges stand where
     every other page has its own, so nothing jumps when the page changes. */
  .room {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 296px;
    grid-template-rows: auto minmax(0, 1fr);
    gap: var(--s-3) var(--s-5);
    height: 100%;
    padding: var(--s-6) var(--s-7) var(--s-5);
  }

  /* Stage */
  .stage-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    gap: var(--s-3);
  }
  .bar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-4);
    min-height: 34px;
  }
  .coach {
    display: flex;
    align-items: center;
    gap: var(--s-4);
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-4);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
  }
  .coach p {
    flex: 1;
  }
  .banner-slot:empty {
    display: none;
  }

  /* Inspector. It scrolls on its own when the window is short; the few
     pixels of padding keep the cards' edges and focus rings from being cut
     by that scroll box. */
  .inspector {
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    margin: -4px;
    padding: 4px;
    /* No bar squeezed against the cards: the wheel, the keyboard and the
       fades below say there is more. */
    scrollbar-width: none;
  }
  .inspector::-webkit-scrollbar {
    display: none;
  }

  /* The edge where more is hidden fades out, and only while there is more
     that way: the fades follow the scroll position, and a column that fits
     has no scroll timeline, so no fade. */
  @property --fade-top {
    syntax: "<length>";
    inherits: false;
    initial-value: 0px;
  }
  @property --fade-bottom {
    syntax: "<length>";
    inherits: false;
    initial-value: 0px;
  }
  @supports (animation-timeline: scroll()) {
    .inspector {
      mask-image: linear-gradient(
        to bottom,
        transparent,
        #000 var(--fade-top),
        #000 calc(100% - var(--fade-bottom)),
        transparent
      );
      animation: inspector-fades linear both;
      animation-timeline: scroll(self);
      /* This follows the scroll, it is not motion: the reduced-motion rule's
         short duration would freeze it at its end. */
      animation-duration: auto !important;
    }
  }
  @keyframes inspector-fades {
    0% {
      --fade-top: 0px;
      --fade-bottom: 32px;
    }
    6%,
    94% {
      --fade-top: 32px;
      --fade-bottom: 32px;
    }
    100% {
      --fade-top: 32px;
      --fade-bottom: 0px;
    }
  }
</style>
