<script lang="ts">
  // Behind the Lighting view, what the bulbs show: a pool of light per light,
  // where it stands in the room (left of the screen on the left), in the
  // colour it was last sent. A light an effect drives shows the frames the
  // effect streams to its bulb, read from the backend as they go out; a light
  // set by hand shows its state.
  import { app, bindingFor, isDriven, isOn, lightState } from "../state.svelte";
  import { lightGlows } from "../ipc";
  import { mapLight } from "../geometry";
  import { kelvinRgb, tintRgb, type RgbTuple } from "../scenes";
  import type { Glow } from "../types";

  const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

  /** Each light with a bulb: where its pool sits, 0..1 across and down. */
  const spots = $derived.by(() => {
    const screen = app.config.room.screen;
    return app.config.room.lights
      .filter((l) => bindingFor(l.id))
      .map((l) => {
        const m = mapLight(screen, l);
        return { id: l.id, x: clamp(0.5 + m.u * 0.32, 0.06, 0.94), y: clamp(0.5 + m.v * 0.22, 0.18, 0.85) };
      });
  });

  /** The frames the effects last sent, by light. */
  let glows: Record<string, Glow[]> = {};

  // Read the frames as fast as the screen draws them, while an effect plays
  // and the window shows: one call at a time, never a queue.
  $effect(() => {
    if (!app.status.running) {
      glows = {};
      return;
    }
    let alive = true;
    let timer = 0;
    const read = async () => {
      const asked = performance.now();
      if (!document.hidden) {
        try {
          glows = await lightGlows();
        } catch {
          glows = {};
        }
      }
      if (alive) timer = window.setTimeout(read, Math.max(0, 16 - (performance.now() - asked)));
    };
    void read();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  });

  /** What a light shows: a colour and a 0..1 level for each of its zones. */
  function target(id: string): [RgbTuple, number][] {
    if (isDriven(id)) {
      return (glows[id] ?? []).map((g) => [g.rgb ?? kelvinRgb(g.kelvin ?? 2700), g.bright / 100]);
    }
    if (!isOn(id)) return [];
    const s = lightState(id);
    return [[tintRgb(s.tint), s.bright / 100]];
  }

  // --- Drawing ---

  let canvas: HTMLCanvasElement;

  $effect(() => {
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const still = matchMedia("(prefers-reduced-motion: reduce)");
    // What each zone's pool shows on screen, eased toward what its light shows.
    const shown = new Map<string, { rgb: RgbTuple; level: number; x: number; y: number }>();
    let last = performance.now();
    let raf = 0;

    const resize = () => {
      // A third of the size is plenty for soft light, and cheap.
      canvas.width = Math.max(1, Math.round(canvas.clientWidth / 3));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight / 3));
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const draw = (now: number) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const { width: w, height: h } = canvas;
      ctx.globalCompositeOperation = "source-over";
      ctx.clearRect(0, 0, w, h);
      ctx.globalCompositeOperation = "lighter";

      const seen = new Set<string>();
      for (const spot of spots) {
        const driven = isDriven(spot.id);
        const zones = target(spot.id);
        zones.forEach(([rgb, level], j) => {
          const key = `${spot.id}/${j}`;
          seen.add(key);
          // A light with several zones spreads them side by side.
          const x = spot.x + (j - (zones.length - 1) / 2) * 0.05;
          const s = shown.get(key) ?? { rgb, level: 0, x, y: spot.y };
          // A frame lands as fast as on the bulb: a beat at once, then the
          // glide the bulb itself makes. Set by hand, it fades gently. Reduced
          // motion keeps the colours but never flashes them.
          const tau = still.matches ? 1.5 : driven ? (level > s.level ? 0.01 : 0.05) : 0.35;
          const k = 1 - Math.exp(-dt / tau);
          s.rgb = s.rgb.map((c, i) => c + (rgb[i] - c) * k) as RgbTuple;
          s.level += (level - s.level) * k;
          s.x = x;
          s.y = spot.y;
          shown.set(key, s);
        });
      }
      for (const [key, s] of shown) {
        // Lights turned off or stopped fade out rather than vanish.
        if (!seen.has(key)) {
          s.level *= Math.exp(-dt / 0.35);
          if (s.level < 0.01) {
            shown.delete(key);
            continue;
          }
        }
        pool(ctx, s.x * w, s.y * h, Math.max(w, h) * (0.32 + 0.12 * s.level), s.rgb, s.level);
      }
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
  });

  function pool(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, [cr, cg, cb]: RgbTuple, level: number) {
    // The eye reads a dim bulb as brighter than its share of full power.
    const a = Math.pow(level, 0.6);
    const c = `${Math.round(cr)} ${Math.round(cg)} ${Math.round(cb)}`;
    const g = ctx.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, `rgb(${c} / ${0.75 * a})`);
    g.addColorStop(0.45, `rgb(${c} / ${0.3 * a})`);
    g.addColorStop(1, `rgb(${c} / 0)`);
    ctx.fillStyle = g;
    ctx.fillRect(x - r, y - r, r * 2, r * 2);
  }
</script>

<canvas class="backdrop" bind:this={canvas} aria-hidden="true"></canvas>

<style>
  .backdrop {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    opacity: var(--backdrop);
    filter: blur(24px) saturate(1.15);
    animation: arrive 600ms var(--ease);
  }
  @keyframes arrive {
    from {
      opacity: 0;
    }
  }
</style>
