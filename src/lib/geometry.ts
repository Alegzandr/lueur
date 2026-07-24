// Front-end mirror of the geometry in crates/lueur-core/src/geometry.rs.
// Used by the room editor to draw the screen and to show, per light, which part
// of the screen it will mirror. Keep this in sync with the Rust version.

import type { Light, Screen, Vec2 } from "./types";

export function facing(screen: Screen): Vec2 {
  const rad = (screen.facing_deg * Math.PI) / 180;
  return { x: Math.sin(rad), y: Math.cos(rad) };
}

/** Unit vector along the screen width, viewer's left → right. */
export function rightAxis(screen: Screen): Vec2 {
  const f = facing(screen);
  return { x: f.y, y: -f.x };
}

export function endpoints(screen: Screen): { left: Vec2; right: Vec2 } {
  const r = rightAxis(screen);
  const half = screen.width * 0.5;
  return {
    left: { x: screen.center.x - r.x * half, y: screen.center.y - r.y * half },
    right: { x: screen.center.x + r.x * half, y: screen.center.y + r.y * half },
  };
}

export type Edge = "left" | "right" | "center";

/** Classify which screen region a light mirrors (mirrors zone_for_light). */
export function classify(screen: Screen, light: Light): { edge: Edge; u: number } {
  const r = rightAxis(screen);
  const half = Math.max(screen.width * 0.5, 1e-6);
  const rel = { x: light.pos.x - screen.center.x, y: light.pos.y - screen.center.y };
  const u = (rel.x * r.x + rel.y * r.y) / half;
  if (u <= -1) return { edge: "left", u };
  if (u >= 1) return { edge: "right", u };
  return { edge: "center", u };
}

export const EDGE_COLOR: Record<Edge, string> = {
  left: "#ff5c9d",
  right: "#38e8ff",
  center: "#9b6cff",
};

export const EDGE_LABEL: Record<Edge, string> = {
  left: "left edge",
  right: "right edge",
  center: "centre slice",
};
