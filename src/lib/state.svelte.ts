// Central app state, built on Svelte 5 runes so components read it directly
// (`app.config.room.lights`) and mutate it through `edit`, which persists the
// change to the backend.

import * as ipc from "./ipc";
import { t } from "./i18n";
import {
  defaultConfig,
  type AppConfig,
  type DeviceBinding,
  type DiscoveredDevice,
  type EffectStatus,
  type Light,
  type Vec2,
} from "./types";

export type View = "lighting" | "room" | "bulbs" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "error";
  msg: string;
  action?: { label: string; run: () => void };
}

class AppState {
  view = $state<View>("lighting");
  config = $state<AppConfig>(defaultConfig());
  status = $state<EffectStatus>({ running: false, connected_lights: 0 });
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

  // Last power state we *sent* to each light. Yeelight's LAN protocol has no
  // push channel, so this is an optimistic view, used only to draw buttons.
  power = $state<Record<string, boolean>>({});
  powerBusy = $state<Record<string, boolean>>({});

  /** The light selected on the plan, shared so other views can deep-link. */
  selectedLight = $state<string | null>(null);

  /** What the running effect was started with (see `topology`). */
  startedWith = $state<string | null>(null);
}

export const app = new AppState();

// --- Derived helpers (plain functions: they read runes, so callers stay reactive) ---

export function bindingFor(lightId: string): DeviceBinding | undefined {
  return app.config.bindings.find((b) => b.light_id === lightId);
}

export function lightById(id: string): Light | undefined {
  return app.config.room.lights.find((l) => l.id === id);
}

/**
 * The parts of the config a running effect only reads at start: the room,
 * which bulb drives which light, and the capture setup. Everything else is
 * pushed live.
 */
function topology(): string {
  const c = app.config;
  return JSON.stringify([c.room, c.bindings, c.capture]);
}

/** True when the running effect is out of date with the room as drawn. */
export function needsRestart(): boolean {
  return app.status.running && app.startedWith !== null && app.startedWith !== topology();
}

export function isOn(lightId: string): boolean {
  return app.power[lightId] ?? true;
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
    notify("error", t("err_save", { e: errMsg(e) }));
  }
}

let loaded = false;

export async function bootstrap(): Promise<void> {
  if (loaded) return;
  loaded = true;
  try {
    const cfg = await ipc.getConfig();
    // Older configs predate some interface fields.
    cfg.interface = { ...defaultConfig().interface, ...cfg.interface };
    // Someone upgrading with a furnished room has arranged it already.
    if (cfg.room.lights.length > 0 && cfg.bindings.length > 0 && !cfg.interface.room_arranged) {
      cfg.interface.room_arranged = true;
    }
    app.config = cfg;
    app.status = await ipc.effectStatus();
    // Running already (autostart): it was started from the saved config.
    if (app.status.running) app.startedWith = topology();
  } catch (e) {
    notify("error", t("err_load", { e: errMsg(e) }));
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

export async function toggleEffect(): Promise<void> {
  if (app.busy) return;
  app.busy = true;
  try {
    await persistNow();
    if (app.status.running) {
      await ipc.stopEffect();
      app.status = { running: false, connected_lights: 0 };
      app.startedWith = null;
    } else {
      app.status = await ipc.startEffect();
      app.startedWith = topology();
      if (app.status.connected_lights === 0) notify("error", t("started_silent"));
    }
  } catch (e) {
    notify("error", errMsg(e));
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
    await ipc.stopEffect();
    app.status = await ipc.startEffect();
    app.startedWith = topology();
  } catch (e) {
    await refreshStatus();
    notify("error", errMsg(e));
  } finally {
    app.busy = false;
  }
}

// --- Bulbs ---

export async function setLightPower(lightId: string, on: boolean): Promise<void> {
  app.powerBusy[lightId] = true;
  try {
    await ipc.setLightPower(lightId, on);
    app.power[lightId] = on;
  } catch (e) {
    notify("error", t("err_power", { e: errMsg(e) }));
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
    for (const b of app.config.bindings) app.power[b.light_id] = on;
    if (!on) await refreshStatus();
    if (res.failures.length > 0) {
      notify(
        "error",
        t("partial_power", { ok: res.succeeded, total: res.total, details: res.failures.join("; ") }),
      );
    }
  } catch (e) {
    notify("error", errMsg(e));
  } finally {
    app.busy = false;
  }
}

// Warm white: an identify flash should read as "this one", never as a colour
// the effect could have picked.
const IDENTIFY_RGB: [number, number, number] = [255, 190, 120];

export async function identify(ip: string, port: number): Promise<void> {
  try {
    await ipc.testDevice(ip, port, ...IDENTIFY_RGB);
  } catch (e) {
    notify("error", t("err_power", { e: errMsg(e) }));
  }
}

export async function identifyLight(lightId: string): Promise<void> {
  const b = bindingFor(lightId);
  if (!b) {
    notify("info", t("no_bulb_yet"));
    return;
  }
  return identify(b.ip, b.port);
}

/**
 * Search the network. Bound bulbs found at a new address are updated in
 * place: the device id is stable, the IP is not. `quiet` skips error toasts
 * for the background search at launch.
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
    if (moved.length > 0) {
      edit((c) => {
        for (const d of moved) {
          const b = c.bindings.find((x) => x.device_id === d.id);
          if (b) {
            b.ip = d.ip;
            b.port = d.port;
          }
        }
      });
      for (const d of moved) {
        const b = app.config.bindings.find((x) => x.device_id === d.id);
        const l = b ? lightById(b.light_id) : undefined;
        notify("info", t("readdressed", { name: l?.name ?? deviceName(d) }));
      }
    }
  } catch (e) {
    if (!quiet) notify("error", t("err_scan", { e: errMsg(e) }));
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
  const rad = (s.facing_deg * Math.PI) / 180;
  const f = { x: Math.sin(rad), y: Math.cos(rad) };
  const r = { x: f.y, y: -f.x };
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
  return { light_id: lightId, device_id: d.id, ip: d.ip, port: d.port, model: d.model, name: deviceName(d) };
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
  const binding = bindingFor(id) ? $state.snapshot(bindingFor(id)!) : undefined;
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

export function notify(kind: Toast["kind"], msg: string, action?: Toast["action"]): void {
  const id = ++toastSeq;
  app.toasts = [...app.toasts.slice(-2), { id, kind, msg, action }];
  setTimeout(() => dismiss(id), kind === "error" || action ? 7000 : 3600);
}

export function dismiss(id: number): void {
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
