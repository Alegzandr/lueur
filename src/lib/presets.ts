// Named "feels" for the effect. Each one only sets the handful of values that
// decide how the light moves; the fine controls stay in Settings.

import type { AppConfig } from "./types";

export type Feel = "instant" | "balanced" | "calm";

interface FeelValues {
  smoothing: number;
  effect: "smooth" | "sudden";
  duration_ms: number;
}

export const FEELS: Record<Feel, FeelValues> = {
  instant: { smoothing: 0.9, effect: "smooth", duration_ms: 40 },
  balanced: { smoothing: 0.55, effect: "smooth", duration_ms: 80 },
  calm: { smoothing: 0.22, effect: "smooth", duration_ms: 220 },
};

export const FEEL_OPTIONS: { value: Feel; label: string; title: string }[] = [
  { value: "instant", label: "Instant", title: "Follows every cut. Best for fast games." },
  { value: "balanced", label: "Balanced", title: "Quick without flicker. The default." },
  { value: "calm", label: "Calm", title: "Slow, soft changes. Best for films." },
];

/** The feel the current settings match, or null if they were hand-tuned. */
export function currentFeel(c: AppConfig): Feel | null {
  for (const [name, v] of Object.entries(FEELS) as [Feel, FeelValues][]) {
    if (
      Math.abs(c.engine.smoothing - v.smoothing) < 0.005 &&
      c.transition.effect === v.effect &&
      c.transition.duration_ms === v.duration_ms
    ) {
      return name;
    }
  }
  return null;
}

export function applyFeel(c: AppConfig, feel: Feel): void {
  const v = FEELS[feel];
  c.engine.smoothing = v.smoothing;
  c.transition.effect = v.effect;
  c.transition.duration_ms = v.duration_ms;
}
