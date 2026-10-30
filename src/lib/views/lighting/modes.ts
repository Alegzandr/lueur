// What the Lighting view's pieces say about modes and moods: their names,
// their swatches, and which mood a light shows.

import { app, isDriven, lightState, modeOf } from "../../state.svelte";
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

// --- What a light does ---

/** What a light does now: plays a mode, shows a mood, shows a white or a
 *  colour set by hand, or is off. */
export type Doing =
  | { kind: "mode"; mode: Mode }
  | { kind: "mood"; mood: string }
  | { kind: "hand"; state: LightState }
  | { kind: "off" };

export function doingOf(id: string): Doing {
  const mode = modeOf(id);
  if (mode) return { kind: "mode", mode };
  const s = lightState(id);
  if (!s.on) return { kind: "off" };
  const mood = moodOf(id);
  return mood ? { kind: "mood", mood } : { kind: "hand", state: s };
}

/** What a light does, in a few words. */
export function doingName(d: Doing): string {
  switch (d.kind) {
    case "mode":
      return modeName(d.mode);
    case "mood":
      return moodName(d.mood);
    case "off":
      return t("light_off");
    case "hand":
      return d.state.tint.mode === "white" ? `${t("white")} ${d.state.tint.kelvin} K` : t("tint_colour");
  }
}

/** The same thing done, so lights doing it read as one. */
export const sameDoing = (a: Doing, b: Doing) =>
  a.kind === b.kind &&
  (a.kind === "mode"
    ? a.mode === (b as typeof a).mode
    : a.kind === "mood"
      ? a.mood === (b as typeof a).mood
      : a.kind === "hand"
        ? JSON.stringify(a.state.tint) === JSON.stringify((b as typeof a).state.tint)
        : true);

/** What a group of lights does, in a few words: the one thing they all do,
 *  or that they do several. `live` while that is an effect giving light. */
export function summaryOf(ids: string[]): { text: string; live: boolean } | null {
  if (ids.length === 0) return null;
  const first = doingOf(ids[0]);
  if (!ids.every((id) => sameDoing(doingOf(id), first))) return { text: t("mode_several"), live: false };
  return { text: doingName(first), live: first.kind === "mode" };
}
