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
  /** 0 = floor, 1 = ceiling, 0.5 = level with the screen. Selects the
   *  horizontal band of the picture this light mirrors. */
  height: number;
  zone_override: SampleZone | null;
  enabled: boolean;
}

export interface Room {
  walls: Vec2[];
  screen: Screen;
  lights: Light[];
  /** Colour of the walls; every light is corrected for it. */
  wall: Rgb;
}

/** Where a Tuya bulb keeps each setting (see lueur-core device/tuya.rs). */
export interface TuyaDps {
  switch: number;
  mode: number;
  bright: number;
  temp: number;
  colour: number;
  control: number | null;
  legacy: boolean;
  /** Absent from layouts saved before Lueur played scenes. */
  scene?: number | null;
}

/** Which protocol a bulb speaks, plus what it needs to be spoken. */
export type DeviceKind =
  | { family: "yeelight" }
  | { family: "wiz" }
  | { family: "lifx"; target: string }
  | { family: "govee" }
  | { family: "hue"; username: string; light: string; cert?: string }
  | { family: "nanoleaf"; token: string }
  | { family: "tuya"; key: string; version: string; dps: TuyaDps };

export type Family = DeviceKind["family"];

/** Everything the backend needs to reach one bulb. */
export interface Endpoint {
  id: string;
  ip: string;
  port: number;
  kind: DeviceKind;
}

export interface DeviceBinding {
  light_id: string;
  device_id: string;
  ip: string;
  port: number;
  model: string;
  name: string;
  /** Absent in configs saved before other families: Yeelight. */
  kind?: DeviceKind;
}

export interface HueBridge {
  id: string;
  ip: string;
  username: string;
  cert?: string;
}

/** A paired bridge or controller the last search heard where nothing
 *  proves it is the one paired (see src-tauri/src/identity.rs). */
export interface UnverifiedDevice {
  id: string;
  family: "hue" | "nanoleaf";
  name: string;
  /** Where it was heard. */
  ip: string;
  /** Where it was paired. */
  known_ip: string;
  /** At its own address, but with another certificate. */
  changed: boolean;
  /** A bridge whose pinned certificate it no longer shows: it can only be
   *  paired again, never confirmed. */
  needs_pairing: boolean;
}

export interface NanoleafPairing {
  id: string;
  ip: string;
  token: string;
}

export interface TuyaDevice {
  id: string;
  name: string;
  key: string;
  product: string;
  dps: TuyaDps;
}

/** What bridges and accounts granted Lueur. */
export interface Pairings {
  hue: HueBridge[];
  nanoleaf: NanoleafPairing[];
  tuya: TuyaDevice[];
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
  white_threshold: number;
  white_kelvin: number;
  /** Leave the black bars around a film out of what the lights follow. */
  crop_bars: boolean;
  /** How dark scenes light the room: 1 keeps them lit, 2.2 as on screen. */
  brightness_curve: number;
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
  /** Start the ambilight when something goes fullscreen (Windows). */
  auto_fullscreen: boolean;
}

/** The hue of a light set by hand (see lueur-core scene.rs). */
export type Tint = { mode: "white"; kelvin: number } | { mode: "color"; r: number; g: number; b: number };

/** What one light shows when the screen is not driving it. */
export interface LightState {
  on: boolean;
  /** 1..=100 */
  bright: number;
  tint: Tint;
}

/** The light shows (see lueur-core show.rs). */
export type ShowKind = "music" | "rainbow" | "sunset" | "ocean" | "candle";

export interface ShowSettings {
  kind: ShowKind;
  /** 1..=100 */
  bright: number;
  /** 1..=100; 50 is each show's natural pace. */
  speed: number;
  /** The music may flash white on its drops, at most three a second. */
  flashes: boolean;
  /** How late the sound reaches the ear, ms, for the music to wait on
   *  (`DELAY_MS` in show.rs: -150..=500). */
  delay_ms: number;
}

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export type Appearance = "system" | "light" | "dark";

export type Language = "system" | "en" | "fr";

export interface InterfaceSettings {
  appearance: Appearance;
  language: Language;
  /** The user has arranged the room plan at least once. */
  room_arranged: boolean;
}

/** The photosensitivity protection (see lueur-core safety.rs). */
export interface SafetySettings {
  /** No light flashes more than three times a second, and the ambilight
   *  brightens gently. */
  flash_guard: boolean;
}

/** What the music show listens to (see lueur-core config.rs). A device or an
 *  application keeps the name it showed when picked, for while it is gone. */
export type AudioSource =
  | { kind: "default" }
  | { kind: "device"; id: string; name: string }
  | { kind: "app"; exe: string; name: string };

