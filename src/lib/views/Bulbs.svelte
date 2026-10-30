<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    app,
    addDevices,
    addReadyTuya,
    assignDevice,
    bindingFor,
    closePairing,
    closeTuya,
    deviceName,
    endpointOf,
    endpointOfDevice,
    finishTuyaImport,
    identify,
    normalizeUserCode,
    openTuya,
    pair,
    PAIR_WINDOW_MS,
    removeLight,
    renameLight,
    scan,
    signInTuya,
    unassign,
    userCodeProblem,
  } from "../state.svelte";
  import { t, tn } from "../i18n";
  import type { DeviceBinding, DiscoveredDevice } from "../types";
  import { FAMILY, bulbLabel as describeBulb } from "../brands";
  import Icon from "../ui/Icon.svelte";
  import DeviceCheck from "../ui/DeviceCheck.svelte";
  import LightDot from "./lighting/LightDot.svelte";

  // First visit: look straight away rather than greeting with an empty list.
  onMount(() => {
    if (!app.hasScanned) void scan();
  });

  const lights = $derived(app.config.room.lights);
  const unboundLights = $derived(lights.filter((l) => !bindingFor(l.id)));
  // Bulbs already bound are listed under "In the room"; only show the rest here.
  const unbound = $derived(app.discovered.filter((d) => !app.config.bindings.some((b) => b.device_id === d.id)));
  const fresh = $derived(unbound.filter((d) => d.paired));

  let userCode = $state("");
  /** What is wrong with the user code, once Continue was pressed. */
  let codeProblem = $state<ReturnType<typeof userCodeProblem>>(null);
  // Characters a code can never hold are pointed out while typing; a code
  // too short only on Continue, since it is short until finished.
  const shownProblem = $derived(
    codeProblem ?? (userCodeProblem(normalizeUserCode(userCode)) === "tuya_code_chars" ? "tuya_code_chars" : null),
  );

  function submitCode(e: SubmitEvent) {
    e.preventDefault();
    userCode = normalizeUserCode(userCode);
    codeProblem = userCodeProblem(userCode);
    if (!codeProblem) void signInTuya(userCode);
  }

  // The pairing countdown, ticking only while a device waits for its button.
  let now = $state(Date.now());
  $effect(() => {
    if (app.pairing?.step !== "waiting") return;
    now = Date.now();
    const tick = setInterval(() => (now = Date.now()), 250);
    return () => clearInterval(tick);
  });
  const pairLeft = $derived(app.pairing ? Math.max(0, app.pairing.until - now) : 0);

  // Keyboard and screen reader users land on the pairing panel when it opens.
  let pairHead = $state<HTMLElement | null>(null);
  let pairShown: string | null = null;
  $effect(() => {
    const id = app.pairing?.device.id ?? null;
    if (id && id !== pairShown) pairHead?.focus();
    pairShown = id;
  });

  /** Retry with the device as last found, in case it moved meanwhile. The
   *  Retry button goes away, so focus stays on the panel, by its title. */
  async function retryPairing(d: DiscoveredDevice) {
    void pair(app.discovered.find((x) => x.id === d.id) ?? d);
    await tick();
    pairHead?.focus();
  }

  let networkHead = $state<HTMLElement | null>(null);
  let pageHead = $state<HTMLElement | null>(null);

  /** Close the pairing panel. Focus goes back to the device's Pair button
   *  when there still is one, else to the list it was in, rather than being
   *  lost with the panel. */
  async function endPairing() {
    const id = app.pairing?.device.id;
    closePairing();
    await tick();
    const button = id ? document.querySelector<HTMLButtonElement>(`[data-pair="${CSS.escape(id)}"]`) : null;
    (button && !button.disabled ? button : (networkHead ?? pageHead))?.focus();
  }

  /** The Smart Life bulbs found after an import, still waiting for a light. */
  const tuyaReady = $derived(
    app.tuya?.step === "done"
      ? app.discovered.filter(
          (d) => app.tuya?.step === "done" && app.tuya.ready.includes(d.id) && !isBound(d.id),
        )
      : [],
  );

  /** Where the import or the pairing stands, read out as it changes. */
  const said = $derived.by(() => {
    const p = app.pairing;
    if (p?.step === "waiting") return t(p.device.kind.family === "hue" ? "pair_hue" : "pair_nanoleaf");
    if (p?.step === "paired") return t("paired", { name: deviceName(p.device) });
    if (p?.step === "failed") return p.failure?.msg ?? "";
    const tu = app.tuya;
    if (tu?.step === "adding") return t("tuya_adding");
    if (tu?.step === "done") return tuyaSummary(tu, tuyaReady.length).join(" ");
    if (tu?.step === "code" && tu.expired) return t("tuya_expired");
    return "";
  });

  /** What came of a Smart Life import, sentence by sentence. */
  function tuyaSummary(
    tu: { imported: number; added: number; missing: boolean },
    ready: number,
  ): string[] {
    if (tu.imported === 0) return [t("tuya_none")];
    const out: string[] = [];
    if (tu.added > 0) out.push(tn("tuya_added", tu.added));
    if (ready > 0) out.push(tn("tuya_ready", ready));
    if (tu.missing) out.push(t("tuya_missing"));
    if (out.length === 0) out.push(t("tuya_already"));
    return out;
  }

  const isBound = (deviceId: string) => app.config.bindings.some((b) => b.device_id === deviceId);

  // Smart Life bulbs are never found by searching alone: say so wherever the
  // list is, until some have been imported.
  const tuyaHint = $derived(
    app.tuya === null &&
      !app.discovered.some((d) => d.kind.family === "tuya") &&
      !app.config.bindings.some((b) => b.kind?.family === "tuya"),
  );

  // Addresses mean nothing to most people: a bulb is told apart by its name,
  // brand and model, and its address only shows when two share a name.
  const sharedNames = $derived.by(() => {
    const seen = new Map<string, number>();
    for (const d of unbound) seen.set(deviceName(d), (seen.get(deviceName(d)) ?? 0) + 1);
    return new Set([...seen].filter(([, n]) => n > 1).map(([name]) => name));
  });
  const sharedBulbNames = $derived.by(() => {
    const seen = new Map<string, number>();
    for (const b of app.config.bindings) seen.set(bulbLabel(b), (seen.get(bulbLabel(b)) ?? 0) + 1);
    return new Set([...seen].filter(([, n]) => n > 1).map(([name]) => name));
  });

  /** How a bound bulb is described under its light. */
  function bulbLabel(b: DeviceBinding): string {
    return describeBulb(b, app.config.room.lights.find((l) => l.id === b.light_id)?.name);
  }

  /** Only Yeelight reports what it can do; every other family can stream. */
  function supportsMusic(d: DiscoveredDevice): boolean {
    if (d.kind.family !== "yeelight") return true;
    return d.support.includes("set_music") && d.support.includes("set_rgb");
  }

  function onAssign(d: DiscoveredDevice, e: Event & { currentTarget: HTMLSelectElement }) {
    const id = e.currentTarget.value;
    e.currentTarget.value = "";
    if (id === "__new") addDevices([d]);
    else if (id) assignDevice(id, d);
  }
