// Scenes the user saved: what every light does, recorded as it is now and
// brought back in one click.

import * as ipc from "../ipc";
import { t } from "../i18n";
import { NATURAL } from "../scenes";
import type { LightState, Mode, SavedScene, SceneLight } from "../types";
import { app } from "./app.svelte";
import { edit, persistNow, refreshStatus } from "./config.svelte";
import { bindingFor, lightById, lightState, modeOf } from "./derived";
import { moveLights } from "./effect";
import { reloadStates, reportPartial } from "./lights";
import { fail, notify } from "./toasts";

/** What one light does now, as a scene records it. */
function recordOf(lightId: string): SceneLight {
  const mode = modeOf(lightId);
  if (mode) return { kind: "mode", mode };
  const state = lightState(lightId);
  if (state.on && app.config.natural?.includes(lightId)) return { kind: "natural" };
  return { kind: "hand", state: $state.snapshot(state) };
}

const sameState = (a: LightState, b: LightState) =>
  a.on === b.on && (!a.on || (a.bright === b.bright && JSON.stringify(a.tint) === JSON.stringify(b.tint)));

/** A scene's lights that still have a bulb, with what each did. */
const recorded = (scene: SavedScene) => Object.entries(scene.lights).filter(([id]) => bindingFor(id));

/** True while every light of the scene does what it did when saved. */
export function showsScene(scene: SavedScene): boolean {
  const lights = recorded(scene);
  return (
    lights.length > 0 &&
    lights.every(([id, s]) => {
      const mode = modeOf(id);
      if (s.kind === "mode") return mode === s.mode;
      if (mode !== null) return false;
      if (s.kind === "natural") return lightState(id).on && (app.config.natural?.includes(id) ?? false);
      return sameState(lightState(id), s.state);
    })
  );
}

/** The scene called `name`, whatever its case and spacing. */
export function sceneNamed(name: string): SavedScene | undefined {
  const key = name.trim().toLocaleLowerCase();
  return app.config.scenes.find((s) => s.name.trim().toLocaleLowerCase() === key);
}

/** Save what every light with a bulb does now as `name`, replacing the
 *  scene of that name if there is one. */
export function saveScene(name: string): void {
  const lights = Object.fromEntries(app.config.bindings.map((b) => [b.light_id, recordOf(b.light_id)]));
  const same = sceneNamed(name);
  edit((c) => {
    if (same) {
      c.scenes = c.scenes.map((s) => (s.id === same.id ? { ...s, lights } : s));
    } else {
      c.scenes = [...c.scenes, { id: `scene-${Date.now().toString(36)}`, name: name.trim(), lights }];
    }
  });
}

/** Delete a scene, with an undo in the toast that says so. */
export function deleteScene(id: string): void {
  const index = app.config.scenes.findIndex((s) => s.id === id);
  if (index < 0) return;
  const scene = $state.snapshot(app.config.scenes[index]);
  edit((c) => (c.scenes = c.scenes.filter((s) => s.id !== id)));
  notify("info", t("scene_deleted", { name: scene.name }), {
    label: t("undo"),
    run: () =>
      edit((c) => {
        const at = Math.min(index, c.scenes.length);
        c.scenes = [...c.scenes.slice(0, at), scene, ...c.scenes.slice(at)];
      }),
  });
}

/** Bring a scene back: each light plays, follows or shows what it did when
 *  the scene was saved. Lights added since are left as they are. */
export async function recallScene(saved: SavedScene): Promise<void> {
  if (app.busy || app.pending) return;
  // A plain copy: what is sent to the bulbs is not the config's live state.
  const scene = $state.snapshot(saved);
  app.pending = scene.id;
  app.busy = true;
  try {
    await persistNow();
    const lights = recorded(scene);
    // A bulb an effect drives refuses a state: the lights set by hand leave
    // their effect first.
    const freed = lights.filter(([id, s]) => s.kind !== "mode" && modeOf(id) !== null).map(([id]) => id);
    if (freed.length > 0) await moveLights(freed, null);

    const playing = new Map<Mode, string[]>();
    for (const [id, s] of lights) {
      if (s.kind === "mode" && modeOf(id) !== s.mode) playing.set(s.mode, [...(playing.get(s.mode) ?? []), id]);
    }
    for (const [mode, ids] of playing) await moveLights(ids, mode);

    const natural = lights.filter(([, s]) => s.kind === "natural").map(([id]) => id);
    if (natural.length > 0) reportPartial(await ipc.applyScene(NATURAL, natural));

    const byHand = lights.flatMap(([id, s]) => (s.kind === "hand" ? [[id, s.state] as const] : []));
    const failures: string[] = [];
    await Promise.all(
      byHand.map(async ([id, state]) => {
        try {
          await ipc.setLightState(id, state);
        } catch {
          failures.push(lightById(id)?.name ?? id);
        }
      }),
    );
    reportPartial({ total: byHand.length, succeeded: byHand.length - failures.length, failures });
    await reloadStates();
    await refreshStatus();
  } catch (e) {
    await refreshStatus();
    fail(e);
  } finally {
    app.busy = false;
    app.pending = null;
  }
}
