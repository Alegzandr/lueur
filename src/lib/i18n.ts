// Interface strings in English and French.
//
// `t(key)` returns the string for the current language; `{name}` placeholders
// are filled from `vars`. Counted strings are [one, other] pairs read with
// `tn(key, n)`: French treats 0 and 1 as singular, English only 1.
//
// Copy rule: say it in as few words as the control needs. Explanations go in
// tooltips (`*_tip`), never in the layout.

import { app } from "./state.svelte";
import enCatalog from "../../locales/en.json";
import frCatalog from "../../locales/fr.json";

const NB = " "; // French puts a non-breaking space before : ? ! and %

// The catalogs live in locales/*.json, shared with the GPUI interface. Error
// strings are `e_` and a code from crates/lueur-shell/src/error.rs.
const en = enCatalog.strings;
type Key = keyof typeof en;
const fr: Record<Key, string> = frCatalog.strings;

const enCounts = enCatalog.counts;
type CountKey = keyof typeof enCounts;
const frCounts: Record<CountKey, string[]> = frCatalog.counts;

export type Lang = "en" | "fr";

/** The language in use: the user's choice, or the system's when automatic. */
export function lang(): Lang {
  const pref = app.config.interface?.language ?? "system";
  if (pref === "en" || pref === "fr") return pref;
  return typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("fr")
    ? "fr"
    : "en";
}

function fill(s: string, vars?: Record<string, string | number>): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
}

export function t(key: Key, vars?: Record<string, string | number>): string {
  return fill((lang() === "fr" ? fr : en)[key], vars);
}

export function tn(key: CountKey, n: number): string {
  const forms = (lang() === "fr" ? frCounts : enCounts)[key];
  const one = lang() === "fr" ? n <= 1 : n === 1;
  return fill(one ? forms[0] : forms[1], { n });
}

/** "45 %" in French, "45%" in English. */
export function pct(v: number): string {
  const n = Math.round(v * 100);
  return lang() === "fr" ? `${n}${NB}%` : `${n}%`;
}

/** A multiplier: "×0,5" in French, "×0.5" in English, with no trailing zero. */
export function times(v: number): string {
  const n = String(Math.round(v * (v < 1 ? 100 : 10)) / (v < 1 ? 100 : 10));
  return `×${lang() === "fr" ? n.replace(".", ",") : n}`;
}

/** An error from the backend: a code from crates/lueur-shell/src/error.rs. */
interface BackendError {
  code: string;
  subject?: string;
  detail?: string;
}

export function isBackendError(e: unknown): e is BackendError {
  return typeof e === "object" && e !== null && typeof (e as BackendError).code === "string";
}

/** Any thrown value as a sentence in the interface language. */
export function errorText(e: unknown): string {
  if (isBackendError(e)) {
    const key = `e_${e.code}` as Key;
    return key in en ? t(key, { subject: e.subject ?? "" }) : t("e_internal");
  }
  if (e instanceof Error) return e.message;
  return typeof e === "string" ? e : t("e_internal");
}

/** The untranslated technical cause, if the backend sent one. */
export function errorDetail(e: unknown): string | undefined {
  return isBackendError(e) ? e.detail : undefined;
}

/** The raw catalogs, for the parity test (src/lib/i18n.test.ts). */
export const catalogs = { en, fr, enCounts, frCounts };
