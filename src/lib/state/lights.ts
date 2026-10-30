// Lights by hand: power, brightness, white and colour, sent to the bulbs as
// they change, and the identify flash.

import * as ipc from "../ipc";
import { t } from "../i18n";
import { dimmedOnly, type RgbTuple } from "../scenes";
import type { Endpoint, LightState, PowerResult } from "../types";
import { app } from "./app.svelte";
import { edit, persistNow, refreshStatus } from "./config.svelte";
import { bindingFor, endpointOf, isDriven, isOn, lightState, modeOf } from "./derived";
import { fail, notify } from "./toasts";

/** Record a state the backend has already applied and persisted. */
function recorded(lightId: string, s: LightState): void {
  app.config.states = { ...app.config.states, [lightId]: s };
}

// --- Power ---

export async function setLightPower(lightId: string, on: boolean): Promise<void> {
  app.powerBusy[lightId] = true;
  // Off takes the light out of its effect (the backend does it).
  const driven = isDriven(lightId);
  try {
    await ipc.setLightPower(lightId, on);
    recorded(lightId, { ...lightState(lightId), on });
  } catch (e) {
    fail(e);
  } finally {
    if (driven && !on) await refreshStatus();
    delete app.powerBusy[lightId];
  }
}

export function toggleLightPower(lightId: string): Promise<void> {
  return setLightPower(lightId, !isOn(lightId));
}

/** Every bound bulb on or off. Off also stops the effect (the backend does it). */
export async function setAllPower(on: boolean): Promise<void> {
  if (app.busy) return;
  app.busy = true;
  try {
    const res = await ipc.setAllPower(on);
    await reloadStates();
    if (!on) await refreshStatus();
    reportPartial(res);
  } catch (e) {
    fail(e);
  } finally {
    app.busy = false;
  }
}

export function reportPartial(res: PowerResult): void {
  if (res.failures.length > 0) {
    notify("error", t("partial_power", { ok: res.succeeded, total: res.total, names: res.failures.join(", ") }));
  }
}

// --- Setting lights by hand ---

// One call per light in flight; while it runs, only the newest wish waits.
// The first call reaches the bulb (seconds, for one that is unplugged), the
// rest only move a slider's value along an open stream.
const inFlight = new Set<string>();
const waiting = new Map<string, LightState>();

/** True while a wish for this light is on its way or queued, so the local
 *  value is newer than anything the backend has. */
const sending = (lightId: string) => inFlight.has(lightId) || waiting.has(lightId);

async function send(lightId: string, s: LightState): Promise<void> {
  if (inFlight.has(lightId)) {
    waiting.set(lightId, s);
    return;
  }
  inFlight.add(lightId);
  let failed = false;
  try {
    await ipc.setLightState(lightId, s);
  } catch (e) {
    // One toast per failure, not one per slider step.
    waiting.delete(lightId);
    failed = true;
    fail(e);
  } finally {
    inFlight.delete(lightId);
  }
  if (failed) {
    void revert(lightId);
    return;
  }
  const next = waiting.get(lightId);
  if (next) {
    waiting.delete(lightId);
    void send(lightId, next);
  }
}

/** Change part of a light's state (brightness, white, colour) and show it.
 *  Touching a light that is off turns it on. */
export function adjustLight(lightId: string, patch: Partial<LightState>): void {
  const now = lightState(lightId);
  const next: LightState = { ...now, on: true, ...patch };
  edit((c) => {
    c.states = { ...c.states, [lightId]: next };
    // The backend takes it off the natural light too, unless only its
    // brightness changed: then it keeps following, that much dimmer or brighter.
    if (!dimmedOnly(now, next)) c.natural = (c.natural ?? []).filter((id) => id !== lightId);
  });
  void send(lightId, next);
}

/** Set the brightness of `lightIds`, 1..=100: of each light set by hand,
 *  and of each effect one of them plays (an effect has one brightness, for
 *  every light playing it). Lights off are turned on. */
export function setBrightness(lightIds: string[], bright: number): void {
  const modes = new Set(lightIds.map(modeOf));
  if (modes.has("ambilight")) edit((c) => (c.engine.brightness = Math.max(0.05, bright / 100)));
  if ([...modes].some((m) => m !== null && m !== "ambilight")) edit((c) => (c.show.bright = Math.max(5, bright)));
  for (const id of lightIds) if (modeOf(id) === null) adjustLight(id, { bright });
}

/** Show what the backend recorded for a light the bulb refused, rather than
 *  a value it never got. */
async function revert(lightId: string): Promise<void> {
  try {
    const cfg = await ipc.getConfig();
    // Moved again meanwhile: that newer wish wins.
    if (sending(lightId)) return;
    const states = { ...app.config.states };
    const s = cfg.states?.[lightId];
    if (s) states[lightId] = s;
    else delete states[lightId];
    app.config.states = states;
    const natural = (app.config.natural ?? []).filter((id) => id !== lightId);
    if (cfg.natural?.includes(lightId)) natural.push(lightId);
    app.config.natural = natural;
  } catch {
    /* the next change will catch up */
  }
}

/** Bring the light states in line with what the backend recorded (after the
 *  tray, automatic mode or a scene changed them). Lights being set by hand
 *  keep their value, which is newer. */
export async function reloadStates(): Promise<void> {
  try {
    await persistNow();
    const cfg = await ipc.getConfig();
    const states = cfg.states ?? {};
    for (const [id, s] of Object.entries(app.config.states ?? {})) {
      if (sending(id)) states[id] = s;
    }
    app.config.states = states;
    app.config.natural = (cfg.natural ?? []).filter((id) => !sending(id));
  } catch {
    /* the next change will catch up */
  }
}

// --- Identify ---

// Warm white: an identify flash should read as "this one", never as a colour
// the effect could have picked.
const IDENTIFY_RGB: RgbTuple = [255, 190, 120];

export async function identify(endpoint: Endpoint): Promise<void> {
  try {
    await ipc.testDevice(endpoint, ...IDENTIFY_RGB);
  } catch (e) {
    fail(e);
  }
}

export async function identifyLight(lightId: string): Promise<void> {
  const b = bindingFor(lightId);
  if (!b) {
    notify("info", t("no_bulb_yet"));
    return;
  }
  return identify(endpointOf(b));
}
