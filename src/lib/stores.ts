// Central app state: the config (persisted to the backend) and the live effect
// status. Uses classic Svelte stores so it works cleanly from plain `.ts`.

import { get, writable } from "svelte/store";
import * as ipc from "./ipc";
import { defaultConfig, type AppConfig, type EffectStatus } from "./types";

export const config = writable<AppConfig>(defaultConfig());
export const status = writable<EffectStatus>({ running: false, connected_lights: 0 });
export const busy = writable<boolean>(false);
export const toast = writable<{ kind: "info" | "error"; msg: string } | null>(null);

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
      notify("info", `Ambilight running — ${s.connected_lights} light(s) connected.`);
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

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
