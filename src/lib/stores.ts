// Central app state: the config (persisted to the backend) and the live effect
// status. Uses classic Svelte stores so it works cleanly from plain `.ts`.

import { get, writable } from "svelte/store";
import * as ipc from "./ipc";
import {
  defaultConfig,
  type AppConfig,
  type DiscoveredDevice,
  type EffectStatus,
} from "./types";

export const config = writable<AppConfig>(defaultConfig());
export const status = writable<EffectStatus>({ running: false, connected_lights: 0 });
export const busy = writable<boolean>(false);
export const toast = writable<{ kind: "info" | "error"; msg: string } | null>(null);

// Discovery results live here rather than in DevicesPanel so they survive the
// panel being unmounted when the user switches tabs.
export const discovered = writable<DiscoveredDevice[]>([]);
export const hasScanned = writable<boolean>(false);
export const scanning = writable<boolean>(false);

// Last power state we *sent* to each light, keyed by light id. Yeelight's LAN
// protocol gives us no push notifications, so this is an optimistic view used
// only to render the buttons — it is not authoritative.
export const powerState = writable<Record<string, boolean>>({});
/** Light ids with an in-flight power command, so buttons can show progress. */
export const powerBusy = writable<Set<string>>(new Set());

function markBusy(id: string, busyNow: boolean): void {
  powerBusy.update((s) => {
    const next = new Set(s);
    if (busyNow) next.add(id);
    else next.delete(id);
    return next;
  });
}

/** Turn one room light's bulb on or off. */
export async function setLightPower(lightId: string, on: boolean): Promise<void> {
  markBusy(lightId, true);
  try {
    await ipc.setLightPower(lightId, on);
    powerState.update((s) => ({ ...s, [lightId]: on }));
  } catch (e) {
    notify("error", `Could not ${on ? "turn on" : "turn off"} bulb: ${errMsg(e)}`);
  } finally {
    markBusy(lightId, false);
  }
}

/** Toggle one room light's bulb, based on the last state we sent. */
export async function toggleLightPower(lightId: string): Promise<void> {
  const current = get(powerState)[lightId] ?? true;
  return setLightPower(lightId, !current);
}

/**
 * Turn every bound bulb on or off. Turning off also stops the effect (the
 * backend does that), so refresh the status afterwards.
 */
export async function setAllPower(on: boolean): Promise<void> {
  busy.set(true);
  try {
    const res = await ipc.setAllPower(on);
    // Reflect the result on every bound light, so the buttons agree with reality.
    const bound = get(config).bindings.map((b) => b.light_id);
    powerState.update((s) => {
      const next = { ...s };
      for (const id of bound) next[id] = on;
      return next;
    });
    if (!on) await refreshStatus();

    const verb = on ? "on" : "off";
    if (res.failures.length === 0) {
      notify("info", `All ${plural(res.total, "light")} turned ${verb}.`);
    } else {
      notify(
        "error",
        `${res.succeeded} of ${res.total} turned ${verb}. Failed: ${res.failures.join("; ")}`,
      );
    }
  } catch (e) {
    notify("error", errMsg(e));
  } finally {
    busy.set(false);
  }
}

/** Run an SSDP discovery pass and keep the results for the whole session. */
export async function scanDevices(timeoutMs = 3000): Promise<void> {
  if (get(scanning)) return;
  scanning.set(true);
  try {
    const found = await ipc.discoverDevices(timeoutMs);
    discovered.set(found);
    hasScanned.set(true);
    if (found.length === 0) {
      notify("info", "No bulbs found. Make sure LAN Control is enabled in the Yeelight app.");
    }
  } catch (e) {
    notify("error", `Scan failed: ${errMsg(e)}`);
  } finally {
    scanning.set(false);
  }
}

let loaded = false;

/** Load config + status from the backend once at startup. */
export async function bootstrap(): Promise<void> {
  if (loaded) return;
  loaded = true;
  try {
    config.set(await ipc.getConfig());
    status.set(await ipc.effectStatus());
  } catch (e) {
    notify("error", `Failed to load settings: ${errMsg(e)}`);
  }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Apply an immutable update to the config and persist it (debounced so dragging
 * a slider doesn't hammer the disk). If the effect is running, the backend
 * pushes the live-tunable parts to it automatically.
 */
export function updateConfig(mutator: (c: AppConfig) => AppConfig): void {
  config.update((c) => mutator(structuredClone(c)));
  schedulePersist();
}

function schedulePersist(): void {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(persistNow, 250);
}

/** Persist immediately (used before starting the effect, etc.). */
export async function persistNow(): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  try {
    await ipc.saveConfig(get(config));
  } catch (e) {
    notify("error", `Failed to save settings: ${errMsg(e)}`);
  }
}

export async function refreshStatus(): Promise<void> {
  try {
    status.set(await ipc.effectStatus());
  } catch {
    /* ignore transient status errors */
  }
}

export async function toggleEffect(): Promise<void> {
  busy.set(true);
  try {
    await persistNow();
    const current = get(status);
    if (current.running) {
      await ipc.stopEffect();
      status.set({ running: false, connected_lights: 0 });
      notify("info", "Ambilight stopped.");
    } else {
      const s = await ipc.startEffect();
      status.set(s);
      notify("info", `Ambilight running, ${plural(s.connected_lights, "light")} connected.`);
    }
  } catch (e) {
    notify("error", errMsg(e));
  } finally {
    busy.set(false);
  }
}

export function notify(kind: "info" | "error", msg: string): void {
  toast.set({ kind, msg });
  setTimeout(() => {
    toast.update((t) => (t && t.msg === msg ? null : t));
  }, 4200);
}

/** "1 light", "3 lights" — every user-facing count goes through this. */
export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
