// Central app state, built on Svelte 5 runes so components read it directly
// (`app.config.room.lights`) and mutate it through `edit`, which persists the
// change to the backend.

import * as ipc from "./ipc";
import { errorDetail, errorText, isBackendError, t } from "./i18n";
import { facing, rightAxis } from "./geometry";
import { DEFAULT_STATE, type RgbTuple } from "./scenes";
import {
  defaultConfig,
  type AppConfig,
  type DeviceBinding,
  type DiscoveredDevice,
  type EffectStatus,
  type Endpoint,
  type Light,
  type LightState,
  type Mode,
  type SleepStatus,
  type TuyaLogin,
  type Vec2,
} from "./types";

export type View = "lighting" | "room" | "bulbs" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "error";
  msg: string;
  /** Untranslated technical cause, shown on request and copied with the message. */
  detail?: string;
  action?: { label: string; run: () => void };
  /** How many times this same message came, when more than once. */
  count?: number;
  /** Bumped each time the toast should be read out: when it arrives, and
   *  when the same message comes again. */
  said: number;
}

/** A bridge or controller being paired: waiting for its button to be
 *  pressed, then how it went. */
export interface Pairing {
  device: DiscoveredDevice;
  step: "waiting" | "paired" | "failed";
  /** When the device stops waiting for its button (epoch ms). */
  until: number;
  /** Why it failed: the button was not pressed in time, or an error. */
  failure?: { expired: boolean; msg: string; detail?: string };
}

/** The Smart Life sign-in: the user code, the QR code to scan, then the
 *  bulbs looked for on the network and added. */
export type TuyaFlow =
  | { step: "code" | "starting"; expired?: boolean }
  | { step: "qr"; login: TuyaLogin }
  | { step: "adding"; imported: number }
  | {
      step: "done";
      imported: number;
      /** How many were added to the room as new lights. */
      added: number;
      /** Found but left for the user to give to lights already drawn. */
      ready: string[];
      /** Fewer bulbs answered than were imported. The backend only says how
       *  many it imported, not which, so this cannot be a count. */
      missing: boolean;
    };

/** What the backend reports while nothing runs. */
const idle = (): EffectStatus => ({
  running: false,
  connected_lights: 0,
  mode: null,
  light_ids: [],
  groups: [],
  reconnecting: [],
  music_waiting: false,
});

class AppState {
  view = $state<View>("lighting");
  config = $state<AppConfig>(defaultConfig());
  status = $state<EffectStatus>(idle());
  /** True while the effect is being started or stopped. */
  busy = $state(false);
  /** True once the backend config has arrived, so the UI does not flash the
   *  first-run guide at someone who already has a setup. */
  ready = $state(false);
  toasts = $state<Toast[]>([]);

  // Discovery results live here so they survive switching views.
  discovered = $state<DiscoveredDevice[]>([]);
  hasScanned = $state(false);
  scanning = $state(false);
  /** The bridge or controller being paired, until the user closes it. */
  pairing = $state<Pairing | null>(null);
  /** The Smart Life sign-in in progress, if any. */
  tuya = $state<TuyaFlow | null>(null);

  powerBusy = $state<Record<string, boolean>>({});
  /** The mood or mode being switched to, while it starts. */
  pending = $state<string | null>(null);
  /** Lights whose mode is being switched, while their effects restart. */
  modeBusy = $state<Record<string, boolean>>({});

  /** The lights effects and moods go to; none picked means the whole room. */
  target = $state<string[]>([]);

  /** The light selected on the plan, shared so other views can deep-link. */
  selectedLight = $state<string | null>(null);

  /** What the running effect was started with (see `topology`). */
  startedWith = $state<string | null>(null);

  /** The sleep timer, if set, and when it was last heard of (to count down). */
  sleep = $state<{ status: SleepStatus; at: number } | null>(null);

  /** Version installed in the background, applied at the next restart. */
  updateReady = $state<string | null>(null);

  /** Unverified bridges answered "not mine" this session, each with what it
   *  showed then (see `DeviceCheck`): asked again only if that changes. */
  dismissedDevices = $state<string[]>([]);
}

