// Notifications: the toasts, their countdowns, and how failures are reported.

import { errorDetail, errorText, isBackendError, t } from "../i18n";
import { app, type Toast } from "./app.svelte";

let toastSeq = 0;

/** Each toast's countdown: what is left of it, and what pauses it (the
 *  pointer or focus on the toast). */
interface Countdown {
  handle: ReturnType<typeof setTimeout> | null;
  left: number;
  since: number;
  held: Set<"hover" | "focus">;
}
const countdowns = new Map<number, Countdown>();

/** At most this many toasts show at once. */
const MAX_TOASTS = 4;

/** Read-out counter shared by every toast (see `Toast.said`). */
let saidSeq = 0;

/** Start (or start over) an information toast's countdown. */
function countDown(id: number, left: number): void {
  const old = countdowns.get(id);
  if (old?.handle) clearTimeout(old.handle);
  const held = old?.held ?? new Set<"hover" | "focus">();
  // Held by the pointer or focus: it starts once they move on.
  const handle = held.size > 0 ? null : setTimeout(() => dismiss(id), left);
  countdowns.set(id, { handle, left, since: Date.now(), held });
}

/**
 * Show a toast. Errors stay until closed, so there is time to read them and
 * pass them on; information goes by itself, later when it has an action.
 *
 * The same message again counts up on the toast showing, which keeps its
 * place, focus and unfolded details, and is read out again. Toasts with an
 * action never merge: each "Undo" undoes its own change. Past the limit the
 * oldest information goes first; information never pushes out an error.
 */
function toast(entry: Omit<Toast, "id" | "said">): void {
  const left = entry.action ? 7000 : 3600;
  const same = entry.action
    ? undefined
    : app.toasts.find((t) => !t.action && t.kind === entry.kind && t.msg === entry.msg && t.detail === entry.detail);
  if (same) {
    same.count = (same.count ?? 1) + 1;
    same.said = ++saidSeq;
    if (same.kind === "info") countDown(same.id, left);
    return;
  }
  while (app.toasts.length >= MAX_TOASTS) {
    const oldest = app.toasts.find((t) => t.kind === "info") ?? (entry.kind === "error" ? app.toasts[0] : undefined);
    // Only errors showing and this is information: it shows over the limit
    // for its few seconds rather than take an error's place.
    if (!oldest) break;
    dismiss(oldest.id);
  }
  const id = ++toastSeq;
  app.toasts = [...app.toasts, { id, ...entry, said: ++saidSeq }];
  if (entry.kind === "info") countDown(id, left);
}

/** Pause a toast's countdown while it is pointed at or focused. */
export function holdToast(id: number, why: "hover" | "focus"): void {
  const c = countdowns.get(id);
  if (!c) return;
  c.held.add(why);
  if (!c.handle) return;
  clearTimeout(c.handle);
  c.handle = null;
  c.left -= Date.now() - c.since;
}

/** Resume it once nothing holds it, with a moment left to finish reading. */
export function releaseToast(id: number, why: "hover" | "focus"): void {
  const c = countdowns.get(id);
  if (!c) return;
  c.held.delete(why);
  if (c.handle || c.held.size > 0) return;
  c.left = Math.max(c.left, 2000);
  c.since = Date.now();
  c.handle = setTimeout(() => dismiss(id), c.left);
}

export function notify(kind: Toast["kind"], msg: string, action?: Toast["action"]): void {
  toast({ kind, msg, action });
}

export function dismiss(id: number): void {
  const c = countdowns.get(id);
  if (c?.handle) clearTimeout(c.handle);
  countdowns.delete(id);
  app.toasts = app.toasts.filter((t) => t.id !== id);
}

/**
 * Report a failure. Backend errors arrive as codes and are worded in the
 * interface language; `fallback` words anything else (a frontend-only step).
 */
export function fail(
  e: unknown,
  fallback?: "err_save" | "err_load" | "err_displays",
  action?: Toast["action"],
): void {
  const msg = isBackendError(e) || !fallback ? errorText(e) : t(fallback);
  toast({ kind: "error", msg, detail: errorDetail(e), action });
}
