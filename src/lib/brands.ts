// What a bulb is called where its light is shown: never its address, which
// only appears in details and to tell two alike apart.

import type { DeviceBinding, Family } from "./types";

/** Brand names, the same in every language. */
export const FAMILY: Record<Family, string> = {
  yeelight: "Yeelight",
  wiz: "WiZ",
  lifx: "LIFX",
  govee: "Govee",
  hue: "Philips Hue",
  nanoleaf: "Nanoleaf",
  tuya: "Smart Life",
};

/** A bound bulb as its light describes it: its own name when it says more
 *  than the light's, then its brand and model. */
export function bulbLabel(b: DeviceBinding, lightName?: string): string {
  const brand = [FAMILY[b.kind?.family ?? "yeelight"], b.model].filter(Boolean).join(" ");
  return b.name && b.name !== lightName ? `${b.name} · ${brand}` : brand;
}
