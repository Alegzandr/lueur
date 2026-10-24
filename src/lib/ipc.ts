// Thin, typed wrappers over the Tauri command layer.
//
// Every call degrades gracefully when running outside a Tauri window (e.g. a
// plain `vite` browser preview), so the UI can be developed without the shell.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import {
  defaultConfig,
  type AppConfig,
  type DiscoveredDevice,
  type EffectStatus,
  type MonitorInfo,
  type PowerResult,
} from "./types";

/** True when we're running inside the Tauri webview. */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Browser preview: `?demo` gives a furnished room to design against, `?fresh`
 *  a first launch with bulbs waiting on the network, `?update` an installed
 *  update waiting for a restart. */
function preview(flag: "demo" | "fresh" | "update"): boolean {
  return typeof location !== "undefined" && new URLSearchParams(location.search).has(flag);
}
const demo = () => preview("demo");

function demoConfig(): AppConfig {
  const c = defaultConfig();
  const lights: [string, number, number, number][] = [
    ["Behind left", 380, 120, 0.6],
    ["Behind right", 620, 120, 0.6],
    ["Floor lamp", 860, 420, 0.3],
    ["Shelf", 140, 560, 0.8],
  ];
  c.room.lights = lights.map(([name, x, y, height], i) => ({
    id: `demo-${i}`,
    name,
    pos: { x, y },
    height,
    zone_override: null,
    enabled: i !== 3,
  }));
  c.bindings = c.room.lights.slice(0, 3).map((l, i) => ({
    light_id: l.id,
    device_id: `0x${i}`,
    ip: `192.168.1.${20 + i}`,
    port: 55443,
    model: "color",
    name: `Yeelight Color ${i + 1}`,
  }));
  return c;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) {
    throw new Error(`not running in Tauri; cannot call ${cmd}`);
  }
  return tauriInvoke<T>(cmd, args);
}

export async function getConfig(): Promise<AppConfig> {
  if (!inTauri()) return demo() ? demoConfig() : defaultConfig();
  return invoke<AppConfig>("get_config");
}

export async function saveConfig(config: AppConfig): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("save_config", { config });
}

export async function listMonitors(): Promise<MonitorInfo[]> {
  if (!inTauri()) {
    return [{ index: 0, name: "Preview Display", width: 2560, height: 1440, is_primary: true }];
  }
  return invoke<MonitorInfo[]>("list_monitors");
}

export async function discoverDevices(timeoutMs = 2500): Promise<DiscoveredDevice[]> {
  if (!inTauri()) {
    if (!demo() && !preview("fresh")) return [];
    await new Promise((r) => setTimeout(r, 600));
    return [0, 1, 2, 3].map((i) => ({
      id: `0x${i}`,
      ip: `192.168.1.${20 + i}`,
      port: 55443,
      model: "color",
      // Most bulbs are never named in the Yeelight app.
      name: demo() && i < 3 ? `Yeelight Color ${i + 1}` : "",
      support: ["set_music", "set_rgb"],
      fw_ver: "26",
    }));
  }
  return invoke<DiscoveredDevice[]>("discover_devices", { timeoutMs });
}

export async function testDevice(
  ip: string,
  port: number,
  r: number,
  g: number,
  b: number,
): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("test_device", { ip, port, r, g, b });
}

/** Power a bulb on/off by address. */
export async function setDevicePower(ip: string, port: number, on: boolean): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("set_device_power", { ip, port, on });
}

/** Power the bulb bound to a room light on/off. */
export async function setLightPower(lightId: string, on: boolean): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("set_light_power", { lightId, on });
}

/** Power every bound bulb on/off. Turning off also stops the effect. */
export async function setAllPower(on: boolean): Promise<PowerResult> {
  if (!inTauri()) {
    if (!on) previewRunning = false;
    return { total: 0, succeeded: 0, failures: [] };
  }
  return invoke<PowerResult>("set_all_power", { on });
}

// The browser preview pretends to run, so the UI can be exercised end to end.
let previewRunning = false;
const previewStatus = (): EffectStatus => ({
  running: previewRunning,
  connected_lights: previewRunning && (demo() || preview("fresh")) ? 3 : 0,
});

export async function startEffect(): Promise<EffectStatus> {
  if (!inTauri()) {
    previewRunning = true;
    return previewStatus();
  }
  return invoke<EffectStatus>("start_effect");
}

export async function stopEffect(): Promise<void> {
  if (!inTauri()) {
    previewRunning = false;
    return;
  }
  return invoke<void>("stop_effect");
}

export async function effectStatus(): Promise<EffectStatus> {
  if (!inTauri()) return previewStatus();
  return invoke<EffectStatus>("effect_status");
}

export async function setRunOnStartup(enabled: boolean): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("set_run_on_startup", { enabled });
}

/** Match the native title bar to the chosen appearance (null follows the OS). */
export async function setWindowTheme(theme: "light" | "dark" | null): Promise<void> {
  if (!inTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().setTheme(theme);
}

export async function appVersion(): Promise<string> {
  if (!inTauri()) return "dev";
  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}

/** Tell the shell which language the interface uses, for the tray menu. */
export async function setUiLanguage(lang: "en" | "fr"): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("set_ui_language", { lang });
}

/** Called when the effect is started or stopped from the tray. The payload
 *  is an error message when a start failed. */
export async function onEffectChanged(cb: (error: unknown) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>("effect-changed", (e) => cb(e.payload));
}

/** The version installed in the background and waiting for a restart. */
export async function updateReady(): Promise<string | null> {
  if (!inTauri()) return preview("update") ? "9.9.9" : null;
  return invoke<string | null>("update_ready");
}

/** Called when an update has been installed in the background. */
export async function onUpdateReady(cb: (version: string) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("update-ready", (e) => cb(e.payload));
}

/** Restart into the installed update. */
export async function restartApp(): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("restart_app");
}
