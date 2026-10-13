// Central app state, built on Svelte 5 runes so components read it directly
// (`app.config.room.lights`) and mutate it through `edit`, which persists the
// change to the backend.

import * as ipc from "./ipc";
import {
  defaultConfig,
  type AppConfig,
  type DiscoveredDevice,
  type EffectStatus,
  type Light,
} from "./types";

export type View = "overview" | "room" | "bulbs" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "error";
  msg: string;
}

class AppState {
  view = $state<View>("overview");
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

export function bindingFor(lightId: string) {
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
    notify("error", `Could not save settings: ${errMsg(e)}`);
  }
}

let loaded = false;

export async function bootstrap(): Promise<void> {
  if (loaded) return;
  loaded = true;
  try {
    const cfg = await ipc.getConfig();
    // Older configs predate the interface block.
    cfg.interface ??= { appearance: "system" };
    app.config = cfg;
    app.status = await ipc.effectStatus();
    // Running already (autostart): it was started from the saved config.
    if (app.status.running) app.startedWith = topology();
  } catch (e) {
    notify("error", `Could not load settings: ${errMsg(e)}`);
  } finally {
    app.ready = true;
  }
}

export async function refreshStatus(): Promise<void> {
  try {
    app.status = await ipc.effectStatus();
    if (!app.status.running) app.startedWith = null;
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
      if (app.status.connected_lights === 0) {
        notify("error", "Ambilight started, but no bulb answered. Check they are powered and on the network.");
      }
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
    notify("error", `Could not turn the bulb ${on ? "on" : "off"}: ${errMsg(e)}`);
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
        `${res.succeeded} of ${res.total} bulbs answered. ${res.failures.join("; ")}`,
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
    notify("error", `Could not reach the bulb: ${errMsg(e)}`);
  }
}

export async function identifyLight(lightId: string): Promise<void> {
  const b = bindingFor(lightId);
  if (!b) {
    notify("info", "This light has no bulb assigned yet.");
    return;
  }
  return identify(b.ip, b.port);
}

export async function scan(timeoutMs = 3000): Promise<void> {
  if (app.scanning) return;
  app.scanning = true;
  try {
    app.discovered = await ipc.discoverDevices(timeoutMs);
    app.hasScanned = true;
  } catch (e) {
    notify("error", `Scan failed: ${errMsg(e)}`);
  } finally {
    app.scanning = false;
  }
}

export function deviceName(d: DiscoveredDevice): string {
  return d.name?.trim() || `Yeelight ${d.model || "bulb"}`;
}

/** Place a new light in front of the screen, staggered so lights never stack. */
function newLight(name: string): Light {
  const s = app.config.room.screen;
  const n = app.config.room.lights.length;
  const rad = (s.facing_deg * Math.PI) / 180;
  const f = { x: Math.sin(rad), y: Math.cos(rad) };
  const r = { x: f.y, y: -f.x };
  const along = ((n % 5) - 2) * (s.width * 0.45);
  const depth = 70 + Math.floor(n / 5) * 70;
  return {
    id: crypto.randomUUID(),
    name,
    pos: {
      x: Math.round(s.center.x + r.x * along + f.x * depth),
      y: Math.round(s.center.y + r.y * along + f.y * depth),
    },
    height: 0.5,
    zone_override: null,
    enabled: true,
  };
}

export function addLight(name?: string): Light {
  const light = newLight(name ?? `Light ${app.config.room.lights.length + 1}`);
  edit((c) => {
    c.room.lights.push(light);
  });
  return light;
}

/** Add a discovered bulb to the room as a new light. */
export function addDevice(d: DiscoveredDevice): void {
  const light = newLight(deviceName(d));
  edit((c) => {
    c.room.lights.push(light);
    c.bindings.push({
      light_id: light.id,
      device_id: d.id,
      ip: d.ip,
      port: d.port,
      model: d.model,
      name: deviceName(d),
    });
  });
  notify("info", `${deviceName(d)} added to the room.`);
}

/** Point an existing light at a discovered bulb (replacing any previous one). */
export function assignDevice(lightId: string, d: DiscoveredDevice): void {
  edit((c) => {
    c.bindings = c.bindings.filter((b) => b.light_id !== lightId && b.device_id !== d.id);
    c.bindings.push({
      light_id: lightId,
      device_id: d.id,
      ip: d.ip,
      port: d.port,
      model: d.model,
      name: deviceName(d),
    });
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

export function removeLight(id: string): void {
  const l = lightById(id);
  edit((c) => {
    c.room.lights = c.room.lights.filter((x) => x.id !== id);
    c.bindings = c.bindings.filter((x) => x.light_id !== id);
  });
  if (app.selectedLight === id) app.selectedLight = null;
  if (l) notify("info", `${l.name} removed.`);
}

// --- Notifications ---

let toastSeq = 0;

export function notify(kind: Toast["kind"], msg: string): void {
  const id = ++toastSeq;
  app.toasts = [...app.toasts.slice(-2), { id, kind, msg }];
  setTimeout(() => dismiss(id), kind === "error" ? 7000 : 3600);
}

export function dismiss(id: number): void {
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

/** "1 light", "3 lights". Every user-facing count goes through this. */
export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
