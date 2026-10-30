// The app state itself: one object of runes that components read directly
// (`app.config.room.lights`). The actions that change it live beside it.

import {
  defaultConfig,
  type AppConfig,
  type DiscoveredDevice,
  type EffectStatus,
  type SleepStatus,
  type TuyaLogin,
} from "../types";

export type View = "lighting" | "room" | "bulbs" | "settings";

export interface Toast {
  id: number;
  kind: "info" | "error";
  msg: string;
  /** Untranslated technical cause, shown on request and copied with the message. */
  detail?: string;
  action?: { label: string; run: () => void };
  /** How many times this same message came, when more than once. */
  count?: number;
  /** Bumped each time the toast should be read out: when it arrives, and
   *  when the same message comes again. */
  said: number;
}

/** A bridge or controller being paired: waiting for its button to be
 *  pressed, then how it went. */
export interface Pairing {
  device: DiscoveredDevice;
  step: "waiting" | "paired" | "failed";
  /** When the device stops waiting for its button (epoch ms). */
  until: number;
  /** Why it failed: the button was not pressed in time, or an error. */
  failure?: { expired: boolean; msg: string; detail?: string };
}

/** The Smart Life sign-in: the user code, the QR code to scan, then the
 *  bulbs looked for on the network and added. */
export type TuyaFlow =
  | { step: "code" | "starting"; expired?: boolean }
  | { step: "qr"; login: TuyaLogin }
  | { step: "adding"; imported: number }
  | {
      step: "done";
      imported: number;
      /** How many were added to the room as new lights. */
      added: number;
      /** Found but left for the user to give to lights already drawn. */
      ready: string[];
      /** Fewer bulbs answered than were imported. The backend only says how
       *  many it imported, not which, so this cannot be a count. */
      missing: boolean;
    };

/** What the backend reports while nothing runs. */
export const idle = (): EffectStatus => ({
  running: false,
  connected_lights: 0,
  mode: null,
  light_ids: [],
  groups: [],
  reconnecting: [],
  elsewhere: [],
  music_waiting: false,
  hdr: false,
  screen_lost: false,
});

class AppState {
  view = $state<View>("lighting");
  config = $state<AppConfig>(defaultConfig());
  status = $state<EffectStatus>(idle());
  /** True while the effect is being started or stopped. */
  busy = $state(false);
  /** True once the backend config has arrived, so the UI does not flash the
   *  first-run guide at someone who already has a setup. */
  ready = $state(false);
  toasts = $state<Toast[]>([]);

  // Discovery results live here so they survive switching views.
  discovered = $state<DiscoveredDevice[]>([]);
  hasScanned = $state(false);
  scanning = $state(false);
  /** The bridge or controller being paired, until the user closes it. */
  pairing = $state<Pairing | null>(null);
  /** The Smart Life sign-in in progress, if any. */
  tuya = $state<TuyaFlow | null>(null);

  powerBusy = $state<Record<string, boolean>>({});
  /** The mood or mode being switched to, while it starts. */
  pending = $state<string | null>(null);
  /** The room was just arranged for the first time: the first-run guide
   *  stays for its last step, starting Screen, until the lights follow it. */
  lastStepPending = $state(false);

  /** The lights effects and moods go to; none picked means the whole room. */
  target = $state<string[]>([]);

  /** The light selected on the plan, shared so other views can deep-link. */
  selectedLight = $state<string | null>(null);

  /** What the running effect was started with (see `topology`). */
  startedWith = $state<string | null>(null);

  /** The sleep timer, if set, and when it was last heard of (to count down). */
  sleep = $state<{ status: SleepStatus; at: number } | null>(null);

  /** Version installed in the background, applied at the next restart. */
  updateReady = $state<string | null>(null);

  /** Unverified bridges answered "not mine" this session, each with what it
   *  showed then (see `DeviceCheck`): asked again only if that changes. */
  dismissedDevices = $state<string[]>([]);
}

export const app = new AppState();
