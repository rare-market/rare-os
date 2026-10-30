import { describe, expect, it } from "vitest";
import { dockChange, dockedLine, nextScale, rootPx, scaleOf } from "./display";
import type { DisplayState } from "./types";

const tv = (mode: string, name = "LG TV"): DisplayState => ({ outputs: [], docked: true, dockName: name, dockMode: mode });
const own: DisplayState = { outputs: [], docked: false };

describe("rootPx", () => {
  it("matches the stylesheet at the sizes we ship", () => {
    expect(rootPx(1280, 800)).toBe(16);
    expect(rootPx(1920, 1200)).toBe(24);
    expect(rootPx(1920, 1080)).toBeCloseTo(21.6);
    expect(rootPx(3840, 2160)).toBeCloseTo(43.2);
  });
  it("keeps the layout proportional, so 4K is not tiny", () => {
    // Same number of rem across the screen at 1080p and 4K.
    expect(3840 / rootPx(3840, 2160)).toBeCloseTo(1920 / rootPx(1920, 1080));
  });
  it("limits by the short side on an unrotated portrait panel", () => {
    expect(rootPx(1200, 1920)).toBe(15);
  });
  it("applies Large and Larger", () => {
    expect(rootPx(1920, 1080, "large")).toBeCloseTo(21.6 * 1.15);
    expect(rootPx(1920, 1080, "larger")).toBeCloseTo(21.6 * 1.3);
  });
});

describe("scale choice", () => {
  it("defaults to automatic and ignores junk", () => {
    expect(scaleOf(null)).toBe("auto");
    expect(scaleOf({})).toBe("auto");
    expect(scaleOf({ uiScale: "larger" })).toBe("larger");
    expect(scaleOf({ uiScale: "huge" as never })).toBe("auto");
  });
  it("cycles", () => {
    expect(nextScale("auto")).toBe("large");
    expect(nextScale("large")).toBe("larger");
    expect(nextScale("larger")).toBe("auto");
  });
});

describe("dock text", () => {
  it("describes the state", () => {
    expect(dockedLine(own)).toBe("Built-in screen");
    expect(dockedLine(tv("3840x2160"))).toBe("Docked to LG TV 3840x2160");
    expect(dockedLine({ outputs: [], docked: true, active: "DP-1" })).toBe("Docked to DP-1");
  });
  it("toasts on connect, disconnect and mode change only", () => {
    expect(dockChange(null, tv("3840x2160"))).toBeNull();
    expect(dockChange(own, tv("3840x2160"))).toBe("Docked to LG TV 3840x2160");
    expect(dockChange(tv("3840x2160"), own)).toBe("Back on the built-in screen");
    expect(dockChange(tv("3840x2160"), tv("1920x1080"))).toBe("Docked to LG TV 1920x1080");
    expect(dockChange(own, own)).toBeNull();
    expect(dockChange(tv("3840x2160"), tv("3840x2160"))).toBeNull();
  });
});
