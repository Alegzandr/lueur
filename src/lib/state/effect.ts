// The ambilight and the shows: starting, stopping and moving lights between
// them, the built-in moods, and the sleep timer.

import * as ipc from "../ipc";
import { t } from "../i18n";
import type { Mode, SleepStatus } from "../types";
import { app, idle } from "./app.svelte";
import { persistNow, refreshStatus } from "./config.svelte";
import { effectTargets, modeOf, pickedLights, topology } from "./derived";
import { reloadStates, reportPartial } from "./lights";
import { fail, notify } from "./toasts";

/** Start the ambilight or a show from the saved config. */
async function start(mode: Mode): Promise<void> {
  app.status = await ipc.startEffect(mode);
  app.startedWith = topology();
  if (app.status.connected_lights === 0) notify("error", t("started_silent"));
}

export async function toggleEffect(): Promise<void> {
  if (app.busy) return;
  app.busy = true;
  try {
    await persistNow();
    if (app.status.running) {
      await ipc.stopEffect();
      await reloadStates();
      app.status = idle();
      app.startedWith = null;
    } else {
      await start("ambilight");
    }
  } catch (e) {
    fail(e);
  } finally {
    app.busy = false;
  }
}

/** Stop and start again so the effect picks up room and capture changes. */
export async function restartEffect(): Promise<void> {
  if (app.busy) return;
  app.busy = true;
  try {
    await persistNow();
    app.status = await ipc.restartEffect();
    app.startedWith = topology();
  } catch (e) {
    await refreshStatus();
    fail(e);
  } finally {
    app.busy = false;
  }
}

/** Take a light's bulb back from whatever else drives it, so the effect
 *  playing on it here drives it again. */
export async function takeLight(lightId: string): Promise<void> {
  try {
    app.status = await ipc.takeLight(lightId);
  } catch (e) {
    await refreshStatus();
    fail(e);
  }
}

/** Apply a built-in mood to the lights picked, or to the whole room. Stops
 *  the effects playing on them. */
export async function applyScene(id: string): Promise<void> {
  if (app.pending) return;
  app.pending = id;
  try {
    await persistNow();
    const res = await ipc.applyScene(id, pickedLights());
    await reloadStates();
    await refreshStatus();
    reportPartial(res);
  } catch (e) {
    fail(e);
  } finally {
    app.pending = null;
  }
}

/** The sleep timer changed (here, from the tray, or it ran out). */
export function sleepChanged(s: SleepStatus | null): void {
  app.sleep = s && { status: s, at: Date.now() };
}

/** Set the sleep timer, or cancel it with null. */
export async function setSleepTimer(minutes: number | null): Promise<void> {
  try {
    sleepChanged(await ipc.setSleepTimer(minutes));
  } catch (e) {
    fail(e);
  }
}

/** Play the ambilight or a show on the lights picked, or on the whole room.
 *  Picking one they all play already stops it on them. */
export async function chooseMode(mode: Mode): Promise<void> {
  if (app.busy || app.pending) return;
  app.pending = mode;
  app.busy = true;
  try {
    await persistNow();
    const ids = effectTargets();
    const picked = pickedLights();
    if (ids.length > 0 && ids.every((id) => modeOf(id) === mode)) {
      if (picked) await moveLights(picked, null);
      else await ipc.stopEffect(mode);
      await reloadStates();
      await refreshStatus();
    } else if (picked) {
      await moveLights(picked, mode);
    } else {
      await start(mode);
    }
  } catch (e) {
    await refreshStatus();
    fail(e);
  } finally {
    app.busy = false;
    app.pending = null;
  }
}

/** Play `mode` on `ids`, in step with every other light playing it, or give
 *  them back to the hand with null. */
export async function moveLights(ids: string[], mode: Mode | null): Promise<void> {
  const wasRunning = app.status.running;
  app.status = await ipc.setLightsMode(ids, mode);
  if (!app.status.running) app.startedWith = null;
  else if (!wasRunning || app.startedWith === null) app.startedWith = topology();
}
