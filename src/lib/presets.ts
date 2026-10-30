// Named "feels" for the effect. Each one only sets the handful of values that
// decide how the light moves; the fine controls stay in Settings.
//
// A feel is its smoothing: how fast the light follows the picture. The bulb's
// own fade is fitted to the time between two updates whatever is asked (see
// `pace` in src-tauri/src/runner/ambilight.rs), so a feel does not set one;
// it only keeps the fade on.

import type { AppConfig } from "./types";

export type Feel = "instant" | "balanced" | "calm";

export const FEELS: Record<Feel, number> = {
  instant: 0.9,
  balanced: 0.55,
  calm: 0.22,
};

export const FEEL_ORDER: Feel[] = ["instant", "balanced", "calm"];

/** The feel the current settings match, or null if they were hand-tuned. */
export function currentFeel(c: AppConfig): Feel | null {
  if (c.transition.effect !== "smooth") return null;
  return FEEL_ORDER.find((name) => Math.abs(c.engine.smoothing - FEELS[name]) < 0.005) ?? null;
}

export function applyFeel(c: AppConfig, feel: Feel): void {
  c.engine.smoothing = FEELS[feel];
  c.transition.effect = "smooth";
}
