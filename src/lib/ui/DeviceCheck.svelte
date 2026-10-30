<script lang="ts">
  // A paired Hue bridge or Nanoleaf controller the last search heard where
  // nothing proves it is the one paired (a new address, or another
  // certificate). Lueur sends it nothing until the user says it is theirs;
  // see src-tauri/src/identity.rs.
  import { onMount } from "svelte";
  import { app, fail, notify, pair, scan } from "../state.svelte";
  import { t } from "../i18n";
  import * as ipc from "../ipc";
  import type { UnverifiedDevice } from "../types";

  let pending = $state<UnverifiedDevice[]>([]);
  let busy = $state(false);
  /** Read out when a search brings news, not each time the page is shown. */
  let news = $state("");

  /** A device as it showed itself: a "not mine" holds for the session
   *  until it shows up somewhere else or with another certificate. */
  const sighting = (d: UnverifiedDevice) => `${d.id}|${d.ip}|${d.changed}`;
  const dismissed = (d: UnverifiedDevice) => app.dismissedDevices.includes(sighting(d));

  function say(d: UnverifiedDevice): string {
    if (d.needs_pairing) return t("unverified_pinned", { name: d.name, ip: d.ip });
    return d.changed
      ? t("unverified_changed", { name: d.name, ip: d.ip })
      : t("unverified_moved", { name: d.name, ip: d.ip, known: d.known_ip });
  }

  onMount(() => {
    void ipc.unverifiedDevices().then((list) => (pending = list), () => {});
    const stop = ipc.onDeviceUnverified((list) => {
      const before = new Set(pending.map(sighting));
      pending = list;
      const fresh = list.filter((d) => !before.has(sighting(d)) && !dismissed(d));
      news = "";
      if (fresh.length > 0) setTimeout(() => (news = fresh.map(say).join(" ")), 60);
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  const shown = $derived(pending.filter((d) => !dismissed(d)));

  async function confirm(d: UnverifiedDevice) {
    busy = true;
    try {
      await ipc.confirmDevice(d.id);
      pending = pending.filter((x) => x.id !== d.id);
      notify("info", t("unverified_confirmed", { name: d.name }));
      // Its lamps come back through the search, at the new address.
      await scan();
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  /** Pair it again with its button: the one way back for a bridge that no
   *  longer shows its pinned certificate, whose old pairing goes first. */
  async function pairAgain(d: UnverifiedDevice) {
    const found = app.discovered.find((x) => x.id === d.id && x.ip === d.ip);
    if (!found) return void scan();
    busy = true;
    try {
      if (d.needs_pairing) await ipc.forgetDevice(d.id);
      pending = pending.filter((x) => x.id !== d.id);
    } catch (e) {
      fail(e);
      return;
    } finally {
      busy = false;
    }
    await pair(found);
  }
</script>

<!-- Always present, so only the arrival of news is read out. -->
<div class="sr-only" role="status">{news}</div>

{#if shown.length > 0}
<div class="checks">
{#each shown as d (d.id)}
  <div class="banner">
    <div class="text">
      <p>{say(d)}</p>
      <p class="tip">{t("unverified_tip")}</p>
    </div>
    <div class="actions">
      <button
        class="btn sm"
        disabled={busy}
        onclick={() => (app.dismissedDevices = [...app.dismissedDevices, sighting(d)])}
      >
        {t("unverified_not_mine")}
      </button>
      <!-- Never the main action: the user's word is the weakest proof. -->
      {#if !d.needs_pairing}
        <button class="btn sm" disabled={busy} onclick={() => confirm(d)}>
          {t("unverified_mine")}
        </button>
      {/if}
      <button class="btn sm primary" disabled={busy} onclick={() => pairAgain(d)}>
        {t("unverified_pair_again")}
      </button>
    </div>
  </div>
{/each}
</div>
{/if}

<style>
  /* Above the page's groups, a group apart from them. */
  .checks {
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
    margin-bottom: var(--s-6);
  }
  .banner {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-2) var(--s-2) var(--s-2) var(--s-4);
    border-radius: var(--r-lg);
    background: var(--surface);
    box-shadow: var(--card-edge);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  p {
    margin: 0;
    text-wrap: pretty;
  }
  .tip {
    margin-top: var(--s-1);
    font-size: var(--t-sm);
    color: var(--text-3);
  }
  .actions {
    display: flex;
    gap: var(--s-2);
    flex-shrink: 0;
  }
</style>
