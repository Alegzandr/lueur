import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { app, dismiss, fail, notify } from "./state.svelte";

// Toasts live on the app state; their timers are faked so expiry can be
// checked without waiting.

beforeEach(() => {
  vi.useFakeTimers();
  for (const t of [...app.toasts]) dismiss(t.id);
});

afterEach(() => {
  vi.useRealTimers();
});

const msgs = () => app.toasts.map((t) => t.msg);

describe("toasts", () => {
  it("lets information go by itself, and keeps errors until closed", () => {
    notify("info", "saved");
    fail("broken");
    vi.advanceTimersByTime(60_000);
    expect(msgs()).toEqual(["broken"]);
  });

  it("counts the same message on the toast showing, keeping its id", () => {
    fail("broken");
    const { id, said } = app.toasts[0];
    fail("broken");
    expect(app.toasts).toHaveLength(1);
    expect(app.toasts[0].id).toBe(id);
    expect(app.toasts[0].count).toBe(2);
    // Marked to be read out again.
    expect(app.toasts[0].said).toBeGreaterThan(said);
  });

  it("starts an information toast's time over when it comes again", () => {
    notify("info", "found");
    vi.advanceTimersByTime(3000);
    notify("info", "found");
    vi.advanceTimersByTime(3000);
    expect(msgs()).toEqual(["found"]);
    vi.advanceTimersByTime(1000);
    expect(msgs()).toEqual([]);
  });

  it("never merges toasts with an action: each undo is its own", () => {
    const a = vi.fn();
    const b = vi.fn();
    notify("info", "Light removed", { label: "Undo", run: a });
    notify("info", "Light removed", { label: "Undo", run: b });
    expect(app.toasts).toHaveLength(2);
    app.toasts[0].action?.run();
    expect(a).toHaveBeenCalled();
    expect(b).not.toHaveBeenCalled();
  });

  it("keeps a few at most, and information never pushes out an error", () => {
    for (const m of ["e1", "e2", "e3", "e4"]) fail(m);
    notify("info", "hello");
    expect(msgs()).toEqual(["e1", "e2", "e3", "e4", "hello"]);
    // A further error takes the oldest information's place first.
    fail("e5");
    expect(msgs()).toEqual(["e2", "e3", "e4", "e5"]);
  });
});
