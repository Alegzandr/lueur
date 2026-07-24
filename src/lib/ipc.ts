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

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) {
    throw new Error(`not running in Tauri; cannot call ${cmd}`);
  }
  return tauriInvoke<T>(cmd, args);
}

export async function getConfig(): Promise<AppConfig> {
  if (!inTauri()) return defaultConfig();
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
  if (!inTauri()) return [];
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
  if (!inTauri()) return { total: 0, succeeded: 0, failures: [] };
  return invoke<PowerResult>("set_all_power", { on });
}

export async function startEffect(): Promise<EffectStatus> {
  if (!inTauri()) return { running: true, connected_lights: 0 };
  return invoke<EffectStatus>("start_effect");
}

export async function stopEffect(): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("stop_effect");
}

export async function effectStatus(): Promise<EffectStatus> {
  if (!inTauri()) return { running: false, connected_lights: 0 };
  return invoke<EffectStatus>("effect_status");
}

export async function setRunOnStartup(enabled: boolean): Promise<void> {
  if (!inTauri()) return;
  return invoke<void>("set_run_on_startup", { enabled });
}
