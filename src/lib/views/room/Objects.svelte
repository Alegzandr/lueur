<script lang="ts">
  import { app, edit, bindingFor } from "../../state.svelte";
  import { mapLight, EDGE_KIND, EDGE_LETTER } from "../../geometry";
  import { t } from "../../i18n";
  import { WALLS, rgbCss, rgbOfHex } from "../../scenes";
  import type { Rgb } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import ZoneBadge from "../../ui/ZoneBadge.svelte";
  import { EDGES, cornerName, type Other } from "./plan";

  // The inspector with nothing selected: every object on the plan, the
  // wall paint, and what the zone colours mean.

  let {
    onpick,
  }: {
    /** A row was picked: the inspector opens on it. */
    onpick: (e: Event, o: { light: string } | Other) => void;
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

<h2 class="side-title" id="objects-title">{t("objects")}</h2>
<ul class="objects" aria-labelledby="objects-title">
  <li>
    <button class="obj" data-key="screen" onclick={(e) => onpick(e, { kind: "screen" })}>
      <Icon name="screen" size={15} />
      <span class="obj-name">{t("plan_screen")}</span>
      <span class="obj-meta num">{scr.facing_deg}°</span>
    </button>
  </li>
  {#each room.lights as l (l.id)}
    {@const m = mapLight(scr, l)}
    <li>
      <button class="obj" class:off={!l.enabled} data-key="light:{l.id}" onclick={(e) => onpick(e, { light: l.id })}>
        <ZoneBadge edge={m.edge} off={!l.enabled} />
        <span class="obj-name"
          >{l.name}<span class="sr-only"
            >, {t("zone_of", { letter: t(EDGE_LETTER[m.edge]), zone: t(EDGE_KIND[m.edge]) })}{#if !l.enabled}, {t(
                "not_following",
              )}{/if}</span
          ></span
        >
        {#if !bindingFor(l.id)}<span class="obj-warn">{t("no_bulb")}</span>{/if}
      </button>
    </li>
  {/each}
</ul>
{#if room.lights.length === 0}
  <p class="empty">{t("no_lights")}</p>
{/if}

<h2 class="side-title corners-title" id="corners-title">{t("corners")}</h2>
<ul class="objects" aria-labelledby="corners-title">
  {#each room.walls as w, i}
    <li>
      <button class="obj" data-key="corner:{i}" onclick={(e) => onpick(e, { kind: "corner", index: i })}>
        <span class="obj-corner" aria-hidden="true"></span>
        <span class="obj-name">{cornerName(i)}</span>
        <span class="obj-meta num" aria-hidden="true">{w.x}, {w.y}</span>
      </button>
    </li>
  {/each}
</ul>

<div class="paint">
  <h2 class="side-title" id="walls-title">{t("walls")}</h2>
  <div class="walls" role="radiogroup" aria-labelledby="walls-title" tabindex="-1" onkeydown={onWallKey}>
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
  <p class="paint-tip">{t("walls_tip")}</p>
</div>

<div class="legend">
  {#each EDGES as edge}
    <span><ZoneBadge {edge} />{t(EDGE_KIND[edge])}</span>
  {/each}
</div>

<style>
  .side-title {
    font-size: var(--t-sm);
    font-weight: 600;
    color: var(--text-3);
    margin: 0 0 var(--s-2) 2px;
  }
  .objects {
    list-style: none;
    margin: 0 -6px;
    padding: 0;
  }
  .obj {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border-radius: var(--r-sm);
    text-align: left;
    color: var(--text-3);
  }
  .obj:hover {
    background: var(--surface-hover);
  }
  .obj-name {
    flex: 1;
    min-width: 0;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .obj.off .obj-name {
    color: var(--text-3);
  }
  .obj :global(.zone-badge) {
    margin: 0 0.5px;
  }
  /* A corner as the plan draws it. */
  .obj-corner {
    width: 9px;
    height: 9px;
    margin: 0 3px;
    flex: none;
    border-radius: 2px;
    background: var(--plan-floor);
    box-shadow: inset 0 0 0 1.5px var(--plan-wall);
  }
  .corners-title {
    margin-top: var(--s-4);
  }
  .obj-meta {
    font-size: var(--t-sm);
  }
  .obj-warn {
    font-size: var(--t-xs);
    color: var(--glow-text);
  }
  .empty {
    font-size: var(--t-sm);
    color: var(--text-3);
    margin: var(--s-2) 2px;
  }
  .paint {
    margin-top: var(--s-5);
    padding-top: var(--s-4);
    border-top: 1px solid var(--line);
  }
  .walls {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
    padding: 2px;
  }
  .wall {
    position: relative;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 0.18);
    transition: transform var(--dur) var(--ease);
  }
  .wall:hover {
    transform: scale(1.08);
  }
  .wall.picked {
    box-shadow:
      inset 0 0 0 1px oklch(0 0 0 / 0.18),
      0 0 0 2px var(--surface),
      0 0 0 3.5px var(--text);
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
  .paint-tip {
    font-size: var(--t-xs);
    color: var(--text-3);
    margin: var(--s-2) 2px 0;
    line-height: 1.4;
  }
  .legend {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: auto;
    padding-top: var(--s-5);
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
</style>
