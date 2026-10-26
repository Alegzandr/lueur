// What the Lighting view's pieces say about modes and moods: their names,
// their swatches, and which mood a light shows.

import { app, isDriven } from "../../state.svelte";
import { t } from "../../i18n";
import { MOODS, NATURAL, SHOWS, kelvinRgb, rgbCss, tintCss, type MoodId } from "../../scenes";
import type { LightState, Mode } from "../../types";

export const modeName = (m: Mode) => t(`mode_${m}`);

// Amber, violet and blue: a picture's colours, without the red that reads
// as an error.
const SCREEN_SWATCH = "linear-gradient(90deg, oklch(0.66 0.16 55), oklch(0.45 0.12 300), oklch(0.6 0.16 250))";
export const modeSwatch = (m: Mode | null, s: LightState) =>
  m === null ? tintCss(s.tint) : m === "ambilight" ? SCREEN_SWATCH : (SHOWS.find((x) => x.id === m)?.preview ?? "");

// --- Moods ---

const same = (a: LightState | undefined, b: LightState) =>
  !!a && a.on === b.on && a.bright === b.bright && JSON.stringify(a.tint) === JSON.stringify(b.tint);

/** The mood a light shows now, if it shows one: the natural light while
 *  it follows it, or the mood its state is. */
export const moodOf = (id: string): string | null => {
  if (isDriven(id)) return null;
  if (app.config.natural?.includes(id)) return NATURAL;
  const s = app.config.states[id];
  return (
    MOODS.find((m) => same(s, { on: true, bright: m.bright, tint: { mode: "white", kelvin: m.kelvin } }))?.id ?? null
  );
};
export const moodName = (id: string) => t(id === NATURAL ? "mood_natural" : `mood_${id as MoodId}`);

/** The natural light's swatch: its day, from night to noon and back. */
export const naturalSwatch = `linear-gradient(135deg, ${[2200, 4000, 5500, 3000].map((k) => rgbCss(kelvinRgb(k))).join(", ")})`;
