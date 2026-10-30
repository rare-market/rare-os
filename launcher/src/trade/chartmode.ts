// The pure part of chart mode: the cursor bar, the visible range, zoom, and the Trade API's indicator lines
// (MA 20, MA 50, VWAP). Kept free of the chart library so it is tested on its own.

import type { Candle } from "./api";

export type ChartType = "candles" | "line";
export const CHART_TYPES: ChartType[] = ["candles", "line"];

export type IndicatorKey = "ma20" | "ma50" | "vwap";
/** A cycles through these: none, MA 20, MA 20 + MA 50, all three. */
export const INDICATOR_STEPS: IndicatorKey[][] = [[], ["ma20"], ["ma20", "ma50"], ["ma20", "ma50", "vwap"]];
export const INDICATOR_COLORS: Record<IndicatorKey, string> = { ma20: "#f5c542", ma50: "#5b8cff", vwap: "#c77dff" };
export const INDICATOR_LABELS: Record<IndicatorKey, string> = { ma20: "MA 20", ma50: "MA 50", vwap: "VWAP" };

export interface Range {
  from: number;
  to: number;
}
export interface LinePoint {
  time: number;
  value: number;
}

export const MIN_BARS = 12;
export const RIGHT_PAD = 4;

/** Simple moving average of the closes; the first n-1 bars have no value. */
export function sma(bars: Candle[], n: number): LinePoint[] {
  const out: LinePoint[] = [];
  let sum = 0;
  for (let i = 0; i < bars.length; i++) {
    sum += bars[i].close;
    if (i >= n) sum -= bars[i - n].close;
    if (i >= n - 1) out.push({ time: bars[i].time, value: sum / n });
  }
  return out;
}

/** Volume weighted average price, restarting each UTC day. */
export function vwap(bars: Candle[]): LinePoint[] {
  const out: LinePoint[] = [];
  let day = -1;
  let pv = 0;
  let v = 0;
  for (const b of bars) {
    const d = Math.floor(b.time / 86_400);
    if (d !== day) {
      day = d;
      pv = 0;
      v = 0;
    }
    const typical = (b.high + b.low + b.close) / 3;
    const w = b.volume ?? 0;
    pv += typical * w;
    v += w;
    if (v > 0) out.push({ time: b.time, value: pv / v });
  }
  return out;
}

export function indicatorLine(key: IndicatorKey, bars: Candle[]): LinePoint[] {
  return key === "ma20" ? sma(bars, 20) : key === "ma50" ? sma(bars, 50) : vwap(bars);
}

export const clampIndex = (i: number, n: number) => (n <= 0 ? 0 : Math.max(0, Math.min(n - 1, Math.round(i))));

/** The range moved the least that shows bar `i` with `margin` bars to spare. */
export function showBar(r: Range, i: number, margin = 2): Range {
  if (i < r.from + margin) return { from: i - margin, to: r.to - (r.from - (i - margin)) };
  if (i > r.to - margin) return { from: r.from + (i + margin - r.to), to: i + margin };
  return r;
}

/** Zoom about the right-hand end (the newest bars stay put): factor < 1 zooms in. */
export function zoomRange(r: Range, factor: number, n: number): Range {
  const width = Math.max(MIN_BARS, Math.min(Math.max(MIN_BARS, n + RIGHT_PAD * 2), (r.to - r.from) * factor));
  return { from: r.to - width, to: r.to };
}

/** Slide the range by `bars`, never past the first bar or more than RIGHT_PAD past the last. */
export function scrollRange(r: Range, bars: number, n: number): Range {
  const width = r.to - r.from;
  let from = r.from + bars;
  from = Math.max(-RIGHT_PAD, Math.min(from, n - 1 + RIGHT_PAD - width));
  return { from, to: from + width };
}

/** What the readout under the chart says for one bar. */
export function readout(b: Candle, prev: Candle | null) {
  const ref = prev?.close ?? b.open;
  return { time: b.time, open: b.open, high: b.high, low: b.low, close: b.close, volume: b.volume ?? 0, changePct: ref > 0 ? ((b.close - ref) / ref) * 100 : 0 };
}

/** Bars per second to move for a stick deflection: slow near the centre, quick at the edge. */
export function stickRate(v: number, maxBarsPerSec: number) {
  const a = Math.abs(v);
  return Math.sign(v) * a * a * maxBarsPerSec;
}
