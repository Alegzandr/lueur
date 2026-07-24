// TypeScript mirrors of the Rust config structs (see crates/lueur-core/src).
// Field names are snake_case to match serde's default serialisation.

export interface Vec2 {
  x: number;
  y: number;
}

export interface Screen {
  center: Vec2;
  width: number;
  facing_deg: number;
}

export interface SampleZone {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Light {
  id: string;
  name: string;
  pos: Vec2;
  zone_override: SampleZone | null;
  enabled: boolean;
}

export interface Room {
  walls: Vec2[];
  screen: Screen;
  lights: Light[];
}

export interface DeviceBinding {
  light_id: string;
  device_id: string;
  ip: string;
  port: number;
  model: string;
  name: string;
}

export interface CaptureSettings {
  monitor_index: number;
  target_fps: number;
}

export interface EngineSettings {
  smoothing: number;
  brightness: number;
  saturation: number;
  sample_step: number;
  black_threshold: number;
}

export type Effect = "smooth" | "sudden";

export interface TransitionSettings {
  effect: Effect;
  duration_ms: number;
}

export interface BehaviorSettings {
  run_on_startup: boolean;
  minimize_to_tray: boolean;
  start_minimized: boolean;
  autostart_effect: boolean;
}

export interface AppConfig {
  version: number;
  room: Room;
  bindings: DeviceBinding[];
  capture: CaptureSettings;
  engine: EngineSettings;
  transition: TransitionSettings;
  behavior: BehaviorSettings;
}

export interface DiscoveredDevice {
  id: string;
  ip: string;
  port: number;
  model: string;
  name: string;
  support: string[];
  fw_ver: string | null;
}

export interface MonitorInfo {
  index: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

export interface EffectStatus {
  running: boolean;
  connected_lights: number;
}

// A sensible default config used when running outside Tauri (browser preview)
// or before the backend responds.
export function defaultConfig(): AppConfig {
  return {
    version: 1,
    room: {
      walls: [
        { x: 50, y: 50 },
        { x: 950, y: 50 },
        { x: 950, y: 700 },
        { x: 50, y: 700 },
      ],
      screen: { center: { x: 500, y: 160 }, width: 300, facing_deg: 0 },
      lights: [],
    },
    bindings: [],
    capture: { monitor_index: 0, target_fps: 45 },
    engine: {
      smoothing: 0.55,
      brightness: 1.0,
      saturation: 0.35,
      sample_step: 4,
      black_threshold: 6,
    },
    transition: { effect: "smooth", duration_ms: 80 },
    behavior: {
      run_on_startup: false,
      minimize_to_tray: true,
      start_minimized: false,
      autostart_effect: false,
    },
  };
}
