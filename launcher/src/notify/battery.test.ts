import { describe, expect, it } from "vitest";
import { lowBattery } from "./battery";

describe("low battery", () => {
  const dis = (percent: number) => ({ percent, charging: false });
  it("warns once at 15 % and once at 5 %", () => {
    let w: number | null = null;
    const step = (p: number) => {
      const r = lowBattery(w, dis(p));
      w = r.warned;
      return r.warn;
    };
    expect([30, 16, 15, 14, 13, 6, 5, 4, 3].map(step)).toEqual([null, null, 15, null, null, null, 5, null, null]);
  });
  it("jumping below 5 % warns for 5 % only, and charging re-arms", () => {
    const a = lowBattery(null, dis(4));
    expect(a).toEqual({ warn: 5, warned: 5 });
    expect(lowBattery(a.warned, { percent: 10, charging: true })).toEqual({ warn: null, warned: null });
    expect(lowBattery(null, dis(14)).warn).toBe(15);
  });
  it("does not repeat at 15 % when the reading wobbles, and says nothing without a battery", () => {
    expect(lowBattery(15, dis(16))).toEqual({ warn: null, warned: 15 });
    expect(lowBattery(15, dis(15)).warn).toBeNull();
    expect(lowBattery(15, dis(20))).toEqual({ warn: null, warned: null });
    expect(lowBattery(15, null)).toEqual({ warn: null, warned: 15 });
  });
});
