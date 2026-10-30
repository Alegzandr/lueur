// Central app state, built on Svelte 5 runes so components read it directly
// (`app.config.room.lights`) and mutate it through `edit`, which persists the
// change to the backend.
//
// The state and its actions live in `state/`, one concern per file; this is
// the one place the rest of the app imports them from.

export { app, type Pairing, type Toast, type View } from "./state/app.svelte";
export {
  bindingFor,
  brightOf,
  drivenElsewhere,
  effectTargets,
  endpointOf,
  endpointOfDevice,
  isDriven,
  isOn,
  isReconnecting,
  lightById,
  lightState,
  modeOf,
  moodTargets,
  needsRestart,
  pickedLights,
  toggleTarget,
} from "./state/derived";
export { bootstrap, edit, exportConfig, importConfig, refreshStatus } from "./state/config.svelte";
export {
  applyScene,
  chooseMode,
  restartEffect,
  setSleepTimer,
  sleepChanged,
  takeLight,
  toggleEffect,
} from "./state/effect";
export {
  adjustLight,
  identify,
  identifyLight,
  reloadStates,
  setAllPower,
  setBrightness,
  setLightPower,
  toggleLightPower,
} from "./state/lights";
export { deleteScene, recallScene, saveScene, sceneNamed, showsScene } from "./state/scenes.svelte";
export {
  PAIR_WINDOW_MS,
  addReadyTuya,
  closePairing,
  closeTuya,
  finishTuyaImport,
  normalizeUserCode,
  openTuya,
  pair,
  signInTuya,
  userCodeProblem,
} from "./state/pairing.svelte";
export {
  addDevices,
  addLight,
  assignDevice,
  deviceName,
  markRoomArranged,
  removeLight,
  renameLight,
  scan,
  setLightEnabled,
  showOnPlan,
  unassign,
} from "./state/devices.svelte";
export { dismiss, fail, holdToast, notify, releaseToast } from "./state/toasts";
