// Undo and redo for the room plan. A step is a snapshot of the room and of
// which bulb drives which light (removing a light drops its bulb too).
// Changes made in one go (a drag, a slider, typing a name) settle into a
// single step.

import { app, edit } from "./state.svelte";
import type { DeviceBinding, Room } from "./types";

interface Snap {
  room: Room;
  bindings: DeviceBinding[];
}

const LIMIT = 100;
/** Quiet time after which a burst of changes becomes a step. */
const SETTLE = 500;

const past: Snap[] = [];
const future: Snap[] = [];
/** The state the newest step ended on, and its key. */
let base: Snap | null = null;
let baseKey = "";
let timer: ReturnType<typeof setTimeout> | null = null;

export const history = $state({ canUndo: false, canRedo: false });

/**
 * What a step compares: the room and the pairings, not the bulbs' addresses,
 * which a background search updates on its own.
 */
export function planKey(): string {
  const pairs = app.config.bindings.map((b) => [b.light_id, b.device_id]);
  return JSON.stringify([app.config.room, pairs]);
}

function current(): Snap {
  return $state.snapshot({ room: app.config.room, bindings: app.config.bindings });
}

function refresh(dirty: boolean): void {
  history.canUndo = past.length > 0 || dirty;
  history.canRedo = future.length > 0 && !dirty;
}

/** Start from the config as it is, unless it is where the history left off
 *  (the plan was not touched elsewhere since). */
export function track(): void {
  const key = planKey();
  if (base && key === baseKey) return;
  past.length = 0;
  future.length = 0;
  base = current();
  baseKey = key;
  refresh(false);
}

/** The plan changed (or a drag ended). While `held`, the step stays open. */
export function observe(key: string, held: boolean): void {
  if (timer) clearTimeout(timer);
  timer = null;
  const dirty = key !== baseKey;
  refresh(dirty);
  if (dirty && !held) timer = setTimeout(commit, SETTLE);
}

/** Close the open step, if the plan moved since the last one. */
function commit(): void {
  if (timer) clearTimeout(timer);
  timer = null;
  const key = planKey();
  if (!base || key === baseKey) return;
  past.push(base);
  if (past.length > LIMIT) past.shift();
  future.length = 0;
  base = current();
  baseKey = key;
  refresh(false);
}

/** Put a snapshot back, keeping the addresses bulbs are known at now. */
function restore(s: Snap): void {
  edit((c) => {
    c.room = structuredClone(s.room);
    c.bindings = s.bindings.map((b) => {
      const now = c.bindings.find((x) => x.device_id === b.device_id);
      return now ? { ...structuredClone(b), ip: now.ip, port: now.port, kind: $state.snapshot(now.kind) } : structuredClone(b);
    });
  });
  base = current();
  baseKey = planKey();
  refresh(false);
}

export function undo(): void {
  commit();
  const s = past.pop();
  if (!s || !base) return;
  future.push(base);
  restore(s);
}

export function redo(): void {
  commit();
  const s = future.pop();
  if (!s || !base) return;
  past.push(base);
  restore(s);
}
