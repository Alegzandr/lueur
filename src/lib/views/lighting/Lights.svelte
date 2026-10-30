<script lang="ts">
  import {
    app,
    bindingFor,
    brightOf,
    drivenElsewhere,
    isOn,
    isReconnecting,
    modeOf,
    takeLight,
    toggleLightPower,
    toggleTarget,
  } from "../../state.svelte";
  import { t, pct } from "../../i18n";
  import type { Light } from "../../types";
  import Icon from "../../ui/Icon.svelte";
  import LightDot from "./LightDot.svelte";
  import { doingName, doingOf } from "./modes";

  // Every light in the room as a card that shows its light and says what it
  // does, and picks the lights the settings beside it go to.

  let {
    pickable,
    wholeRoom,
  }: {
    /** The lights that can be picked: those with a bulb. */
    pickable: Light[];
    /** True while the settings go to the whole room. */
    wholeRoom: boolean;
  } = $props();

  const lights = $derived(app.config.room.lights);
  /** Picking only means something with two lights or more. */
  const selectable = $derived(pickable.length > 1);
</script>

{#if lights.length > 0}
  <section class="group">
    <div class="group-head">
      <h2 class="group-title">{t("lights")}</h2>
      {#if selectable}
        <!-- Who the settings go to, not what plays: a neutral edge and a
             tick, never the amber of something running. -->
        <button
          class="chip"
          class:picked={wholeRoom}
          aria-pressed={wholeRoom}
          title={t("select_all_tip")}
          onclick={() => (app.target = [])}
        >
          <span class="ring" aria-hidden="true">{#if wholeRoom}<Icon name="check" size={10} />{/if}</span>
          {t("select_all")}
        </button>
      {/if}
    </div>
    <div class="cards">
      {#each lights as l (l.id)}
        {@const b = bindingFor(l.id)}
        {@const lm = modeOf(l.id)}
        {@const other = b ? drivenElsewhere(l.id) : null}
        {@const picked = selectable && !wholeRoom && app.target.includes(l.id)}
        <div class="light" class:picked class:unbound={!b}>
          <span class="disc">
            <LightDot id={l.id} size={28} />
            <!-- Picked: a tick on the light and a neutral edge, like a checkbox. -->
            {#if picked}<span class="tick" aria-hidden="true"><Icon name="check" size={9} /></span>{/if}
          </span>

          <div class="text">
            {#if b && selectable}
              <button
                class="pick"
                aria-pressed={picked}
                aria-describedby="light-sub-{l.id}"
                onclick={() => toggleTarget(l.id)}
              >
                <span class="name">{l.name}</span>
              </button>
            {:else}
              <span class="name">{l.name}</span>
            {/if}
            <span class="sub" id="light-sub-{l.id}">
              {#if !b}
                <span>{t("no_bulb")}</span>
              {:else if lm && other !== null}
                <span title={other ? t("held_tip", { host: other }) : t("held_other_tip")}>
                  {other ? t("held_by", { host: other }) : t("held_by_other")}
                </span>
              {:else if lm && isReconnecting(l.id)}
                <span title={t("reconnecting_tip")}>{t("reconnecting")}</span>
              {:else if !lm && other}
                <span>{t("elsewhere_sub", { host: other })}</span>
              {:else}
                <!-- An effect plays: its name in the amber of light. -->
                <span class="what" class:live={lm}>{doingName(doingOf(l.id))}</span>
                {#if isOn(l.id)}<span class="num bright">{pct(brightOf(l.id) / 100)}</span>{/if}
              {/if}
            </span>
          </div>

          {#if !b}
            <!-- A small button: the light's name keeps the card's width. -->
            <button
              class="btn sm icon act"
              title={t("assign")}
              aria-label={t("labelled", { name: l.name, what: t("assign") })}
              onclick={() => (app.view = "bulbs")}
            >
              <Icon name="plus" size={14} />
            </button>
          {:else if lm && other !== null}
            <button class="btn sm act" title={t("take_over_tip")} onclick={() => takeLight(l.id)}>
              {t("take_over")}
            </button>
          {:else}
            <!-- Whether it is on, the disc shows: the switch itself stays quiet. -->
            <button
              class="btn plain icon sm power"
              disabled={app.powerBusy[l.id]}
              title={isOn(l.id) ? t("turn_off") : t("turn_on")}
              aria-label={t("labelled", { name: l.name, what: isOn(l.id) ? t("turn_off") : t("turn_on") })}
              onclick={() => toggleLightPower(l.id)}
            >
              <Icon name="power" size={14} />
            </button>
          {/if}
        </div>
      {/each}
    </div>
  </section>
{/if}

<style>
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--s-2);
  }
  .light {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--s-3);
    min-width: 0;
    min-height: 60px;
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-3);
    border-radius: var(--r-lg);
    background: var(--material);
    -webkit-backdrop-filter: var(--material-filter);
    backdrop-filter: var(--material-filter);
    box-shadow: var(--card-edge);
    transition:
      box-shadow 200ms var(--ease),
      background var(--dur) var(--ease);
  }
  .light:has(.pick:hover) {
    background: var(--material-hover);
  }
  /* The tint of a selection over the card, never the room's glow through it. */
  .light.picked {
    background: linear-gradient(var(--select-soft), var(--select-soft)), var(--material-hover);
    box-shadow: inset 0 0 0 1.5px var(--select);
  }
  /* No bulb yet: an outline waiting for one, not a card. */
  .light.unbound {
    background: none;
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }
  .disc {
    position: relative;
    display: grid;
    flex: none;
  }
  .tick {
    position: absolute;
    right: -4px;
    bottom: -4px;
    width: 15px;
    height: 15px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--select);
    box-shadow: 0 0 0 2px var(--surface);
    color: var(--on-ink);
    animation: pop 180ms var(--ease);
  }
  @keyframes pop {
    from {
      transform: scale(0.4);
      opacity: 0;
    }
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  /* The whole card picks the light; its buttons sit above it. */
  .pick {
    min-width: 0;
    display: flex;
    text-align: left;
  }
  .pick::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
  }
  .pick:focus-visible {
    outline: none;
  }
  .pick:focus-visible::after {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .power,
  .act {
    position: relative;
    z-index: 1;
    flex: none;
  }
  .name {
    min-width: 0;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .unbound .name {
    color: var(--text-2);
    font-weight: 500;
  }
  .sub {
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    font-size: var(--t-sm);
    color: var(--text-2);
    white-space: nowrap;
  }
  .sub > * {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .what.live {
    color: var(--glow-text);
    font-weight: 500;
  }
  .bright {
    flex: none;
    color: var(--text-3);
  }
  .power {
    color: var(--text-3);
  }
  .power:hover {
    color: var(--text);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 var(--s-3) 0 var(--s-2);
    border-radius: 999px;
    background: var(--material);
    -webkit-backdrop-filter: var(--material-filter);
    backdrop-filter: var(--material-filter);
    box-shadow: var(--card-edge);
    font-size: var(--t-sm);
    font-weight: 500;
    white-space: nowrap;
  }
  .chip:hover {
    background: var(--material-hover);
  }
  .chip.picked {
    background: linear-gradient(var(--select-soft), var(--select-soft)), var(--material-hover);
    box-shadow: inset 0 0 0 1.5px var(--select);
  }
  .ring {
    width: 14px;
    height: 14px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1.5px var(--control-line);
  }
  .chip.picked .ring {
    background: var(--select);
    box-shadow: none;
    color: var(--on-ink);
  }
  @media (prefers-reduced-motion: reduce) {
    .tick {
      animation: none;
    }
  }
</style>
