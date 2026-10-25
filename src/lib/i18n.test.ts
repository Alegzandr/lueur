import { describe, expect, it } from "vitest";
import { catalogs } from "./i18n";

// Every string the interface shows exists in both languages, says something,
// and takes the same placeholders: a `{name}` only one language fills would
// show up raw in the other.

const { en, fr, enCounts, frCounts } = catalogs;

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe("strings", () => {
  it("has the same keys in English and French", () => {
    expect(Object.keys(fr).sort()).toEqual(Object.keys(en).sort());
  });

  it("leaves no string empty", () => {
    for (const [lang, table] of [
      ["en", en],
      ["fr", fr],
    ] as const) {
      for (const [key, text] of Object.entries(table)) {
        expect(text.trim(), `${lang}.${key}`).not.toBe("");
      }
    }
  });

  it("takes the same placeholders in both languages", () => {
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(placeholders(fr[key]), key).toEqual(placeholders(en[key]));
    }
  });
});

describe("counted strings", () => {
  it("has the same keys in English and French", () => {
    expect(Object.keys(frCounts).sort()).toEqual(Object.keys(enCounts).sort());
  });

  it("has a singular and a plural, neither empty", () => {
    for (const [lang, table] of [
      ["en", enCounts],
      ["fr", frCounts],
    ] as const) {
      for (const [key, forms] of Object.entries(table)) {
        expect(forms, `${lang}.${key}`).toHaveLength(2);
        for (const form of forms) expect(form.trim(), `${lang}.${key}`).not.toBe("");
      }
    }
  });

  it("takes the same placeholders in both languages", () => {
    for (const key of Object.keys(enCounts) as (keyof typeof enCounts)[]) {
      for (const i of [0, 1]) {
        expect(placeholders(frCounts[key][i]), `${key}[${i}]`).toEqual(placeholders(enCounts[key][i]));
      }
    }
  });
});
