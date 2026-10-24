// Lights set by hand: the built-in moods, the colour palette, and how a
// light's state is drawn on screen.

import type { LightState, Rgb, ShowKind, Tint } from "./types";

export type MoodId = "daylight" | "reading" | "relax" | "night";

/** Mirrors `MOODS` in lueur-core scene.rs; the backend applies them by id. */
export const MOODS: { id: MoodId; kelvin: number; bright: number }[] = [
  { id: "daylight", kelvin: 5500, bright: 100 },
  { id: "reading", kelvin: 4000, bright: 100 },
  { id: "relax", kelvin: 2700, bright: 60 },
  { id: "night", kelvin: 2200, bright: 8 },
];

/** A warm, comfortable white: what a light shows before anyone sets it. */
export const DEFAULT_STATE: LightState = { on: true, bright: 80, tint: { mode: "white", kelvin: 2700 } };

export const MIN_KELVIN = 2200;
export const MAX_KELVIN = 6500;

/** Saturated enough to read as a choice, spread evenly round the wheel. */
export const PALETTE: [number, number, number][] = [
  [255, 59, 48],
  [255, 122, 0],
  [255, 184, 0],
  [52, 199, 89],
  [0, 199, 190],
  [10, 132, 255],
  [94, 92, 230],
  [191, 90, 242],
  [255, 45, 146],
];

/** The colour a white of `kelvin` looks like (Tanner Helland's fit). */
export function kelvinRgb(kelvin: number): [number, number, number] {
  const t = kelvin / 100;
  const clamp = (v: number) => Math.round(Math.max(0, Math.min(255, v)));
  const r = t <= 66 ? 255 : 329.698727446 * Math.pow(t - 60, -0.1332047592);
  const g = t <= 66 ? 99.4708025861 * Math.log(t) - 161.1195681661 : 288.1221695283 * Math.pow(t - 60, -0.0755148492);
  const b = t >= 66 ? 255 : t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307;
  return [clamp(r), clamp(g), clamp(b)];
}

export function tintRgb(tint: Tint): [number, number, number] {
  return tint.mode === "white" ? kelvinRgb(tint.kelvin) : [tint.r, tint.g, tint.b];
}

export function rgbCss([r, g, b]: [number, number, number]): string {
  return `rgb(${r} ${g} ${b})`;
}

export function tintCss(tint: Tint): string {
  return rgbCss(tintRgb(tint));
}

export function sameRgb(tint: Tint, [r, g, b]: [number, number, number]): boolean {
  return tint.mode === "color" && tint.r === r && tint.g === g && tint.b === b;
}

export function hexOf(tint: Tint): string {
  return "#" + tintRgb(tint).map((v) => v.toString(16).padStart(2, "0")).join("");
}

export function rgbOfHex(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  return m ? [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)] : null;
}

/** The shows, in the order they are offered, with a preview of each. */
export const SHOWS: { id: ShowKind; preview: string }[] = [
  { id: "music", preview: "linear-gradient(135deg, #ff0050, #7a00ff 55%, #0078ff)" },
  {
    id: "rainbow",
    preview: "linear-gradient(135deg, #ff3b30, #ffa000, #c8ff00, #00ff78, #008cff, #9600ff)",
  },
  { id: "sunset", preview: "linear-gradient(135deg, #ff461e, #ff8c00 40%, #ff2878 70%, #a028dc)" },
  { id: "ocean", preview: "linear-gradient(135deg, #005aff, #00c8d2 55%, #00e696)" },
  { id: "candle", preview: "radial-gradient(circle at 50% 70%, #ffd27a, #ff8a1e 45%, #7a2a00)" },
];

/** Common wall paints. Lueur compensates for the one picked. */
export type WallId = "white" | "cream" | "beige" | "blue" | "green" | "pink";

export const WALLS: { id: WallId; rgb: Rgb }[] = [
  { id: "white", rgb: { r: 255, g: 255, b: 255 } },
  { id: "cream", rgb: { r: 250, g: 240, b: 215 } },
  { id: "beige", rgb: { r: 235, g: 215, b: 180 } },
  { id: "blue", rgb: { r: 190, g: 210, b: 235 } },
  { id: "green", rgb: { r: 195, g: 220, b: 195 } },
  { id: "pink", rgb: { r: 240, g: 205, b: 205 } },
];
