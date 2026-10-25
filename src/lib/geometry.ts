// Front-end mirror of the geometry in crates/lueur-core/src/geometry.rs.
// Used by the room editor to draw the screen and to show, per light, which part
// of the screen it will mirror and how strongly. Keep this in sync with the Rust
// version: the constants below must match the ones there exactly, or the
// preview will lie about what the engine does.

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

/** The point of the segment from `a` to `b` closest to `p`. */
export function closestOnSegment(p: Vec2, a: Vec2, b: Vec2): Vec2 {
  const ab = { x: b.x - a.x, y: b.y - a.y };
  const len2 = ab.x * ab.x + ab.y * ab.y || 1;
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2));
  return { x: a.x + ab.x * t, y: a.y + ab.y * t };
}

export type Edge = "left" | "right" | "center";

export interface Mapping {
  zone: { x: number; y: number; w: number; h: number };
  edge: Edge;
  /** 0 = tight slice, 1 = the whole picture. */
  spread: number;
  /** Distance brightness multiplier. */
  gain: number;
  /** Distance smoothing multiplier. */
  responsiveness: number;
  /** Lateral position, -1 = left end of the screen, +1 = right end. */
  u: number;
  /** Depth, positive = on the viewer's side of the screen plane. */
  v: number;
}

// --- Must match crates/lueur-core/src/geometry.rs ---
const MIN_HALF_W = 0.08;
const MIN_HALF_H = 0.28;
const MAX_HALF = 0.5;
const FRONT_WIDEN = 0.21;
const LATERAL_WIDEN = 0.14;
const FALLOFF = 2.0;
const MIN_GAIN = 0.55;
const MIN_RESPONSIVENESS = 0.35;

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

/** Full mapping for a light (mirrors `map_light`). */
export function mapLight(screen: Screen, light: Light): Mapping {
  const r = rightAxis(screen);
  const f = facing(screen);
  const half = Math.max(screen.width * 0.5, 1e-6);
  const rel = { x: light.pos.x - screen.center.x, y: light.pos.y - screen.center.y };
  const u = (rel.x * r.x + rel.y * r.y) / half;
  const v = (rel.x * f.x + rel.y * f.y) / half;

  if (light.zone_override) {
    return {
      zone: light.zone_override,
      edge: "center",
      spread: 0,
      gain: 1,
      responsiveness: 1,
      u,
      v,
    };
  }

  const edge: Edge = u <= -1 ? "left" : u >= 1 ? "right" : "center";

  const lateralOut = Math.max(Math.abs(u) - 1, 0);
  const front = Math.max(v, 0);
  const widen = FRONT_WIDEN * front + LATERAL_WIDEN * lateralOut;

  const hw = Math.min(MIN_HALF_W + widen, MAX_HALF);
  const hh = Math.min(MIN_HALF_H + widen, MAX_HALF);
  const spread = clamp((hw - MIN_HALF_W) / (MAX_HALF - MIN_HALF_W), 0, 1);

  const xCenter = clamp((u + 1) * 0.5, 0, 1);
  const yCenter = 1 - clamp(light.height, 0, 1);
  const x = clamp(xCenter - hw, 0, Math.max(1 - 2 * hw, 0));
  const y = clamp(yCenter - hh, 0, Math.max(1 - 2 * hh, 0));

  const dist = Math.hypot(lateralOut, v);
  const t = dist / FALLOFF;
  const atten = 1 / (1 + t * t);

  return {
    zone: { x, y, w: Math.min(2 * hw, 1 - x), h: Math.min(2 * hh, 1 - y) },
    edge,
    spread,
    gain: MIN_GAIN + (1 - MIN_GAIN) * atten,
    responsiveness: MIN_RESPONSIVENESS + (1 - MIN_RESPONSIVENESS) * atten,
    u,
    v,
  };
}

// Design tokens, so the plan's only saturated colour stays in one place and
// matches the legend. Equal lightness across the three, none dominates.
export const EDGE_COLOR: Record<Edge, string> = {
  left: "var(--zone-left)",
  right: "var(--zone-right)",
  center: "var(--zone-center)",
};

export type MappingKind = "map_whole" | "map_broad" | "map_left" | "map_right" | "map_slice";

/** What a mapping does, as a string key for the interface. */
export function describe(m: Mapping): MappingKind {
  if (m.spread > 0.85) return "map_whole";
  if (m.spread > 0.45) return "map_broad";
  if (m.edge === "left") return "map_left";
  if (m.edge === "right") return "map_right";
  return "map_slice";
}

export const EDGE_KIND: Record<Edge, MappingKind> = {
  left: "map_left",
  center: "map_slice",
  right: "map_right",
};
