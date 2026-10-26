// Pairing Hue bridges and Nanoleaf controllers, and the Smart Life sign-in
// that imports Tuya bulbs.

import * as ipc from "../ipc";
import { errorDetail, errorText, isBackendError, t } from "../i18n";
import type { DiscoveredDevice } from "../types";
import { app } from "./app.svelte";
import { reloadConfig } from "./config.svelte";
import { bindingFor } from "./derived";
import { addDevices, scan } from "./devices.svelte";
import { fail } from "./toasts";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** How long a bridge or controller waits for its button to be pressed. */
export const PAIR_WINDOW_MS = 30_000;

/** Bumped by every pairing started or cancelled, so an older one that is
 *  still waiting on the device knows to stop. */
let pairRun = 0;

/**
 * Pair with a Hue bridge or Nanoleaf controller: the page asks the user to
 * press its button and counts down, while Lueur keeps asking the device for
 * up to 30 seconds. The outcome stays on the page until it is closed.
 */
export async function pair(d: DiscoveredDevice): Promise<void> {
  if (app.pairing?.step === "waiting") return;
  const run = ++pairRun;
  const current = () => run === pairRun;
  const device = $state.snapshot(d);
  const until = Date.now() + PAIR_WINDOW_MS;
  app.pairing = { device, step: "waiting", until };
  try {
    for (;;) {
      try {
        await ipc.pairDevice(device);
        break;
      } catch (e) {
        const waiting = isBackendError(e) && e.code === "pair_waiting";
        if (!waiting || Date.now() > until || !current()) throw e;
        await sleep(1500);
      }
    }
    // Even when cancelled meanwhile: the backend keeps the pairing, so the
    // config here must catch up with it.
    await reloadConfig();
    if (!current()) return;
    app.pairing = { device, step: "paired", until };
    await scanNow();
  } catch (e) {
    if (!current()) return;
    const expired = isBackendError(e) && e.code === "pair_waiting";
    const msg = expired ? t("pair_expired") : `${t("pair_failed")} ${errorText(e)}`;
    app.pairing = { device, step: "failed", until, failure: { expired, msg, detail: errorDetail(e) } };
  }
}

/** Stop waiting for the button, or close the outcome of a pairing. */
export function closePairing(): void {
  pairRun++;
  app.pairing = null;
}

/** Search now, after any search already running, so the results are fresh. */
async function scanNow(timeoutMs?: number): Promise<void> {
  while (app.scanning) await sleep(200);
  await scan(timeoutMs);
}

/** Bumped by every Smart Life sign-in started, reopened or closed, so one
 *  still waiting on the network knows the page has moved on without it. */
let tuyaRun = 0;

/** Open the Smart Life sign-in, at the user-code step. */
export function openTuya(): void {
  tuyaRun++;
  app.tuya = { step: "code" };
}

export function closeTuya(): void {
  tuyaRun++;
  app.tuya = null;
}

/**
 * A Smart Life user code as typed or pasted, without the spaces and dashes
 * people put in to read it (the backend does the same). Letter case stays:
 * the codes mix both, and changing it would make a good code wrong.
 */
export function normalizeUserCode(s: string): string {
  return s.replace(/[\s\-_.‐-―−]/g, "");
}

/** Why a (normalized) user code cannot be right, or null if it may be. */
export function userCodeProblem(code: string): "tuya_code_empty" | "tuya_code_chars" | "tuya_code_short" | null {
  if (code === "") return "tuya_code_empty";
  if (!/^[A-Za-z0-9]*$/.test(code) || code.length > 64) return "tuya_code_chars";
  if (code.length < 4) return "tuya_code_short";
  return null;
}

/**
 * Sign in to Smart Life with the user code: show the QR code, wait (three
 * minutes at most) for it to be scanned, import the bulbs, then find and add
 * them (see `finishTuyaImport`).
 */
export async function signInTuya(userCode: string): Promise<void> {
  if (!app.tuya || app.tuya.step !== "code") return;
  const code = normalizeUserCode(userCode);
  if (userCodeProblem(code)) return;
  const run = ++tuyaRun;
  const current = () => run === tuyaRun;
  app.tuya = { step: "starting" };
  try {
    const login = await ipc.tuyaLoginStart(code);
    if (!current()) return;
    app.tuya = { step: "qr", login };
    const deadline = Date.now() + 180_000;
    while (current()) {
      const n = await ipc.tuyaLoginPoll(login.token, code);
      if (n !== null) {
        // Imported even if the page moved on meanwhile: the backend keeps
        // the keys, so the config here must catch up with it.
        await reloadConfig();
        if (current()) await finishTuyaImport(n, run);
        return;
      }
      if (!current()) return;
      if (Date.now() > deadline) {
        app.tuya = { step: "code", expired: true };
        return;
      }
      await sleep(2000);
    }
  } catch (e) {
    // Cancelled or replaced meanwhile: the failure is no longer news.
    if (!current()) return;
    app.tuya = { step: "code" };
    fail(e);
  }
}

/**
 * After a Smart Life import: look for the bulbs on the network and add the
 * ones found to the room, so importing really ends with bulbs in the room.
 * When lights drawn without a bulb are waiting, adding new lights would
 * duplicate them: the user is asked to pick instead, with a button to add
 * them all anyway. Run again to look for bulbs that did not answer.
 */
export async function finishTuyaImport(imported: number, run = ++tuyaRun): Promise<void> {
  if (run !== tuyaRun) return;
  app.tuya = { step: "adding", imported };
  if (imported > 0) await scanNow(6000);
  if (run !== tuyaRun) return;
  const bound = (id: string) => app.config.bindings.some((b) => b.device_id === id);
  const found = app.discovered.filter((d) => d.kind.family === "tuya");
  const ready = found.filter((d) => d.paired && !bound(d.id));
  const lightsWaiting = app.config.room.lights.some((l) => !bindingFor(l.id));
  const added = lightsWaiting ? 0 : ready.length;
  if (added > 0) addDevices(ready, false);
  app.tuya = {
    step: "done",
    imported,
    added,
    ready: added > 0 ? [] : ready.map((d) => d.id),
    // Every Smart Life bulb heard, bound or not, against every one imported:
    // fewer heard means some did not answer, though not how many, since
    // bound ones may come from another account.
    missing: found.length < imported,
  };
}

/** Add the Smart Life bulbs left for the user to place, as new lights. */
export function addReadyTuya(): void {
  if (app.tuya?.step !== "done") return;
  const ids = app.tuya.ready;
  // Some may have been given to a light by hand meanwhile.
  const unbound = app.discovered.filter(
    (d) => ids.includes(d.id) && !app.config.bindings.some((b) => b.device_id === d.id),
  );
  addDevices(unbound, false);
  app.tuya = { ...app.tuya, added: app.tuya.added + unbound.length, ready: [] };
}
