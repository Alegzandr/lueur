// Derived helpers: plain functions that read runes, so callers stay reactive.

import { DEFAULT_STATE } from "../scenes";
import type { DeviceBinding, DiscoveredDevice, Endpoint, Light, LightState, Mode } from "../types";
import { app } from "./app.svelte";

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
export function topology(): string {
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

/** A light an effect drives is on, whatever it was last set to by hand. */
export function isOn(lightId: string): boolean {
  return isDriven(lightId) || lightState(lightId).on;
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

/** What else drives this light's bulb: another computer running Lueur (its
 *  name), another app (an empty string), or nothing (null). */
export function drivenElsewhere(lightId: string): string | null {
  const e = app.status.elsewhere?.find((x) => x.light_id === lightId);
  return e ? (e.by ?? "") : null;
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

/** How bright a light is, 1..=100: as set by hand, or the brightness of the
 *  effect it plays, which every light playing it shares. */
export function brightOf(lightId: string): number {
  const mode = modeOf(lightId);
  if (mode === "ambilight") return Math.round(app.config.engine.brightness * 100);
  if (mode) return app.config.show.bright;
  return lightState(lightId).bright;
}

/** True while some light plays `mode`. */
export function isPlaying(mode: Mode): boolean {
  return app.status.groups.some((g) => g.mode === mode);
}

/** Record a state the backend has already applied and persisted. */
export function recorded(lightId: string, s: LightState): void {
  app.config.states = { ...app.config.states, [lightId]: s };
}
