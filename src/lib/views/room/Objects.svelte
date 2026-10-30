<script lang="ts">
  import { app, edit, bindingFor } from "../../state.svelte";
  import { mapLight, EDGE_KIND } from "../../geometry";
  import { t } from "../../i18n";
  import { WALLS, rgbCss, rgbOfHex } from "../../scenes";
  import type { Rgb } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import ZoneBadge from "../../ui/ZoneBadge.svelte";
  import { cornerName, type Other } from "./plan";

  // The inspector with nothing selected: what the room holds, in the order
  // it is set up. The lights first, each saying which part of the picture it
  // mirrors, so the letters on the plan read without a legend; then the
  // screen; then the walls, their paint and, folded away, their corners,
  // which are mostly dragged on the plan. Each part keeps its own "add".

  let {
    onpick,
    onaddlight,
    onaddcorner,
    onreset,
    cornersOpen = $bindable(false),
  }: {
    /** A row was picked: the inspector opens on it. */
    onpick: (e: Event, o: { light: string } | Other) => void;
    onaddlight: (e: Event) => void;
    onaddcorner: (e: Event) => void;
    /** Back to the room's first shape. */
    onreset: () => void;
    /** Whether the list of corners is unfolded; the room keeps it, so it
     *  stays open while one of them is being edited. */
    cornersOpen?: boolean;
  } = $props();

  const room = $derived(app.config.room);
  const scr = $derived(room.screen);

  const sameWall = (a: Rgb, b: Rgb) => a.r === b.r && a.g === b.g && a.b === b.b;
  /** The paint picked, or -1 for one of the user's own. */
  const wallIndex = $derived(WALLS.findIndex((w) => sameWall(app.config.room.wall, w.rgb)));
  const wallHex = $derived.by(() => {
    const { r, g, b } = app.config.room.wall;
    return "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
  });

  function setWall(rgb: Rgb) {
    edit((c) => (c.room.wall = { ...rgb }));
  }

  /** The wall swatches are a radio group: the arrows move the choice. */
  function onWallKey(e: KeyboardEvent & { currentTarget: HTMLElement }) {
    const last = WALLS.length - 1;
    const i = Math.max(0, wallIndex);
    const to =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? (i === last ? 0 : i + 1)
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? (i === 0 ? last : i - 1)
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? last
              : -1;
    if (to < 0) return;
    e.preventDefault();
    setWall(WALLS[to].rgb);
    e.currentTarget.querySelectorAll<HTMLButtonElement>("button")[to]?.focus();
  }
</script>

