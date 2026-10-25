import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import Switch from "./Switch.svelte";
import Slider from "./Slider.svelte";
import Segmented from "./Segmented.svelte";

// The base controls are what a screen reader and a keyboard user meet first.

describe("Switch", () => {
  it("is a named switch that reports its state", () => {
    render(Switch, { checked: true, label: "Desk lamp", onchange: () => {} });
    const sw = screen.getByRole("switch", { name: "Desk lamp" });
    expect(sw.getAttribute("aria-checked")).toBe("true");
    expect(sw.getAttribute("type")).toBe("button");
  });

  it("flips on click, and on Enter or Space as a native button does", async () => {
    const onchange = vi.fn();
    render(Switch, { checked: false, label: "Desk lamp", onchange });
    const sw = screen.getByRole("switch");
    expect(sw.getAttribute("aria-checked")).toBe("false");
    sw.focus();
    expect(document.activeElement).toBe(sw);
    await fireEvent.click(sw);
    expect(onchange).toHaveBeenCalledWith(true);
    // A <button> turns Enter and Space into a click: no key handler of its own
    // that could swallow them.
    expect(sw.tagName).toBe("BUTTON");
  });

  it("does nothing when disabled", async () => {
    const onchange = vi.fn();
    render(Switch, { checked: false, label: "Desk lamp", disabled: true, onchange });
    const sw = screen.getByRole("switch");
    expect((sw as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(sw);
    expect(onchange).not.toHaveBeenCalled();
  });
});

describe("Slider", () => {
  const props = {
    label: "Brightness",
    value: 40,
    min: 0,
    max: 100,
    step: 5,
    def: 60,
    format: (v: number) => `${v}%`,
  };

  it("is a named range that reads its value in words", () => {
    render(Slider, { ...props, onchange: () => {} });
    const range = screen.getByRole("slider", { name: "Brightness" });
    expect(range.getAttribute("aria-valuetext")).toBe("40%");
    expect(range.getAttribute("min")).toBe("0");
    expect(range.getAttribute("max")).toBe("100");
    expect(range.getAttribute("step")).toBe("5");
  });

  it("reports what the keyboard or the pointer sets", async () => {
    const onchange = vi.fn();
    render(Slider, { ...props, onchange });
    const range = screen.getByRole("slider") as HTMLInputElement;
    // Arrow keys move a native range and fire `input`, which is all it hears.
    await fireEvent.input(range, { target: { value: "45" } });
    expect(onchange).toHaveBeenCalledWith(45);
  });

  it("offers a reset in the tab order only once off its default", async () => {
    const onchange = vi.fn();
    const { rerender } = render(Slider, { ...props, onchange });
    const reset = screen.getByRole("button", { name: /^Brightness: / });
    expect(reset.getAttribute("tabindex")).toBe("0");
    await fireEvent.click(reset);
    expect(onchange).toHaveBeenCalledWith(60);
    await rerender({ ...props, value: 60, onchange });
    expect(reset.getAttribute("tabindex")).toBe("-1");
  });
});

describe("Segmented", () => {
  const options = [
    { value: "30", label: "30" },
    { value: "45", label: "45" },
    { value: "60", label: "60" },
  ];

  it("is a named radio group with one stop in the tab order", () => {
    render(Segmented, { value: "45", options, label: "Frame rate", onchange: () => {} });
    expect(screen.getByRole("radiogroup", { name: "Frame rate" })).toBeTruthy();
    const radios = screen.getAllByRole("radio");
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
    expect(radios.map((r) => r.getAttribute("tabindex"))).toEqual(["-1", "0", "-1"]);
  });

  it("moves the choice with the arrows, Home and End, wrapping around", async () => {
    const onchange = vi.fn();
    render(Segmented, { value: "60", options, label: "Frame rate", onchange });
    const chosen = screen.getByRole("radio", { checked: true });
    await fireEvent.keyDown(chosen, { key: "ArrowRight" });
    expect(onchange).toHaveBeenLastCalledWith("30");
    expect(document.activeElement).toBe(screen.getAllByRole("radio")[0]);
    await fireEvent.keyDown(chosen, { key: "ArrowUp" });
    expect(onchange).toHaveBeenLastCalledWith("45");
    await fireEvent.keyDown(chosen, { key: "Home" });
    expect(onchange).toHaveBeenLastCalledWith("30");
    await fireEvent.keyDown(chosen, { key: "End" });
    expect(onchange).toHaveBeenLastCalledWith("60");
    // Other keys are left to the browser.
    onchange.mockClear();
    await fireEvent.keyDown(chosen, { key: "a" });
    expect(onchange).not.toHaveBeenCalled();
  });
});
