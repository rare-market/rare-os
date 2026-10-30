import { describe, expect, it } from "vitest";
import type { Candle } from "./api";
import { clampIndex, readout, scrollRange, showBar, sma, stickRate, vwap, zoomRange } from "./chartmode";

const bar = (time: number, close: number, volume = 1): Candle => ({ time, open: close, high: close + 1, low: close - 1, close, volume });

describe("indicators", () => {
  it("sma starts at the nth bar", () => {
    const bars = [1, 2, 3, 4, 5].map((c, i) => bar(i, c));
    expect(sma(bars, 3).map((p) => p.value)).toEqual([2, 3, 4]);
  });
  it("vwap weights by volume and restarts each day", () => {
    const a = [bar(0, 10, 1), bar(60, 20, 3)];
    expect(vwap(a)[1].value).toBeCloseTo((10 * 1 + 20 * 3) / 4, 6);
    expect(vwap([...a, bar(86_400, 50, 2)])[2].value).toBeCloseTo(50, 6);
  });
});

describe("range maths", () => {
  it("keeps the cursor on screen", () => {
    expect(showBar({ from: 10, to: 50 }, 30)).toEqual({ from: 10, to: 50 });
    expect(showBar({ from: 10, to: 50 }, 9)).toEqual({ from: 7, to: 47 });
    expect(showBar({ from: 10, to: 50 }, 60)).toEqual({ from: 22, to: 62 });
  });
  it("zooms about the newest end and stops at the limits", () => {
    expect(zoomRange({ from: 100, to: 200 }, 0.5, 300)).toEqual({ from: 150, to: 200 });
    expect(zoomRange({ from: 100, to: 120 }, 0.1, 300).to - 108).toBe(12);
    const wide = zoomRange({ from: 0, to: 300 }, 10, 300);
    expect(wide.to - wide.from).toBe(308);
  });
  it("scrolls inside the data", () => {
    expect(scrollRange({ from: 0, to: 50 }, -20, 300).from).toBe(-4);
    expect(scrollRange({ from: 240, to: 290 }, 100, 300).to).toBe(303);
  });
  it("clamps indexes", () => {
    expect(clampIndex(-3, 10)).toBe(0);
    expect(clampIndex(99, 10)).toBe(9);
    expect(clampIndex(2.6, 10)).toBe(3);
  });
  it("stick rate is quadratic and signed", () => {
    expect(stickRate(0.5, 40)).toBe(10);
    expect(stickRate(-1, 40)).toBe(-40);
  });
  it("readout change is against the bar before", () => {
    expect(readout(bar(1, 110), bar(0, 100)).changePct).toBeCloseTo(10, 6);
  });
});
