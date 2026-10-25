// Central app state, built on Svelte 5 runes so components read it directly
// (`app.config.room.lights`) and mutate it through `edit`, which persists the
// change to the backend.

import * as ipc from "./ipc";
import { errorDetail, errorText, isBackendError, t, tn } from "./i18n";
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
  /** Untranslated technical cause, shown on hover. */
  detail?: string;
  action?: { label: string; run: () => void };
}

/** What the backend reports while nothing runs. */
const idle = (): EffectStatus => ({ running: false, connected_lights: 0, mode: null, light_ids: [], groups: [] });

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
  /** The bridge or controller waiting for its button to be pressed. */
  pairing = $state<string | null>(null);
  /** The Smart Life sign-in in progress, if any. */
  tuya = $state<{ step: "code" | "starting" | "qr"; login?: TuyaLogin } | null>(null);

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
 * the wall colour included, is pushed live.
 */
function topology(): string {
  const c = app.config;
  return JSON.stringify([{ ...c.room, wall: undefined }, c.bindings, c.capture]);
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
    app.status = await ipc.effectStatus();
    sleepChanged(await ipc.sleepTimer());
    // Running already (resumed at launch): it was started from the saved config.
    if (app.status.running) app.startedWith = topology();
  } catch (e) {
    fail(e, "err_load");
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

async function send(lightId: string, s: LightState): Promise<void> {
  if (inFlight.has(lightId)) {
    waiting.set(lightId, s);
    return;
  }
  inFlight.add(lightId);
  try {
    await ipc.setLightState(lightId, s);
  } catch (e) {
    // One toast per failure, not one per slider step.
    waiting.delete(lightId);
    fail(e);
  } finally {
    inFlight.delete(lightId);
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

/** Bring the light states in line with what the backend recorded (after the
 *  tray, automatic mode or a scene changed them). */
export async function reloadStates(): Promise<void> {
  try {
    await persistNow();
    const cfg = await ipc.getConfig();
    app.config.states = cfg.states ?? {};
    app.config.natural = cfg.natural ?? [];
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
  try {
    await persistNow();
    await moveLights([lightId], mode);
    if (mode === null) await reloadStates();
  } catch (e) {
    await refreshStatus();
    fail(e);
  } finally {
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
    notify("info", t("imported"));
    if (cfg.bindings.length > 0) void scan(3000, true);
  } catch (e) {
    fail(e);
  }
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * Pair with a Hue bridge or Nanoleaf controller: ask the user to press its
 * button, and keep asking the device for up to 30 seconds.
 */
export async function pair(d: DiscoveredDevice): Promise<void> {
  if (app.pairing) return;
  app.pairing = d.id;
  notify("info", t(d.kind.family === "hue" ? "pair_hue" : "pair_nanoleaf"));
  try {
    const deadline = Date.now() + 30_000;
    for (;;) {
      try {
        await ipc.pairDevice($state.snapshot(d));
        break;
      } catch (e) {
        const waiting = isBackendError(e) && e.code === "pair_waiting";
        if (!waiting || Date.now() > deadline) throw e;
        await sleep(1500);
      }
    }
    await reloadConfig();
    notify("info", t("paired"));
    app.pairing = null;
    await scan();
  } catch (e) {
    fail(e);
  } finally {
    app.pairing = null;
  }
}

/** Open the Smart Life sign-in, at the user-code step. */
export function openTuya(): void {
  app.tuya = { step: "code" };
}

export function closeTuya(): void {
  app.tuya = null;
}

/**
 * Sign in to Smart Life with the user code: show the QR code, wait (three
 * minutes at most) for it to be scanned, import the bulbs and search for them.
 */
export async function signInTuya(userCode: string): Promise<void> {
  if (!app.tuya || app.tuya.step !== "code") return;
  app.tuya = { step: "starting" };
  try {
    const login = await ipc.tuyaLoginStart(userCode.trim());
    if (!app.tuya) return;
    app.tuya = { step: "qr", login };
    const deadline = Date.now() + 180_000;
    while (app.tuya?.login?.token === login.token) {
      const n = await ipc.tuyaLoginPoll(login.token, userCode.trim());
      if (n !== null) {
        app.tuya = null;
        await reloadConfig();
        notify("info", n === 0 ? t("tuya_none") : tn("tuya_imported", n));
        if (n > 0) await scan();
        return;
      }
      if (Date.now() > deadline) {
        app.tuya = { step: "code" };
        notify("info", t("tuya_expired"));
        return;
      }
      await sleep(2000);
    }
  } catch (e) {
    if (app.tuya) app.tuya = { step: "code" };
    fail(e);
  }
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

/** The bulb's own name if it has one; otherwise a numbered light name. */
export function deviceName(d: DiscoveredDevice): string {
  const own = d.name?.trim();
  if (own) return own;
  const i = app.discovered.findIndex((x) => x.id === d.id);
  return t("light_n", { n: (i < 0 ? app.discovered.length : i) + 1 });
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

/** Add discovered bulbs to the room as new lights. */
export function addDevices(devices: DiscoveredDevice[]): void {
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
        if (binding) c.bindings.push(binding);
      }),
  });
}

/** The plan has been arranged: the first-run guide moves on. */
export function markRoomArranged(): void {
  if (!app.config.interface.room_arranged) edit((c) => (c.interface.room_arranged = true));
}

// --- Notifications ---

let toastSeq = 0;

/** Show a toast, keeping at most three. Errors and toasts with an action
 *  stay longer, so there is time to read or act on them. */
function toast(entry: Omit<Toast, "id">): void {
  const id = ++toastSeq;
  app.toasts = [...app.toasts.slice(-2), { id, ...entry }];
  setTimeout(() => dismiss(id), entry.kind === "error" || entry.action ? 7000 : 3600);
}

export function notify(kind: Toast["kind"], msg: string, action?: Toast["action"]): void {
  toast({ kind, msg, action });
}

export function dismiss(id: number): void {
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

/**
 * Report a failure. Backend errors arrive as codes and are worded in the
 * interface language; `fallback` words anything else (a frontend-only step).
 */
export function fail(e: unknown, fallback?: "err_save" | "err_load" | "err_displays"): void {
  const msg = isBackendError(e) || !fallback ? errorText(e) : t(fallback);
  toast({ kind: "error", msg, detail: errorDetail(e) });
}