export const app = new AppState();

// --- Derived helpers (plain functions: they read runes, so callers stay reactive) ---

export function bindingFor(lightId: string): DeviceBinding | undefined {
  return app.config.bindings.find((b) => b.light_id === lightId);
}

export function lightById(id: string): Light | undefined {
  return app.config.room.lights.find((l) => l.id === id);
}

/** What the backend needs to reach a bound bulb. */
export function endpointOf(b: DeviceBinding): Endpoint {
  return { id: b.device_id, ip: b.ip, port: b.port, kind: b.kind ?? { family: "yeelight" } };
}

export function endpointOfDevice(d: DiscoveredDevice): Endpoint {
  return { id: d.id, ip: d.ip, port: d.port, kind: d.kind };
}

/**
 * The parts of the config a running effect only reads at start: the room,
 * which bulb drives which light, and the capture setup. Everything else,
 * the wall colour and the frame rate included, is pushed live.
 */
function topology(): string {
  const c = app.config;
  return JSON.stringify([
    { ...c.room, wall: undefined },
    c.bindings,
    { ...c.capture, target_fps: undefined },
  ]);
}

/** True when the running effect is out of date with the room as drawn. */
export function needsRestart(): boolean {
  return app.status.running && app.startedWith !== null && app.startedWith !== topology();
}

/**
 * What a light was last set to. Bulbs have no push channel back to us, so
 * this is what Lueur sent rather than what the bulb reports; a light nobody
 * has set yet reads as a warm white, on.
 */
export function lightState(lightId: string): LightState {
  return app.config.states?.[lightId] ?? DEFAULT_STATE;
}

export function isOn(lightId: string): boolean {
  return lightState(lightId).on;
}

/** True while the running ambilight or show sets this light's colour. */
export function isDriven(lightId: string): boolean {
  return app.status.running && app.status.light_ids.includes(lightId);
}

/** The mode a light plays, or null while it is set by hand. */
export function modeOf(lightId: string): Mode | null {
  return app.status.groups.find((g) => g.light_ids.includes(lightId))?.mode ?? null;
}

/** True while an effect drives this light but its bulb dropped and is being
 *  reconnected. */
export function isReconnecting(lightId: string): boolean {
  return app.status.reconnecting?.includes(lightId) ?? false;
}

/** The lights picked for effects and moods that still have a bulb, or
 *  null for the whole room. */
export function pickedLights(): string[] | null {
  const ids = app.target.filter((id) => bindingFor(id));
  return ids.length > 0 ? ids : null;
}

/** The lights an effect goes to: those picked, or every light with a bulb
 *  that takes part in effects. */
export function effectTargets(): string[] {
  return pickedLights() ?? app.config.room.lights.filter((l) => l.enabled && bindingFor(l.id)).map((l) => l.id);
}

/** The lights a mood goes to: those picked, or every light with a bulb. */
export function moodTargets(): string[] {
  return pickedLights() ?? app.config.bindings.map((b) => b.light_id);
}

/** Pick a light for effects and moods, or put it back. Picking them all is
 *  the same as the whole room. */
export function toggleTarget(lightId: string): void {
  const ids = app.target.includes(lightId) ? app.target.filter((id) => id !== lightId) : [...app.target, lightId];
  app.target = app.config.bindings.every((b) => ids.includes(b.light_id)) ? [] : ids;
}

/** True while some light plays `mode`. */
export function isPlaying(mode: Mode): boolean {
  return app.status.groups.some((g) => g.mode === mode);
}

/** Record a state the backend has already applied and persisted. */
function recorded(lightId: string, s: LightState): void {
  app.config.states = { ...app.config.states, [lightId]: s };
}

// --- Config editing and persistence ---

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

// --- Effect ---

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

// --- Bulbs ---

