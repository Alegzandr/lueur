import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { history, observe, planKey, redo, track, undo } from "./history.svelte";
import { app, edit } from "./state.svelte";
import { defaultConfig, type Light } from "./types";

// The undo history works on the live app state, outside Tauri: `edit` only
// schedules a save, which does nothing before a config has been loaded.

function light(id: string, x: number): Light {
  return { id, name: id, pos: { x, y: 300 }, height: 0.5, zone_override: null, enabled: true };
}

/** Change the plan as a view does: edit, then report the new key. */
function change(mutator: Parameters<typeof edit>[0], held = false): void {
  edit(mutator);
  observe(planKey(), held);
}

const xs = () => app.config.room.lights.map((l) => l.pos.x);

beforeEach(() => {
  vi.useFakeTimers();
  app.config = defaultConfig();
  app.config.room.lights = [light("a", 100)];
  track();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("history", () => {
  it("starts with nothing to undo or redo", () => {
    expect(history).toEqual({ canUndo: false, canRedo: false });
  });

  it("undoes and redoes a change", () => {
    change((c) => (c.room.lights[0].pos.x = 200));
    expect(history.canUndo).toBe(true);
    undo();
    expect(xs()).toEqual([100]);
    expect(history).toEqual({ canUndo: false, canRedo: true });
    redo();
    expect(xs()).toEqual([200]);
    expect(history).toEqual({ canUndo: true, canRedo: false });
  });

  it("settles a burst of changes into one step", () => {
    for (const x of [110, 120, 130]) change((c) => (c.room.lights[0].pos.x = x));
    vi.advanceTimersByTime(1000);
    change((c) => (c.room.lights[0].pos.x = 500));
    vi.advanceTimersByTime(1000);
    undo();
    expect(xs()).toEqual([130]);
    undo();
    expect(xs()).toEqual([100]);
    expect(history.canUndo).toBe(false);
  });

  it("keeps a held drag open until it is let go", () => {
    change((c) => (c.room.lights[0].pos.x = 150), true);
    vi.advanceTimersByTime(5000);
    change((c) => (c.room.lights[0].pos.x = 180), true);
    change(() => {}, false);
    vi.advanceTimersByTime(1000);
    undo();
    expect(xs()).toEqual([100]);
  });

  it("forgets the redo once the plan moves on", () => {
    change((c) => (c.room.lights[0].pos.x = 200));
    undo();
    change((c) => c.room.lights.push(light("b", 400)));
    expect(history.canRedo).toBe(false);
    redo();
    expect(app.config.room.lights.map((l) => l.id)).toEqual(["a", "b"]);
  });

  it("keeps the address a bulb is known at now", () => {
    const binding = (ip: string) => ({
      light_id: "a",
      device_id: "0x1",
      ip,
      port: 55443,
      model: "color",
      name: "Desk",
      kind: { family: "yeelight" as const },
    });
    app.config.bindings = [binding("192.168.1.20")];
    track();
    change((c) => (c.room.lights[0].pos.x = 300));
    // A background search finds the bulb elsewhere: not a step of its own.
    app.config.bindings[0].ip = "192.168.1.77";
    undo();
    expect(xs()).toEqual([100]);
    expect(app.config.bindings[0].ip).toBe("192.168.1.77");
  });
});
