import { describe, expect, it } from "vitest";
import { normalizeUserCode, userCodeProblem } from "./state.svelte";

// The Smart Life user code is checked in the window before the backend
// checks it again; both must agree on what a code looks like.

describe("normalizeUserCode", () => {
  it("drops the spaces and dashes people type to read it", () => {
    expect(normalizeUserCode(" ab12 cd-34_ef.56 ")).toBe("ab12cd34ef56");
    expect(normalizeUserCode("AB‐12–CD—34−EF")).toBe("AB12CD34EF");
    expect(normalizeUserCode("a\tb\nc")).toBe("abc");
  });

  it("keeps the letter case, which the codes mix", () => {
    expect(normalizeUserCode("AbCd")).toBe("AbCd");
  });
});

describe("userCodeProblem", () => {
  it("accepts a plausible code", () => {
    expect(userCodeProblem("Ab12")).toBeNull();
    expect(userCodeProblem("x".repeat(64))).toBeNull();
  });

  it("names what is wrong", () => {
    expect(userCodeProblem("")).toBe("tuya_code_empty");
    expect(userCodeProblem("ab1")).toBe("tuya_code_short");
    expect(userCodeProblem("ab/12")).toBe("tuya_code_chars");
    expect(userCodeProblem("éèàç")).toBe("tuya_code_chars");
    expect(userCodeProblem("x".repeat(65))).toBe("tuya_code_chars");
  });

  it("is answered by a key the interface can word", () => {
    // tuya_code_* are i18n keys; a typo here would show a raw key.
    for (const code of ["", "a", "a b"]) {
      expect(userCodeProblem(code)).toMatch(/^tuya_code_(empty|chars|short)$/);
    }
  });
});
