// Config editing and persistence: changes are saved to the backend, and the
// config is loaded, reloaded, exported and imported here.

import * as ipc from "../ipc";
import { t } from "../i18n";
import { defaultConfig, type AppConfig } from "../types";
import { app } from "./app.svelte";
import { topology } from "./derived";
import { scan } from "./devices.svelte";
import { sleepChanged } from "./effect";
import { fail, notify } from "./toasts";

let saveTimer: ReturnType<typeof setTimeout> | null = null;

/** False until the backend config has arrived. Until then `app.config` holds
 *  defaults, and saving them would wipe the real setup. */
let configLoaded = false;

/**
 * Mutate the config in place and persist it (debounced, so dragging a slider
 * does not hammer the disk). A running effect picks up live-tunable settings.
 */
export function edit(mutator: (c: AppConfig) => void): void {
  mutator(app.config);
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(persistNow, 250);
}

export async function persistNow(): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  if (!configLoaded) return;
  try {
    await ipc.saveConfig($state.snapshot(app.config));
  } catch (e) {
    fail(e, "err_save");
  }
}

/** Fill in what a config from the backend or a file may lack: older configs
 *  predate some interface fields, the light states and the shows. */
function normalized(cfg: AppConfig): AppConfig {
  const def = defaultConfig();
  cfg.interface = { ...def.interface, ...cfg.interface };
  cfg.states ??= {};
  cfg.scenes ??= [];
  cfg.show = { ...def.show, ...cfg.show };
  return cfg;
}

let loaded = false;

export async function bootstrap(): Promise<void> {
  if (loaded) return;
  loaded = true;
  try {
    const cfg = normalized(await ipc.getConfig());
    // Someone upgrading with a furnished room has arranged it already.
    if (cfg.room.lights.length > 0 && cfg.bindings.length > 0 && !cfg.interface.room_arranged) {
      cfg.interface.room_arranged = true;
    }
    app.config = cfg;
    configLoaded = true;
    app.status = await ipc.effectStatus();
    sleepChanged(await ipc.sleepTimer());
    // Running already (resumed at launch): it was started from the saved config.
    if (app.status.running) app.startedWith = topology();
  } catch (e) {
    // Without the config, offer to try again rather than run on defaults.
    const retry = () => {
      loaded = false;
      void bootstrap();
    };
    fail(e, "err_load", configLoaded ? undefined : { label: t("retry"), run: retry });
  } finally {
    app.ready = true;
  }
  // Bulbs get new addresses from the router now and then. A quiet search at
  // launch finds them again before anyone notices.
  if (app.config.bindings.length > 0) void scan(3000, true);
}

export async function refreshStatus(): Promise<void> {
  try {
    app.status = await ipc.effectStatus();
    if (!app.status.running) app.startedWith = null;
    else if (app.startedWith === null) app.startedWith = topology();
  } catch {
    /* transient; the next poll will catch up */
  }
}

/** The backend changed what only it writes (pairings, Tuya keys): reload. */
export async function reloadConfig(): Promise<void> {
  await persistNow();
  app.config = normalized(await ipc.getConfig());
  configLoaded = true;
}

/** Save the config to a file of the user's choosing. */
export async function exportConfig(): Promise<void> {
  try {
    await persistNow();
    if (await ipc.exportConfig()) notify("info", t("exported"));
  } catch (e) {
    fail(e);
  }
}

/** Replace the config with one from a file. Unsaved edits are flushed
 *  first, so they cannot land on top of the import. */
export async function importConfig(): Promise<void> {
  try {
    await persistNow();
    const cfg = await ipc.importConfig();
    if (!cfg) return;
    app.config = normalized(cfg);
    configLoaded = true;
    notify("info", t("imported"));
    if (cfg.bindings.length > 0) void scan(3000, true);
  } catch (e) {
    fail(e);
  }
}