<section class="group" aria-labelledby="lights-title">
  <h2 class="group-title side-title" id="lights-title">
    {t("lights")}
    {#if room.lights.length > 0}<span class="count num">{room.lights.length}</span>{/if}
  </h2>
  <ul class="card list">
    {#each room.lights as l (l.id)}
      {@const m = mapLight(scr, l)}
      {@const bulb = bindingFor(l.id)}
      <li>
        <button class="item" class:off={!l.enabled} data-key="light:{l.id}" onclick={(e) => onpick(e, { light: l.id })}>
          <span class="mark"><ZoneBadge edge={m.edge} off={!l.enabled} /></span>
          <span class="text">
            <span class="name">{l.name}</span>
            <span class="meta">{l.enabled ? t(EDGE_KIND[m.edge]) : t("left_out")}</span>
          </span>
          {#if !bulb}
            <span class="waiting"><span class="lamp" aria-hidden="true"></span>{t("no_bulb")}</span>
          {/if}
          <span class="go" aria-hidden="true"><Icon name="chevron" size={12} /></span>
        </button>
      </li>
    {:else}
      <li class="empty">{t("no_lights")}</li>
    {/each}
    <li>
      <button class="item add" onclick={onaddlight}>
        <span class="mark"><Icon name="plus" size={14} /></span>
        <span class="name">{t("add_light")}</span>
      </button>
    </li>
  </ul>
</section>

<section class="group" aria-labelledby="screen-title">
  <h2 class="group-title side-title" id="screen-title">{t("plan_screen")}</h2>
  <div class="card list">
    <button class="item" data-key="screen" onclick={(e) => onpick(e, { kind: "screen" })}>
      <span class="mark"><Icon name="screen" size={16} /></span>
      <span class="text">
        <span class="name">{t("plan_screen")}</span>
        <span class="meta num">{t("screen_meta", { w: Math.round(scr.width), deg: scr.facing_deg })}</span>
      </span>
      <span class="go" aria-hidden="true"><Icon name="chevron" size={12} /></span>
    </button>
  </div>
</section>

<section class="group" aria-labelledby="walls-title">
  <h2 class="group-title side-title" id="walls-title">{t("walls_title")}</h2>
  <div class="card">
    <div class="paint">
      <div class="walls" role="radiogroup" aria-label={t("walls")} tabindex="-1" onkeydown={onWallKey}>
        {#each WALLS as w, i (w.id)}
          {@const picked = i === wallIndex}
          <button
            class="wall"
            class:picked
            role="radio"
            aria-checked={picked}
            tabindex={i === Math.max(0, wallIndex) ? 0 : -1}
            title={t(`wall_${w.id}`)}
            aria-label={t(`wall_${w.id}`)}
            style="--c:{rgbCss([w.rgb.r, w.rgb.g, w.rgb.b])}"
            onclick={() => setWall(w.rgb)}
          ></button>
        {/each}
        <label
          class="wall any"
          class:picked={wallIndex < 0}
          title={t("wall_custom")}
          style={wallIndex < 0 ? `--c:${wallHex}` : undefined}
        >
          <input
            type="color"
            value={wallHex}
            aria-label={t("wall_custom")}
            oninput={(e) => {
              const c = rgbOfHex(e.currentTarget.value);
              if (c) setWall({ r: c[0], g: c[1], b: c[2] });
            }}
          />
        </label>
      </div>
      <p class="tip">{t("walls_tip")}</p>
    </div>

    <details class="corners" bind:open={cornersOpen}>
      <summary class="item">
        <span class="mark"><span class="corner" aria-hidden="true"></span></span>
        <span class="name">{t("corners")}</span>
        <span class="tally num">{room.walls.length}</span>
        <span class="go fold" aria-hidden="true"><Icon name="chevron" size={12} /></span>
      </summary>
      <ul class="list" aria-label={t("corners")}>
        {#each room.walls as w, i}
          <li>
            <button class="item sub" data-key="corner:{i}" onclick={(e) => onpick(e, { kind: "corner", index: i })}>
              <span class="name">{cornerName(i)}</span>
              <span class="tally num" aria-hidden="true">{w.x}, {w.y}</span>
            </button>
          </li>
        {/each}
        <li class="actions">
          <button class="btn plain sm" onclick={onaddcorner}><Icon name="plus" size={13} />{t("add_corner")}</button>
          <button class="btn plain sm" onclick={onreset}><Icon name="reset" size={13} />{t("reset_shape")}</button>
        </li>
      </ul>
    </details>
  </div>
</section>

<style>
  .group + .group {
    margin-top: var(--s-5);
  }
  .side-title {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
    margin: 0 0 var(--s-2) var(--s-1);
  }
  .count {
    text-shadow: none;
    min-width: 18px;
    height: 16px;
    padding: 0 5px;
    border-radius: var(--r-full);
    background: var(--surface-hover);
    color: var(--text-2);
    font-size: var(--t-xs);
    font-weight: 600;
    line-height: 16px;
    text-align: center;
  }

  /* A card of rows, as on the platforms' own inspectors: a hairline between
     rows that starts at their text, not at the card's edge. */
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .card {
    padding: 3px;
  }
  .list > li + li > .item,
  .list > li + li.actions {
    position: relative;
  }
  .list > li + li > .item::before,
  .list > li + li.actions::before {
    content: "";
    position: absolute;
    top: 0;
    left: 40px;
    right: 8px;
    border-top: 1px solid var(--separator);
  }
  /* The rule hides next to a row under the pointer, as a pressed row
     joins its neighbours. */
  .list > li:hover + li > .item::before,
  .list > li > .item:hover::before {
    opacity: 0;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 44px;
    padding: 5px 8px 5px 6px;
    /* Concentric with the card around it. */
    border-radius: calc(var(--r-lg) - 3px);
    text-align: left;
    color: var(--text-3);
    cursor: default;
    transition: background var(--dur) var(--ease);
  }
  .item:hover {
    background: var(--surface-hover);
  }
  .item:active {
    background: color-mix(in oklab, var(--surface-hover), var(--text) 4%);
  }
  .mark {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 8px;
    color: var(--text-2);
  }
  .item:not(.add) .mark {
    background: var(--surface-2);
    box-shadow: inset 0 0 0 0.5px var(--separator);
  }
  /* The zone mark is the plan's pin, a little larger, so the two read as one. */
  .mark :global(.zone-badge) {
    width: 18px;
    height: 18px;
    font-size: 10.5px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-weight: 500;
  }
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .item.off .name {
    color: var(--text-2);
  }
  .waiting {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: none;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--glow-text);
  }
  .waiting .lamp {
    --lamp: 6px;
  }
  .go {
    display: grid;
    flex: none;
    color: var(--text-3);
    opacity: 0.7;
    transition:
      opacity var(--dur) var(--ease),
      transform 220ms var(--ease);
  }
  .item:hover .go {
    opacity: 1;
  }

  .item.add {
    min-height: 38px;
    color: var(--text-2);
  }
  .item.add .name {
    color: var(--text-2);
  }
  .item.add:hover .name,
  .item.add:hover .mark {
    color: var(--text);
  }
  .empty {
    padding: var(--s-3) var(--s-3) var(--s-2) 42px;
    font-size: var(--t-sm);
    color: var(--text-3);
  }

  /* Walls: the paint, then the corners folded away. */
  .paint {
    padding: var(--s-3) var(--s-3) var(--s-3) var(--s-3);
  }
  .walls {
    display: flex;
    justify-content: space-between;
    gap: 6px;
  }
  .wall {
    position: relative;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.16);
    transition:
      transform var(--dur) var(--ease),
      box-shadow var(--dur) var(--ease);
  }
  .wall:hover {
    transform: scale(1.08);
  }
  .wall.picked {
    box-shadow:
      inset 0 0 0 1px oklch(0 0 0 / 0.16),
      0 0 0 2px var(--surface),
      0 0 0 3.5px var(--select);
  }
  /* The user's own paint: a spectrum until one is picked, then that paint. */
  .wall.any {
    background: var(--c, conic-gradient(red, yellow, lime, cyan, blue, magenta, red));
    cursor: pointer;
  }
  .wall.any input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
  }
  .wall.any:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .tip {
    margin-top: var(--s-3);
    font-size: var(--t-xs);
    color: var(--text-3);
    line-height: 1.45;
    text-wrap: pretty;
  }

  .corners {
    position: relative;
  }
  .corners::before {
    content: "";
    position: absolute;
    top: 0;
    left: 8px;
    right: 8px;
    border-top: 1px solid var(--separator);
  }
  summary {
    list-style: none;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary .name {
    flex: 1;
  }
  .fold {
    transform: rotate(90deg);
  }
  .corners[open] .fold {
    transform: rotate(-90deg);
  }
  .tally {
    flex: none;
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  /* A corner as the plan draws it. */
  .corner {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    background: var(--plan-floor);
    box-shadow: inset 0 0 0 1.5px var(--plan-wall);
  }
  .item.sub {
    min-height: 34px;
    padding-left: 42px;
  }
  .item.sub .name {
    flex: 1;
    font-weight: 400;
  }
  .corners .list > li + li > .item::before,
  .corners .list > li + li.actions::before {
    left: 42px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 6px 4px 4px 34px;
  }
  .actions .btn {
    color: var(--text-2);
  }
</style>
