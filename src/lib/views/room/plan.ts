// What the Room view's pieces share: the plan's extent, what can be
// selected on it, and the names of its corners.

import { app } from "../../state.svelte";
import { t } from "../../i18n";
import type { Edge } from "../../geometry";
import type { Vec2 } from "../../types";

// Extent of the plan in plan units. Everything placed is kept inside it.
export const VIEW_W = 1000;
export const VIEW_H = 750;
export const MARGIN = 12;
/** How far in front of the screen the seat handle sits, in plan units. */
export const SEAT = 120;
export const SCREEN_MIN = 120;
export const SCREEN_MAX = 520;

// Typed positions are kept in the plan's bounds.
export const X_MAX = VIEW_W - MARGIN;
export const Y_MAX = VIEW_H - MARGIN;

export const EDGES: Edge[] = ["left", "center", "right"];

// Selection: a light lives in shared state (other views link to it); the
// screen and corners are only selectable here.
export type Other = { kind: "screen" } | { kind: "corner"; index: number } | null;

export const sub = (a: Vec2, b: Vec2): Vec2 => ({ x: a.x - b.x, y: a.y - b.y });
export const clampP = (p: Vec2): Vec2 => ({
  x: Math.round(Math.max(MARGIN, Math.min(VIEW_W - MARGIN, p.x))),
  y: Math.round(Math.max(MARGIN, Math.min(VIEW_H - MARGIN, p.y))),
});

// --- Naming corners ---

const WHERE = [
  ["where_top_left", "where_top", "where_top_right"],
  ["where_left", "where_middle", "where_right"],
  ["where_bottom_left", "where_bottom", "where_bottom_right"],
] as const;

/** "Corner 2, top right": where it sits in the room as drawn. */
export function cornerName(i: number): string {
  const w = app.config.room.walls;
  const p = w[i];
  if (!p) return t("corner_n", { n: i + 1 });
  const third = (v: number, all: number[]) => {
    const lo = Math.min(...all);
    const f = (v - lo) / Math.max(Math.max(...all) - lo, 1);
    return f < 1 / 3 ? 0 : f > 2 / 3 ? 2 : 1;
  };
  const row = third(p.y, w.map((q) => q.y));
  const col = third(p.x, w.map((q) => q.x));
  return t("corner_at", { n: i + 1, where: t(WHERE[row][col]) });
}