/** What the music show can listen to right now. */
export interface AudioSources {
  /** Outputs and applications (Windows), rather than inputs. */
  loopback: boolean;
  devices: { id: string; name: string; is_default: boolean }[];
  /** The applications that have played sound since they opened. */
  apps: { exe: string; name: string }[];
}

export interface AppConfig {
  version: number;
  room: Room;
  bindings: DeviceBinding[];
  pairings: Pairings;
  capture: CaptureSettings;
  engine: EngineSettings;
  transition: TransitionSettings;
  behavior: BehaviorSettings;
  interface: InterfaceSettings;
  /** What each light was last set to by hand, by room light id. */
  states: Record<string, LightState>;
  show: ShowSettings;
  audio: AudioSource;
  /** The lights that follow the time of day, by room light id. The backend
   *  switches them (moods, lights set by hand other than dimmed) and ignores
   *  the copy the interface saves. */
  natural?: string[];
  /** What each light plays, by room light id, as last chosen; the backend
   *  owns it. */
  modes?: Record<string, Mode>;
  safety: SafetySettings;
}

/** A running sleep timer. */
export interface SleepStatus {
  /** What it was set to. */
  minutes: number;
  /** Seconds until the lights go off. */
  left: number;
}

export interface DiscoveredDevice {
  id: string;
  ip: string;
  port: number;
  model: string;
  name: string;
  support: string[];
  fw_ver: string | null;
  kind: DeviceKind;
  /** False for a Hue bridge or Nanoleaf that still needs pairing. */
  paired: boolean;
  /** A Hue bridge paired before Lueur asked for its streaming key: its
   *  lamps work, and pairing it again lets them keep up with the music. */
  faster?: boolean;
}

/** A Smart Life sign-in waiting for its QR code to be scanned. */
export interface TuyaLogin {
  token: string;
  /** SVG markup generated by the backend. */
  svg: string;
}

export interface MonitorInfo {
  index: number;
  name: string;
  width: number;
  height: number;
  is_primary: boolean;
}

/** `ambilight` or a show. */
export type Mode = "ambilight" | ShowKind;

/** One running effect: a mode and the lights playing it, in step. */
export interface EffectGroup {
  mode: Mode;
  light_ids: string[];
}

/** What a driven light shows, one zone of it: a colour or a white, as sent
 *  to its bulb (`Glow` in runner.rs). */
export interface Glow {
  rgb: [number, number, number] | null;
  kelvin: number | null;
  /** 0..=100. */
  bright: number;
}

/** A room light whose bulb something else drives. */
export interface Elsewhere {
  light_id: string;
  /** The computer running Lueur that drives it, or null for another app. */
  by: string | null;
}

export interface EffectStatus {
  running: boolean;
  connected_lights: number;
  /** The mode playing, when exactly one does. */
  mode: Mode | null;
  /** Every room light an effect drives. */
  light_ids: string[];
  /** Each mode playing and its lights. */
  groups: EffectGroup[];
  /** The driven lights whose bulb dropped and is being reconnected. */
  reconnecting: string[];
  /** The room lights whose bulb something else drives: driven here, they
   *  wait for it, and the user may take it. */
  elsewhere: Elsewhere[];
  /** The music plays but its sound source is away, and it waits for it. */
  music_waiting: boolean;
  /** The ambilight reads an HDR screen, its colours brought back to SDR. */
  hdr: boolean;
  /** The ambilight has not been able to read the screen for a while. */
  screen_lost: boolean;
}

export interface PowerResult {
  total: number;
  succeeded: number;
  failures: string[];
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
      wall: { r: 255, g: 255, b: 255 },
    },
    bindings: [],
    pairings: { hue: [], nanoleaf: [], tuya: [] },
    capture: { monitor_index: 0, target_fps: 45 },
    engine: {
      smoothing: 0.55,
      brightness: 1.0,
      saturation: 0.35,
      sample_step: 4,
      black_threshold: 6,
      white_threshold: 0.18,
      white_kelvin: 4000,
      crop_bars: true,
      brightness_curve: 1.0,
    },
    transition: { effect: "smooth", duration_ms: 80 },
    behavior: {
      run_on_startup: false,
      minimize_to_tray: true,
      start_minimized: false,
      auto_fullscreen: true,
    },
    interface: { appearance: "system", language: "system", room_arranged: false },
    states: {},
    show: { kind: "music", bright: 80, speed: 50, flashes: true, delay_ms: 0 },
    audio: { kind: "default" },
    natural: [],
    safety: { flash_guard: true },
  };
}
