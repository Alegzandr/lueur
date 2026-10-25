import { describe, expect, it } from "vitest";
import { closestOnSegment, describe as describeMapping, endpoints, facing, mapLight, rightAxis } from "./geometry";
import type { Light, Screen } from "./types";

// A 200-wide screen at (500, 100) facing down the plan (towards +y), as the
// default room has it.
const screen: Screen = { center: { x: 500, y: 100 }, width: 200, facing_deg: 0 };

function light(x: number, y: number, height = 0.5): Light {
  return { id: "l", name: "L", pos: { x, y }, height, zone_override: null, enabled: true };
}

const near = (a: number, b: number) => expect(a).toBeCloseTo(b, 9);

describe("screen axes", () => {
  it("faces +y at 0° and turns clockwise", () => {
    const f = facing(screen);
    near(f.x, 0);
    near(f.y, 1);
    const turned = facing({ ...screen, facing_deg: 90 });
    near(turned.x, 1);
    near(turned.y, 0);
  });

  it("puts the viewer's right on the plan's left when facing them", () => {
    const r = rightAxis(screen);
    near(r.x, 1);
    near(r.y, 0);
  });

  it("ends half a width either side of the centre", () => {
    const { left, right } = endpoints(screen);
    near(left.x, 400);
    near(right.x, 600);
    near(left.y, 100);
    near(right.y, 100);
  });
});

describe("closestOnSegment", () => {
  const a = { x: 0, y: 0 };
  const b = { x: 10, y: 0 };

  it("projects onto the segment", () => {
    expect(closestOnSegment({ x: 4, y: 7 }, a, b)).toEqual({ x: 4, y: 0 });
  });

  it("stops at the ends", () => {
    expect(closestOnSegment({ x: -5, y: 3 }, a, b)).toEqual(a);
    expect(closestOnSegment({ x: 50, y: -3 }, a, b)).toEqual(b);
  });

  it("survives a segment of zero length", () => {
    expect(closestOnSegment({ x: 3, y: 3 }, a, a)).toEqual(a);
  });
});

describe("mapLight", () => {
  it("gives a light right behind the screen a tight centre slice at full strength", () => {
    const m = mapLight(screen, light(500, 100));
    expect(m.edge).toBe("center");
    expect(m.spread).toBe(0);
    near(m.gain, 1);
    near(m.responsiveness, 1);
    near(m.zone.w, 0.16);
    near(m.zone.x, 0.42);
    expect(describeMapping(m)).toBe("map_slice");
  });

  it("maps lights beyond the screen's ends to its edges", () => {
    expect(mapLight(screen, light(300, 100)).edge).toBe("left");
    expect(mapLight(screen, light(700, 100)).edge).toBe("right");
    expect(describeMapping(mapLight(screen, light(300, 100)))).toBe("map_left");
  });

  it("widens and dims with distance, never past the whole picture", () => {
    const close = mapLight(screen, light(500, 150));
    const far = mapLight(screen, light(500, 2000));
    expect(far.spread).toBeGreaterThan(close.spread);
    expect(far.gain).toBeLessThan(close.gain);
    expect(far.gain).toBeGreaterThanOrEqual(0.55);
    expect(far.responsiveness).toBeGreaterThanOrEqual(0.35);
    expect(far.spread).toBe(1);
    expect(describeMapping(far)).toBe("map_whole");
  });

  it("always samples inside the frame", () => {
    for (const [x, y, h] of [
      [0, 0, 0],
      [1000, 800, 1],
      [-400, 100, 2],
      [500, -300, -1],
    ]) {
      const { zone } = mapLight(screen, light(x, y, h));
      expect(zone.x).toBeGreaterThanOrEqual(0);
      expect(zone.y).toBeGreaterThanOrEqual(0);
      expect(zone.x + zone.w).toBeLessThanOrEqual(1 + 1e-9);
      expect(zone.y + zone.h).toBeLessThanOrEqual(1 + 1e-9);
    }
  });

  it("picks the band of the picture from the light's height", () => {
    const high = mapLight(screen, light(500, 100, 1));
    const low = mapLight(screen, light(500, 100, 0));
    expect(high.zone.y).toBe(0);
    near(low.zone.y + low.zone.h, 1);
  });

  it("keeps a hand-drawn zone as it is", () => {
    const zone = { x: 0.1, y: 0.2, w: 0.3, h: 0.4 };
    const m = mapLight(screen, { ...light(900, 900), zone_override: zone });
    expect(m.zone).toEqual(zone);
    expect(m.gain).toBe(1);
  });
});
