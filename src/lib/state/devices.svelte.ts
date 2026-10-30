// Bulbs found on the network and the lights of the room: searching, adding,
// assigning, renaming and removing.

import * as ipc from "../ipc";
import { t } from "../i18n";
import { facing, rightAxis } from "../geometry";
import type { DeviceBinding, DiscoveredDevice, Light, Vec2 } from "../types";
import { app } from "./app.svelte";
import { edit } from "./config.svelte";
import { bindingFor, endpointOf, endpointOfDevice, lightById } from "./derived";
import { fail, notify } from "./toasts";

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

/** "Light n" for the smallest n no light of the room is called yet, so a
 *  light added after a removal never takes a name already in use. */
function freshName(): string {
  const taken = new Set(app.config.room.lights.map((l) => l.name));
  let n = app.config.room.lights.length + 1;
  for (let i = 1; i <= n; i++) {
    if (!taken.has(t("light_n", { n: i }))) {
      n = i;
      break;
    }
  }
  return t("light_n", { n });
}

/** The longest name a light can have, as the fields that rename it allow. */
export const MAX_LIGHT_NAME = 60;

export function addLight(name?: string): Light {
  const light = newLight(name ?? freshName(), freshPositions(1)[0]);
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
    if (l) l.name = name.slice(0, MAX_LIGHT_NAME);
  });
}

/** Once its field is left, a light without a name gets back the one it had
 *  (`before`), or a fresh one: a nameless light would leave its card, its row
 *  and its pin on the plan blank. */
export function settleName(id: string, before = ""): void {
  const l = lightById(id);
  if (!l) return;
  const name = l.name.trim() || before.trim() || freshName();
  if (name !== l.name) renameLight(id, name);
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
        // Already back, by the plan's own undo: once is enough.
        if (c.room.lights.some((l) => l.id === light.id)) return;
        c.room.lights.splice(Math.min(index, c.room.lights.length), 0, light);
        // The bulb went to another light since: it stays there.
        if (binding && !c.bindings.some((b) => b.device_id === binding.device_id)) c.bindings.push(binding);
      }),
  });
}

/** The plan has been arranged: the first-run guide moves on. */
export function markRoomArranged(): void {
  if (app.config.interface.room_arranged) return;
  app.lastStepPending = true;
  edit((c) => {
    c.interface.room_arranged = true;
  });
}

/** Open the room plan with light `id` picked, to see or move it. */
export function showOnPlan(id: string): void {
  app.selectedLight = id;
  app.view = "room";
}
