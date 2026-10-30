// The room in every light: one photograph per mood and show, `screen` for
// the mode that follows the screen, `off` for the lights out and `custom`
// for a white or colour set by hand, tinted on the page. Generated once,
// locally, from the same room, so they read as one series (see DESIGN.md).

import { tintCss } from "../../scenes";
import type { SceneLight } from "../../types";
import type { Doing } from "./modes";

const files = import.meta.glob<string>("../../assets/modes/*.webp", { eager: true, import: "default" });
const PICTURES: Record<string, string> = Object.fromEntries(
  Object.entries(files).map(([path, url]) => [path.slice(path.lastIndexOf("/") + 1, -".webp".length), url]),
);

/** A mode or mood's photograph. */
export const pictureOf = (id: string): string => PICTURES[id === "ambilight" ? "screen" : id] ?? PICTURES.off;

/** How a light looks in the room: its photograph and, when set by hand,
 *  the colour laid over it. `key` is the same for lights that look alike. */
export interface Look {
  key: string;
  src: string;
  tint: string | null;
}

export function lookOfDoing(d: Doing): Look {
  switch (d.kind) {
    case "mode":
      return { key: d.mode, src: pictureOf(d.mode), tint: null };
    case "mood":
      return { key: d.mood, src: pictureOf(d.mood), tint: null };
    case "off":
      return { key: "off", src: pictureOf("off"), tint: null };
    case "hand": {
      const tint = tintCss(d.state.tint);
      return { key: `custom ${tint}`, src: pictureOf("custom"), tint };
    }
  }
}

export function lookOfScene(s: SceneLight): Look {
  if (s.kind === "mode") return lookOfDoing({ kind: "mode", mode: s.mode });
  if (s.kind === "natural") return { key: "natural", src: pictureOf("natural"), tint: null };
  return lookOfDoing(s.state.on ? { kind: "hand", state: s.state } : { kind: "off" });
}

/** The looks of a group of lights, each once, in the order first met. */
export function distinctLooks(looks: Look[], max = 4): Look[] {
  const seen = new Map<string, Look>();
  for (const l of looks) if (!seen.has(l.key)) seen.set(l.key, l);
  return [...seen.values()].slice(0, max);
}