</script>

<div class="page bulbs" class:wide={lights.length > 0}>
  <header class="page-head">
    <h1 class="page-title" tabindex="-1" bind:this={pageHead}>{t("nav_bulbs")}</h1>
    <div class="actions">
      {#if fresh.length > 1 && unboundLights.length === 0}
        <button class="btn" onclick={() => addDevices(fresh)}>{t("add_all")}</button>
      {/if}
      <button
        class="btn"
        title={t("tuya_open_tip")}
        onclick={openTuya}
        disabled={app.tuya !== null && app.tuya.step !== "done"}
      >
        {t("tuya_open")}
      </button>
      <button class="btn" class:primary={!app.hasScanned} onclick={() => scan()} disabled={app.scanning}>
        {#if app.scanning}<span class="spinner" aria-hidden="true"></span>{/if}
        {app.scanning ? t("searching") : app.hasScanned ? t("search_again") : t("search")}
      </button>
    </div>
  </header>

  <!-- Always present, so each change to the pairing or the import is read out. -->
  <div class="sr-only" role="status">{said}</div>

  <!-- Security team: paired bridges heard where nothing proves they are yours. -->
  <DeviceCheck />

  {#if app.pairing}
    {@const p = app.pairing}
    {@const hue = p.device.kind.family === "hue"}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title" tabindex="-1" bind:this={pairHead}>
          {t("pair_title", { name: deviceName(p.device) })}
        </h2>
      </div>
      <div class="card pairing">
        <span class="pair-mark" class:ok={p.step === "paired"} class:bad={p.step === "failed"} aria-hidden="true">
          {#if p.step === "waiting"}<span class="spinner big"></span>
          {:else if p.step === "paired"}<Icon name="check" size={18} />
          {:else}<Icon name="alert" size={18} />{/if}
        </span>
        <div class="pair-text">
          {#if p.step === "waiting"}
            <p class="pair-say">{t(hue ? "pair_hue" : "pair_nanoleaf")}</p>
            <p class="note">{t(hue ? "pair_why" : "pair_why_nanoleaf")}</p>
            <div class="countdown">
              <div class="bar" aria-hidden="true">
                <span style="transform: scaleX({pairLeft / PAIR_WINDOW_MS})"></span>
              </div>
              <span class="note num" role="timer">
                {#if pairLeft > 0}
                  {t("pair_left", { n: Math.ceil(pairLeft / 1000) })} · {t("pair_waiting")}
                {:else}
                  <!-- Time is up, but the last answer from the device is still on its way. -->
                  {t("pair_checking")}
                {/if}
              </span>
            </div>
          {:else if p.step === "paired"}
            <p class="pair-say">{t("paired", { name: deviceName(p.device) })}</p>
          {:else}
            <p class="pair-say">{p.failure?.msg}</p>
            {#if p.failure?.detail}
              <details class="note detail">
                <summary>{t("details")}</summary>
                <code>{p.failure.detail}</code>
              </details>
            {/if}
          {/if}
          <div class="actions">
            {#if p.step === "failed"}
              <button class="btn primary sm" onclick={() => retryPairing(p.device)}>{t("retry")}</button>
            {/if}
            <button class="btn sm" class:plain={p.step !== "paired"} onclick={endPairing}>
              {p.step === "paired" ? t("close") : t("tuya_cancel")}
            </button>
          </div>
        </div>
      </div>
    </section>
  {/if}

  {#if app.tuya}
    {@const tu = app.tuya}
    <section class="group">
      <div class="group-head">
        <h2 class="group-title">{t("tuya_title")}</h2>
      </div>
      <div class="card tuya">
        {#if tu.step === "qr"}
          <!-- Generated by the backend from a validated token, never fetched. -->
          <div class="qr" role="img" aria-label={t("tuya_qr")}>{@html tu.login.svg}</div>
          <div class="tuya-text">
            <p>{t("tuya_scan")}</p>
            <p class="note waiting"><span class="spinner" aria-hidden="true"></span> {t("tuya_waiting")}</p>
            <div>
              <button class="btn plain sm" onclick={closeTuya}>{t("tuya_cancel")}</button>
            </div>
          </div>
        {:else if tu.step === "adding"}
          <div class="tuya-text">
            <p class="waiting"><span class="spinner" aria-hidden="true"></span> {t("tuya_adding")}</p>
          </div>
        {:else if tu.step === "done"}
          <div class="tuya-text">
            {#each tuyaSummary(tu, tuyaReady.length) as line, i (i)}
              <p class:pair-say={i === 0}>{line}</p>
            {/each}
            <div class="actions">
              {#if tuyaReady.length > 0}
                <button class="btn primary" onclick={addReadyTuya}>{tn("tuya_add_n", tuyaReady.length)}</button>
              {:else if tu.added > 0}
                <button
                  class="btn primary"
                  onclick={() => {
                    closeTuya();
                    app.view = "room";
                  }}>{t("step2_btn")}</button
                >
              {/if}
              {#if tu.missing}
                <button class="btn" onclick={() => finishTuyaImport(tu.imported)} disabled={app.scanning}>
                  {t("search_again")}
                </button>
              {/if}
              <button class="btn plain" onclick={closeTuya}>{t("close")}</button>
            </div>
          </div>
        {:else}
          <form class="tuya-text" novalidate onsubmit={submitCode}>
            <div class="code">
              <label for="tuya-code">{t("tuya_code")}</label>
              <input
                id="tuya-code"
                class="code-input"
                type="text"
                bind:value={userCode}
                oninput={() => (codeProblem = null)}
                spellcheck="false"
                autocomplete="off"
                autocapitalize="off"
                maxlength="80"
                aria-invalid={shownProblem !== null}
                aria-describedby="tuya-code-help"
              />
            </div>
            <!-- Read out once as it appears (the alert); not also tied to the
                 field, which would read it a second time. -->
            {#if shownProblem}
              <p class="problem" role="alert">
                <Icon name="alert" size={13} />
                {t(shownProblem)}
              </p>
            {/if}
            {#if tu.expired}
              <p class="warn">{t("tuya_expired")}</p>
            {/if}
            <p class="note" id="tuya-code-help">{t("tuya_code_help")}</p>
            <div class="actions">
              <button class="btn plain" type="button" onclick={closeTuya}>{t("tuya_cancel")}</button>
              <button class="btn primary" type="submit" disabled={tu.step === "starting"}>
                {#if tu.step === "starting"}<span class="spinner" aria-hidden="true"></span>{/if}
                {t("tuya_continue")}
              </button>
            </div>
          </form>
        {/if}
      </div>
    </section>
  {/if}

  <!-- What the network offers on one side, the room's lights on the other,
       side by side when the window is wide enough. -->
  <div class="layout">
    <div class="col">
      {#if unbound.length > 0 || lights.length === 0}
      <section class="group">
        <div class="group-head">
          <h2 class="group-title" tabindex="-1" bind:this={networkHead}>{t("on_network")}</h2>
          {#if app.hasScanned && !app.scanning}
            <span class="note">{tn("n_found", unbound.length)}</span>
          {/if}
        </div>

        <div class="card rows">
          {#if unbound.length === 0}
            <div class="empty">
              {#if app.scanning}
                <p>{t("listening")}</p>
              {:else if app.hasScanned}
                <p class="empty-title">{t("none_title")}</p>
              {:else}
                <p class="empty-title">{t("first_title")}</p>
                <p>{t("first_text")}</p>
              {/if}
              {#if tuyaHint}
                <p class="empty-hint">{t("tuya_hint")}</p>
                <button class="btn sm" onclick={openTuya}>{t("tuya_title")}</button>
              {/if}
            </div>
          {:else}
            {#each unbound as d (d.id)}
              <div class="device">
                <span class="found" aria-hidden="true"><Icon name="bulb" size={14} /></span>
                <div class="device-text">
                  <div class="device-name">{deviceName(d)}</div>
                  <div class="device-meta">
                    {FAMILY[d.kind.family]}{#if d.model}&nbsp;{d.model}{/if}{#if sharedNames.has(deviceName(d))}<span
                        class="num"
                      >
                        · {d.ip}</span
                      >{/if}{#if d.faster}<span class="warn">, {t("pair_faster")}</span>{:else if !d.paired}<span class="warn"
                        >, {t("needs_pairing")}</span
                      >{:else if !supportsMusic(d)}<span
                        class="warn">, {t("no_music")}</span
                      >{/if}
                  </div>
                </div>
                {#if d.paired}
                  <button class="btn plain sm" title={t("identify_tip")} onclick={() => identify(endpointOfDevice(d))}>
                    {t("identify")}
                  </button>
                {/if}
                {#if !d.paired}
                  {@const waiting = app.pairing?.step === "waiting"}
                  <button
                    class="btn sm"
                    class:plain={d.faster}
                    title={t(d.faster ? "pair_faster_tip" : "pair_tip")}
                    data-pair={d.id}
                    onclick={() => pair(d)}
                    disabled={waiting}
                  >
                    {#if waiting && app.pairing?.device.id === d.id}<span class="spinner" aria-hidden="true"></span>{/if}
                    {waiting && app.pairing?.device.id === d.id ? t("pairing") : t(d.faster ? "pair_again" : "pair")}
                  </button>
                {:else if unboundLights.length > 0}
                  <select class="assign" aria-label={t("labelled", { name: deviceName(d), what: t("use_for") })} onchange={(e) => onAssign(d, e)}>
                    <option value="" selected disabled>{t("use_for")}</option>
                    {#each unboundLights as l (l.id)}
                      <option value={l.id}>{l.name}</option>
                    {/each}
                    <option value="__new">{t("new_light")}</option>
                  </select>
                {:else}
                  <button class="btn sm" onclick={() => addDevices([d])}>{t("add")}</button>
                {/if}
              </div>
            {/each}
            {#if tuyaHint}
              <div class="device hint">
                <p class="note grow">{t("tuya_hint")}</p>
                <button class="btn plain sm" onclick={openTuya}>{t("tuya_title")}</button>
              </div>
            {/if}
          {/if}
        </div>
      </section>
      {/if}

      <section class="group">
        <details class="card tips" open={app.hasScanned && app.discovered.length === 0}>
          <summary>
            <Icon name="chevron" size={14} />
            <span>{t("tips_title")}</span>
          </summary>
          <ol>
            <li>{t("tip_1")}</li>
            <li>{t("tip_2")}</li>
            <li>{t("tip_3")}</li>
            <li>{t("tip_4")}</li>
            <li>{t("tip_5")}</li>
          </ol>
        </details>
      </section>
    </div>

  {#if lights.length > 0}
    <div class="col">
      <section class="group">
        <div class="group-head">
          <h2 class="group-title">{t("in_room")}</h2>
          <button class="btn plain sm" onclick={() => (app.view = "room")}>{t("step2_btn")}</button>
        </div>
        <!-- Each light as on the Lighting page: the light it gives, its
             name, which can be changed in place, and its bulb. -->
        <div class="cards">
          {#each lights as l (l.id)}
            {@const b = bindingFor(l.id)}
            <div class="light" class:unbound={!b}>
              <LightDot id={l.id} size={28} />
              <div class="light-text">
                <input
                  class="rename"
                  type="text"
                  value={l.name}
                  spellcheck="false"
                  aria-label={t("light_name")}
                  oninput={(e) => renameLight(l.id, e.currentTarget.value)}
                />
                <span class="device-meta">
                  {#if b}{bulbLabel(b)}{#if sharedBulbNames.has(bulbLabel(b))}<span class="num"> · {b.ip}</span>{/if}{:else}<span
                      class="warn">{t("pick_above")}</span
                    >{/if}
                </span>
              </div>
              {#if b}
                <button
                  class="btn plain icon sm"
                  title={t("identify_tip")}
                  aria-label={t("labelled", { name: l.name, what: t("identify") })}
                  onclick={() => identify(endpointOf(b))}
                >
                  <Icon name="flash" size={14} />
                </button>
                <button
                  class="btn plain icon sm"
                  title={t("unassign_tip")}
                  aria-label={t("labelled", { name: l.name, what: t("unassign") })}
                  onclick={() => unassign(l.id)}
                >
                  <Icon name="unlink" size={14} />
                </button>
              {:else}
                <!-- A light left without its bulb, say one paired again from
                     another account and added anew, can go from here too. -->
                <button
                  class="btn plain icon sm danger"
                  title={t("remove_light")}
                  aria-label={t("labelled", { name: l.name, what: t("remove_light") })}
                  onclick={() => removeLight(l.id)}
                >
                  <Icon name="trash" size={14} />
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    </div>
  {/if}
  </div>
</div>

<style>
  /* Wider than a single column once the room has lights: two, when the
     window has the room for them. Before that, the network alone. */
  .page.bulbs.wide {
    max-width: 1240px;
  }
  .col {
    min-width: 0;
  }
  /* A pairing or an import open above the lists stands a group apart. */
  .group + .layout,
  .layout > .col + .col,
  .col > :global(.group + .group) {
    margin-top: var(--s-6);
  }
  @media (min-width: 1200px) {
    .wide .layout {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      align-items: start;
      gap: var(--s-7);
    }
    .layout > .col + .col {
      margin-top: 0;
    }
  }

  /* --- The room's lights --- */
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: var(--s-2);
  }
  .light {
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
  }
  /* No bulb yet: an outline waiting for one, not a card. */
  .light.unbound {
    background: none;
    box-shadow: inset 0 0 0 1px var(--line-strong);
  }
  .light-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .light-text .device-meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* --- What the network offers --- */
  /* A bulb found but not in the room yet: a dashed place, like a light
     with no bulb, holding the bulb mark. */
  .found {
    width: 28px;
    height: 28px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1.5px dashed var(--line-strong);
    color: var(--text-2);
  }
  .actions {
    display: flex;
    gap: var(--s-2);
  }
  .note {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .spinner {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 1.5px solid var(--line-strong);
    border-top-color: var(--text);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  /* Reduced motion still needs to say "working": a slower turn, not a stop. */
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 1.6s !important;
      animation-iteration-count: infinite !important;
    }
  }

  .empty {
    padding: var(--s-6) var(--s-5);
    text-align: center;
    color: var(--text-3);
  }
  .empty-title {
    color: var(--text);
    font-weight: 600;
  }

  .device {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-3) var(--s-2) var(--s-4);
    min-height: 50px;
  }
  .device-text,
  .grow {
    flex: 1;
    min-width: 0;
  }
  .device-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .device-meta {
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .warn {
    color: var(--glow-text);
  }
  .assign {
    height: 26px;
    font-size: var(--t-sm);
    max-width: 180px;
  }
  .rename {
    width: 100%;
    height: 24px;
    margin-left: -9px;
    font-weight: 500;
    background: transparent;
    box-shadow: none;
    transition:
      background var(--dur) var(--ease),
      box-shadow var(--dur) var(--ease);
  }
  /* Plain text until it is pointed at or edited, then a field. */
  .rename:hover,
  .rename:focus {
    background: var(--surface);
    box-shadow: var(--control-edge);
  }

  .tuya {
    display: flex;
    gap: var(--s-5);
    align-items: center;
    padding: var(--s-4) var(--s-5);
  }
  .qr {
    flex: none;
    width: 168px;
    height: 168px;
    border-radius: var(--r-md);
    overflow: hidden;
    background: #fff;
  }
  .qr :global(svg) {
    width: 100%;
    height: 100%;
    display: block;
  }
  .tuya-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
  }
  .tuya-text p {
    margin: 0;
  }
  .waiting {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
  }
  .code {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    font-weight: 500;
  }
  .code-input {
    width: 220px;
  }
  .problem {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    color: var(--danger);
  }
  .code-input[aria-invalid="true"] {
    box-shadow:
      0 0 0 1px var(--danger),
      var(--control-hi);
  }

  .empty-hint {
    margin-top: var(--s-3);
  }
  .hint {
    min-height: 44px;
  }
  .hint p {
    margin: 0;
  }

  .pairing {
    display: flex;
    gap: var(--s-4);
    align-items: flex-start;
    padding: var(--s-4) var(--s-5);
  }
  .pair-mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: var(--r-full);
    background: var(--glow-soft);
    color: var(--glow-text);
  }
  .pair-mark.ok {
    background: color-mix(in oklab, var(--ok) 16%, transparent);
    color: var(--ok);
  }
  .pair-mark.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .spinner.big {
    width: 18px;
    height: 18px;
    border-width: 2px;
    border-top-color: var(--glow-text);
  }
  .pair-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
  }
  .pair-text p {
    margin: 0;
  }
  .pair-say {
    font-size: var(--t-lg);
    font-weight: 600;
    text-wrap: pretty;
  }
  .pair-text .actions {
    margin-top: var(--s-1);
  }
  .countdown {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .bar {
    flex: 1;
    max-width: 260px;
    height: 4px;
    border-radius: var(--r-full);
    background: var(--line);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--glow);
    transform-origin: left;
    transition: transform 250ms linear;
  }
  .detail summary {
    cursor: pointer;
    width: fit-content;
  }
  .detail code {
    display: block;
    margin-top: var(--s-1);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .tips summary {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-3) var(--s-4);
    cursor: pointer;
    list-style: none;
    font-weight: 500;
  }
  .tips summary::-webkit-details-marker {
    display: none;
  }
  .tips summary :global(svg) {
    color: var(--text-3);
    transition: transform var(--dur) var(--ease);
  }
  .tips[open] summary :global(svg) {
    transform: rotate(90deg);
  }
  .tips ol {
    margin: 0;
    padding: 0 var(--s-5) var(--s-4) calc(var(--s-4) + 30px);
    color: var(--text-2);
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
  }
</style>
