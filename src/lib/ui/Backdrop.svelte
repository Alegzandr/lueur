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

  /** How many zones the backdrop draws at most; more than any room holds. */
  const MAX_POOLS = 32;

  const VERTEX = `
    attribute vec2 corner;
    void main() { gl_Position = vec4(corner, 0.0, 1.0); }
  `;

  // Each pool falls off as a bell, so no ring marks where a slope changes,
  // and the pools add up as light does. Then each pixel is dithered: a faint
  // glow spans few of the 256 levels a screen shows, and rounded to them it
  // falls into bands. A pixel's own noise, under a level either way, scatters
  // the steps so the eye reads the slope between them. The glow is laid on
  // the page's own background here, and the canvas drawn opaque: left
  // translucent, the page would blend it in after the dither, and on a light
  // background that blend takes nearly all the noise away.
  const FRAGMENT = `
    precision highp float;
    uniform vec4 pools[${MAX_POOLS}];
    uniform vec3 colours[${MAX_POOLS}];
    uniform int count;
    uniform float strength;
    uniform float saturation;
    uniform vec3 background;
    const float EDGE = 0.011109; // exp(-4.5), where the bell meets 0

    float hash(vec2 p) {
      vec3 q = fract(p.xyx * 0.1031);
      q += dot(q, q.yzx + 33.33);
      return fract((q.x + q.y) * q.z);
    }

    void main() {
      vec2 xy = gl_FragCoord.xy;
      vec4 sum = vec4(0.0);
      for (int i = 0; i < ${MAX_POOLS}; i++) {
        if (i >= count) break;
        float t = distance(xy, pools[i].xy) / pools[i].z;
        if (t >= 1.0) continue;
        float a = pools[i].w * (exp(-4.5 * t * t) - EDGE) / (1.0 - EDGE);
        sum += vec4(colours[i] * a, a);
      }
      sum = min(sum, 1.0);
      // More saturated, as coloured light on a wall looks.
      float grey = dot(sum.rgb, vec3(0.2126, 0.7152, 0.0722));
      sum.rgb = clamp(mix(vec3(grey), sum.rgb, saturation), 0.0, sum.a);
      sum *= strength;
      vec3 lit = sum.rgb + background * (1.0 - sum.a);
      float noise = (hash(xy) + hash(xy + 17.17) - 1.0) / 255.0;
      gl_FragColor = vec4(clamp(lit + noise, 0.0, 1.0), 1.0);
    }
  `;

  let canvas: HTMLCanvasElement;

  /** Draws again until the pools settle; set while the canvas lives. */
  let wake = () => {};

  // Frames are drawn only while something moves: an effect playing, a light
  // changing, the glow coming in. Once still, the backdrop stays as drawn and
  // costs nothing until the lights or the theme change.
  $effect(() => {
    void app.status.running;
    for (const spot of spots) target(spot.id);
    wake();
  });

  $effect(() => {
    const gl = canvas.getContext("webgl", { alpha: false, antialias: false, depth: false });
    if (!gl) return;
    const program = link(gl);
    if (!program) return;
    gl.useProgram(program);
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const corner = gl.getAttribLocation(program, "corner");
    gl.enableVertexAttribArray(corner);
    gl.vertexAttribPointer(corner, 2, gl.FLOAT, false, 0, 0);
    const where = {
      pools: gl.getUniformLocation(program, "pools"),
      colours: gl.getUniformLocation(program, "colours"),
      count: gl.getUniformLocation(program, "count"),
      strength: gl.getUniformLocation(program, "strength"),
      saturation: gl.getUniformLocation(program, "saturation"),
      background: gl.getUniformLocation(program, "background"),
    };
    const pools = new Float32Array(MAX_POOLS * 4);
    const colours = new Float32Array(MAX_POOLS * 3);

    const still = matchMedia("(prefers-reduced-motion: reduce)");
    // What each zone's pool shows on screen, eased toward what its light shows.
    const shown = new Map<string, { rgb: RgbTuple; level: number; x: number; y: number }>();
    const born = performance.now();
    let last = born;
    let raf = 0;

    // The theme sets how strongly the glow shows, and how pure; read when it
    // changes, not every frame, which would make the page work out its
    // styles again each time.
    const readTheme = () => {
      const style = getComputedStyle(canvas);
      return {
        strength: parseFloat(style.getPropertyValue("--backdrop")) || 0.7,
        saturation: parseFloat(style.getPropertyValue("--backdrop-saturation")) || 1,
        background: rgbOf(style.getPropertyValue("--bg")),
      };
    };
    let theme = readTheme();
    const retheme = () => {
      theme = readTheme();
      wake();
    };
    const dark = matchMedia("(prefers-color-scheme: dark)");
    dark.addEventListener("change", retheme);
    const forced = new MutationObserver(retheme);
    forced.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });

    const resize = () => {
      // Drawn pixel for pixel: the dither only hides the bands at the size
      // the screen shows them.
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * devicePixelRatio));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * devicePixelRatio));
      gl.viewport(0, 0, canvas.width, canvas.height);
      // Resizing clears the canvas, and opaque it would show black until the
      // next frame: draw it again at once, with the last frame's pools.
      if (canvas.dataset.ready !== undefined) gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const draw = (now: number) => {
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const { width: w, height: h } = canvas;

      // The glow comes in gently when the view opens: faded here rather than
      // by the canvas's opacity, which would blend the dither away again.
      const arrival = 1 - Math.pow(1 - Math.min(1, (now - born) / 600), 3);
      let moving = arrival < 1;

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
          // A frame swells in quickly and ebbs a little slower: the beat still
          // shows, but a pool this large never flashes on screen, whatever
          // the tempo (the bulbs keep their own speed). Set by hand, it fades
          // gently. Reduced motion keeps the colours but only drifts.
          const tau = still.matches ? 1.5 : driven ? (level > s.level ? 0.06 : 0.15) : 0.35;
          const k = 1 - Math.exp(-dt / tau);
          s.rgb = s.rgb.map((c, i) => c + (rgb[i] - c) * k) as RgbTuple;
          s.level += (level - s.level) * k;
          if (Math.abs(level - s.level) > 0.002 || s.rgb.some((c, i) => Math.abs(rgb[i] - c) > 0.5)) moving = true;
          s.x = x;
          s.y = spot.y;
          shown.set(key, s);
        });
      }
      let n = 0;
      for (const [key, s] of shown) {
        // Lights turned off or stopped fade out rather than vanish.
        if (!seen.has(key)) {
          s.level *= Math.exp(-dt / 0.35);
          if (s.level < 0.01) {
            shown.delete(key);
            continue;
          }
          moving = true;
        }
        if (n === MAX_POOLS) continue;
        // The eye reads a dim bulb as brighter than its share of full power.
        pools.set([s.x * w, (1 - s.y) * h, Math.max(w, h) * (0.32 + 0.12 * s.level), 0.75 * Math.pow(s.level, 0.6)], n * 4);
        colours.set(s.rgb.map((c) => c / 255), n * 3);
        n++;
      }

      gl.uniform4fv(where.pools, pools);
      gl.uniform3fv(where.colours, colours);
      gl.uniform1i(where.count, n);
      gl.uniform1f(where.strength, theme.strength * arrival);
      gl.uniform3fv(where.background, theme.background);
      gl.uniform1f(where.saturation, theme.saturation);
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      canvas.dataset.ready = "";
      // An effect's frames arrive between two draws: keep drawing while one plays.
      raf = moving || app.status.running ? requestAnimationFrame(draw) : 0;
    };
    wake = () => {
      if (raf !== 0) return;
      last = performance.now();
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(raf);
      wake = () => {};
      observer.disconnect();
      forced.disconnect();
      dark.removeEventListener("change", retheme);
    };
  });

  // The theme's background, as the shader takes it. The page writes it in
  // oklch, which a 2D canvas paints and reads back as plain sRGB.
  let bgSource = "";
  let bgRgb: [number, number, number] = [0, 0, 0];
  function rgbOf(css: string): [number, number, number] {
    if (css === bgSource) return bgRgb;
    const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
    if (!ctx) return bgRgb;
    ctx.fillStyle = css.trim() || "#000";
    ctx.fillRect(0, 0, 1, 1);
    const [r, g, b] = ctx.getImageData(0, 0, 1, 1).data;
    bgSource = css;
    bgRgb = [r / 255, g / 255, b / 255];
    return bgRgb;
  }

  function link(gl: WebGLRenderingContext): WebGLProgram | null {
    const program = gl.createProgram();
    for (const [type, source] of [
      [gl.VERTEX_SHADER, VERTEX],
      [gl.FRAGMENT_SHADER, FRAGMENT],
    ] as const) {
      const shader = gl.createShader(type);
      if (!shader) return null;
      gl.shaderSource(shader, source);
      gl.compileShader(shader);
      gl.attachShader(program, shader);
    }
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      console.warn("backdrop:", gl.getProgramInfoLog(program));
      return null;
    }
    return program;
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
  }
  /* Opaque, an unpainted canvas shows black. */
  .backdrop:not([data-ready]) {
    visibility: hidden;
  }
</style>