export async function setLightPower(lightId: string, on: boolean): Promise<void> {
  app.powerBusy[lightId] = true;
  try {
    await ipc.setLightPower(lightId, on);
    recorded(lightId, { ...lightState(lightId), on });
  } catch (e) {
    fail(e);
  } finally {
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

function reportPartial(res: { total: number; succeeded: number; failures: string[] }): void {
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
  const next: LightState = { ...lightState(lightId), on: true, ...patch };
  edit((c) => {
    c.states = { ...c.states, [lightId]: next };
    // The backend takes it off the natural light too.
    c.natural = (c.natural ?? []).filter((id) => id !== lightId);
  });
  void send(lightId, next);
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
async function moveLights(ids: string[], mode: Mode | null): Promise<void> {
  const wasRunning = app.status.running;
  app.status = await ipc.setLightsMode(ids, mode);
  if (!app.status.running) app.startedWith = null;
  else if (!wasRunning || app.startedWith === null) app.startedWith = topology();
}

/** Play `mode` on one light, or give it back to the hand with null. */
export async function setLightMode(lightId: string, mode: Mode | null): Promise<void> {
  if (app.busy || app.modeBusy[lightId] || modeOf(lightId) === mode) return;
  app.modeBusy[lightId] = true;
  app.busy = true;
  try {
    await persistNow();
    await moveLights([lightId], mode);
    if (mode === null) await reloadStates();
  } catch (e) {
    await refreshStatus();
    fail(e);
  } finally {
    app.busy = false;
    delete app.modeBusy[lightId];
  }
}

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

/** The backend changed what only it writes (pairings, Tuya keys): reload. */
async function reloadConfig(): Promise<void> {
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

/**
 * Search the network. Bound bulbs found at a new address are updated in
 * place: the device id is stable, the IP is not (nor, for a Tuya bulb, the
 * protocol version it was found speaking). `quiet` skips error toasts for the
 * background search at launch.
 */
export async function scan(timeoutMs = 3000, quiet = false): Promise<void> {
  if (app.scanning) return;
  app.scanning = true;
  try {
    const found = await ipc.discoverDevices(timeoutMs);
    app.discovered = found;
    app.hasScanned = true;
    const moved = found.filter((d) =>
      app.config.bindings.some((b) => b.device_id === d.id && (b.ip !== d.ip || b.port !== d.port)),
    );
    const changed = found.filter((d) =>
      app.config.bindings.some(
        (b) => b.device_id === d.id && JSON.stringify(endpointOf(b)) !== JSON.stringify(endpointOfDevice(d)),
      ),
    );
    if (changed.length > 0) {
      edit((c) => {
        for (const d of changed) {
          const b = c.bindings.find((x) => x.device_id === d.id);
          if (b) {
            b.ip = d.ip;
            b.port = d.port;
            b.kind = $state.snapshot(d.kind);
          }
        }
      });
    }
    for (const d of moved) {
      const b = app.config.bindings.find((x) => x.device_id === d.id);
      const l = b ? lightById(b.light_id) : undefined;
      notify("info", t("readdressed", { name: l?.name ?? deviceName(d) }));
    }
  } catch (e) {
    if (!quiet) fail(e);
  } finally {
    app.scanning = false;
  }
}

/** The bulb's own name if it has one; otherwise a numbered bulb, not to be
 *  taken for one of the room's lights. */
export function deviceName(d: DiscoveredDevice): string {
  const own = d.name?.trim();
  if (own) return own;
  const i = app.discovered.findIndex((x) => x.id === d.id);
  return t("bulb_n", { n: (i < 0 ? app.discovered.length : i) + 1 });
}

/**
 * Where new lights go: in a row just behind the screen, the usual ambilight
 * setup, spread evenly across its width. A spot already taken is skipped, so
 * lights never land on top of each other; the user drags them to their real
 * places afterwards.
 */
function freshPositions(count: number): Vec2[] {
  const s = app.config.room.screen;
  const f = facing(s);
  const r = rightAxis(s);
  const at = (u: number, depth: number): Vec2 => ({
    x: Math.round(Math.max(20, Math.min(980, s.center.x + r.x * u * s.width + f.x * depth))),
    y: Math.round(Math.max(20, Math.min(730, s.center.y + r.y * u * s.width + f.y * depth))),
  });
  const taken = app.config.room.lights.map((l) => l.pos);
  const free = (p: Vec2) => taken.every((q) => Math.hypot(p.x - q.x, p.y - q.y) > 45);

  const out: Vec2[] = [];
  for (let i = 0; i < count; i++) {
    // Even spread across 120% of the screen width, one row behind it.
    let p = at(count === 1 ? 0 : -0.6 + (1.2 * i) / (count - 1), -45);
    // Taken: walk outwards along further rows until a free spot turns up.
    for (let k = 0; !free(p) && k < 40; k++) {
      const u = ((k % 7) - 3) * 0.3;
      p = at(u, -45 - Math.floor(k / 7) * 55 + (k >= 7 ? 140 : 0));
    }
    taken.push(p);
    out.push(p);
  }
  return out;
}

function newLight(name: string, pos: Vec2): Light {
  return { id: crypto.randomUUID(), name, pos, height: 0.5, zone_override: null, enabled: true };
}

export function addLight(name?: string): Light {
  const light = newLight(name ?? t("light_n", { n: app.config.room.lights.length + 1 }), freshPositions(1)[0]);
  edit((c) => {
    c.room.lights.push(light);
  });
  return light;
}

function bindingOf(lightId: string, d: DiscoveredDevice): DeviceBinding {
  return {
    light_id: lightId,
    device_id: d.id,
    ip: d.ip,
    port: d.port,
    model: d.model,
    name: deviceName(d),
    kind: $state.snapshot(d.kind),
  };
}

/** Add discovered bulbs to the room as new lights. `announce` is false when
 *  the page already says so. */
export function addDevices(devices: DiscoveredDevice[], announce = true): void {
  if (devices.length === 0) return;
  const added: [Light, DiscoveredDevice][] = [];
  const spots = freshPositions(devices.length);
  edit((c) => {
    for (const [i, d] of devices.entries()) {
      const light = newLight(deviceName(d), spots[i]);
      c.room.lights.push(light);
      c.bindings.push(bindingOf(light.id, d));
      added.push([light, d]);
    }
  });
  if (!announce) return;
  notify(
    "info",
    added.length === 1 ? t("added", { name: added[0][0].name }) : t("added_many", { n: added.length }),
  );
}

/** Point an existing light at a discovered bulb (replacing any previous one). */
export function assignDevice(lightId: string, d: DiscoveredDevice): void {
  edit((c) => {
    c.bindings = c.bindings.filter((b) => b.light_id !== lightId && b.device_id !== d.id);
    c.bindings.push(bindingOf(lightId, d));
  });
}

export function unassign(lightId: string): void {
  edit((c) => {
    c.bindings = c.bindings.filter((b) => b.light_id !== lightId);
  });
}

export function renameLight(id: string, name: string): void {
  edit((c) => {
    const l = c.room.lights.find((x) => x.id === id);
    if (l) l.name = name;
  });
}

export function setLightEnabled(id: string, enabled: boolean): void {
  edit((c) => {
    const l = c.room.lights.find((x) => x.id === id);
    if (l) l.enabled = enabled;
  });
}

/** Remove a light (and its bulb assignment), with an undo in the toast. */
export function removeLight(id: string): void {
  const index = app.config.room.lights.findIndex((l) => l.id === id);
  if (index < 0) return;
  const light = $state.snapshot(app.config.room.lights[index]);
  const bound = bindingFor(id);
  const binding = bound ? $state.snapshot(bound) : undefined;
  edit((c) => {
    c.room.lights.splice(index, 1);
    c.bindings = c.bindings.filter((x) => x.light_id !== id);
  });
  if (app.selectedLight === id) app.selectedLight = null;
  notify("info", t("removed", { name: light.name }), {
    label: t("undo"),
    run: () =>
      edit((c) => {
        c.room.lights.splice(Math.min(index, c.room.lights.length), 0, light);
        // The bulb went to another light since: it stays there.
        if (binding && !c.bindings.some((b) => b.device_id === binding.device_id)) c.bindings.push(binding);
      }),
  });
}

/** The plan has been arranged: the first-run guide moves on. */
export function markRoomArranged(): void {
  if (!app.config.interface.room_arranged) edit((c) => (c.interface.room_arranged = true));
}

// --- Notifications ---

let toastSeq = 0;

/** Each toast's countdown: what is left of it, and what pauses it (the
 *  pointer or focus on the toast). */
interface Countdown {
  handle: ReturnType<typeof setTimeout> | null;
  left: number;
  since: number;
  held: Set<"hover" | "focus">;
}
const countdowns = new Map<number, Countdown>();

/** At most this many toasts show at once. */
const MAX_TOASTS = 4;

/** Read-out counter shared by every toast (see `Toast.said`). */
let saidSeq = 0;

/** Start (or start over) an information toast's countdown. */
function countDown(id: number, left: number): void {
  const old = countdowns.get(id);
  if (old?.handle) clearTimeout(old.handle);
  const held = old?.held ?? new Set<"hover" | "focus">();
  // Held by the pointer or focus: it starts once they move on.
  const handle = held.size > 0 ? null : setTimeout(() => dismiss(id), left);
  countdowns.set(id, { handle, left, since: Date.now(), held });
}

/**
 * Show a toast. Errors stay until closed, so there is time to read them and
 * pass them on; information goes by itself, later when it has an action.
 *
 * The same message again counts up on the toast showing, which keeps its
 * place, focus and unfolded details, and is read out again. Toasts with an
 * action never merge: each "Undo" undoes its own change. Past the limit the
 * oldest information goes first; information never pushes out an error.
 */
function toast(entry: Omit<Toast, "id" | "said">): void {
  const left = entry.action ? 7000 : 3600;
  const same = entry.action
    ? undefined
    : app.toasts.find((t) => !t.action && t.kind === entry.kind && t.msg === entry.msg && t.detail === entry.detail);
  if (same) {
    same.count = (same.count ?? 1) + 1;
    same.said = ++saidSeq;
    if (same.kind === "info") countDown(same.id, left);
    return;
  }
  while (app.toasts.length >= MAX_TOASTS) {
    const oldest = app.toasts.find((t) => t.kind === "info") ?? (entry.kind === "error" ? app.toasts[0] : undefined);
    // Only errors showing and this is information: it shows over the limit
    // for its few seconds rather than take an error's place.
    if (!oldest) break;
    dismiss(oldest.id);
  }
  const id = ++toastSeq;
  app.toasts = [...app.toasts, { id, ...entry, said: ++saidSeq }];
  if (entry.kind === "info") countDown(id, left);
}

/** Pause a toast's countdown while it is pointed at or focused. */
export function holdToast(id: number, why: "hover" | "focus"): void {
  const c = countdowns.get(id);
  if (!c) return;
  c.held.add(why);
  if (!c.handle) return;
  clearTimeout(c.handle);
  c.handle = null;
  c.left -= Date.now() - c.since;
}

/** Resume it once nothing holds it, with a moment left to finish reading. */
export function releaseToast(id: number, why: "hover" | "focus"): void {
  const c = countdowns.get(id);
  if (!c) return;
  c.held.delete(why);
  if (c.handle || c.held.size > 0) return;
  c.left = Math.max(c.left, 2000);
  c.since = Date.now();
  c.handle = setTimeout(() => dismiss(id), c.left);
}

export function notify(kind: Toast["kind"], msg: string, action?: Toast["action"]): void {
  toast({ kind, msg, action });
}

export function dismiss(id: number): void {
  const c = countdowns.get(id);
  if (c?.handle) clearTimeout(c.handle);
  countdowns.delete(id);
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

/**
 * Report a failure. Backend errors arrive as codes and are worded in the
 * interface language; `fallback` words anything else (a frontend-only step).
 */
export function fail(
  e: unknown,
  fallback?: "err_save" | "err_load" | "err_displays",
  action?: Toast["action"],
): void {
  const msg = isBackendError(e) || !fallback ? errorText(e) : t(fallback);
  toast({ kind: "error", msg, detail: errorDetail(e), action });
}
