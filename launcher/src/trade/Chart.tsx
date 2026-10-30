// Candles with volume in its own pane under them: the Trade API's CandleChart setup (lightweight-charts,
// quiet grid, dotted last-price line, flat ranges padded open, one-weight time labels), read in the
// launcher's colour tokens. The chart is to look at; the pad never needs to focus it.

import {
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  HistogramSeries,
  LineSeries,
  LineStyle,
  createChart,
  type AutoscaleInfo,
  type IChartApi,
  type ISeriesApi,
  type Time,
  type UTCTimestamp,
} from "lightweight-charts";
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import type { Candle } from "./api";
import { count } from "./format";
import { INDICATOR_COLORS, clampIndex, indicatorLine, scrollRange, showBar, zoomRange, type ChartType, type IndicatorKey } from "./chartmode";

const HAIRLINE = "rgba(203,208,235,0.08)";
const GRID = "rgba(203,208,235,0.045)";
const OPEN_BARS = 150;
const RIGHT_BARS = 4;

function token(name: string, fallback: string) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v ? `rgb(${v.split(/\s+/).join(",")})` : fallback;
}
const alpha = (rgb: string, a: number) => rgb.replace(/^rgb\((.*)\)$/, `rgba($1,${a})`);

/** A flat range (one candle, or no trades in view) opened up around its price. */
export function padFlatRange(minValue: number, maxValue: number, pad = 0.01) {
  const mid = (minValue + maxValue) / 2;
  const need = Math.max(Math.abs(mid) * pad * 2, 1e-12 * 100);
  if (maxValue - minValue >= need / 1e4) return { minValue, maxValue };
  return { minValue: mid - need / 2, maxValue: mid + need / 2 };
}

/** the Trade API's time axis labels: year, month, day, then hh:mm. */
export function timeLabel(unixSeconds: number, kind: number): string {
  const d = new Date(unixSeconds * 1000);
  if (kind === 0) return String(d.getFullYear());
  if (kind === 1) return d.toLocaleDateString("en-US", { month: "short" });
  if (kind === 2) return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** Bars by time, one per time, no gaps in the open price. */
export function cleanCandles(rows: Candle[]): Candle[] {
  const by = new Map<number, Candle>();
  for (const r of rows) if (Number.isFinite(r.time) && Number.isFinite(r.close) && r.close > 0) by.set(r.time, r);
  const out = [...by.values()].sort((a, b) => a.time - b.time);
  for (let i = 1; i < out.length; i++) if (!(out[i].open > 0)) out[i] = { ...out[i], open: out[i - 1].close };
  return out;
}

/** What chart mode drives: slide the time axis, zoom it. */
export interface ChartHandle {
  scrollBy(bars: number): void;
  zoom(factor: number): void;
}

interface ChartProps {
  rows: Candle[];
  format: (n: number) => string;
  fitKey: string;
  className?: string;
  type?: ChartType;
  volume?: boolean;
  indicators?: IndicatorKey[];
  /** Index into the cleaned bars of the bar the crosshair sits on; null hides the crosshair. */
  cursor?: number | null;
}

export const Chart = forwardRef<ChartHandle, ChartProps>(function Chart({ rows, format, fitKey, className, type = "candles", volume: showVolume = true, indicators = [], cursor = null }, ref) {
  const box = useRef<HTMLDivElement>(null);
  const line = useRef<ISeriesApi<"Line"> | null>(null);
  const lines = useRef(new Map<IndicatorKey, ISeriesApi<"Line">>());
  const barsRef = useRef<Candle[]>([]);
  const sizeFn = useRef<() => void>(() => {});
  const volOn = useRef(showVolume);
  volOn.current = showVolume;
  const chart = useRef<IChartApi | null>(null);
  const candles = useRef<ISeriesApi<"Candlestick"> | null>(null);
  const volume = useRef<ISeriesApi<"Histogram"> | null>(null);
  const fmt = useRef(format);
  fmt.current = format;
  const fitted = useRef("");
  const colors = useRef({ up: "", down: "" });

  useEffect(() => {
    if (!box.current) return;
    const up = token("--text-positive", "#2adf72");
    const down = token("--text-negative", "#ea4141");
    const dim = token("--text-secondary", "#857f94");
    const raised = token("--layer-04", "#231e2f");
    colors.current = { up, down };
    const c = createChart(box.current, {
      autoSize: true,
      layout: {
        background: { type: ColorType.Solid, color: "transparent" },
        textColor: dim,
        fontSize: 11,
        fontFamily: getComputedStyle(box.current).fontFamily,
        attributionLogo: false,
        panes: { enableResize: false, separatorColor: HAIRLINE, separatorHoverColor: "transparent" },
      },
      grid: { vertLines: { visible: false }, horzLines: { color: GRID } },
      crosshair: {
        mode: CrosshairMode.Normal,
        vertLine: { color: alpha(dim, 0.35), labelBackgroundColor: raised },
        horzLine: { color: alpha(dim, 0.35), labelBackgroundColor: raised },
      },
      rightPriceScale: { borderVisible: false, scaleMargins: { top: 0.08, bottom: 0.06 } },
      timeScale: {
        borderColor: HAIRLINE,
        timeVisible: true,
        secondsVisible: false,
        allowBoldLabels: false,
        rightOffset: RIGHT_BARS,
        tickMarkFormatter: (time: Time, type: number) => (typeof time === "number" ? timeLabel(time, type) : null),
      },
      handleScroll: false,
      handleScale: false,
    });
    candles.current = c.addSeries(CandlestickSeries, {
      upColor: up,
      downColor: down,
      borderVisible: false,
      wickUpColor: up,
      wickDownColor: down,
      lastValueVisible: true,
      priceLineVisible: true,
      priceLineColor: alpha(dim, 0.7),
      priceLineStyle: LineStyle.Dotted,
      priceLineWidth: 1,
      priceFormat: { type: "custom", formatter: (p: number) => fmt.current(p), minMove: 1e-12 },
      autoscaleInfoProvider: (base: () => AutoscaleInfo | null) => {
        const info = base();
        if (!info?.priceRange) return info;
        return { ...info, priceRange: padFlatRange(info.priceRange.minValue, info.priceRange.maxValue) };
      },
    });
    line.current = c.addSeries(LineSeries, {
      color: up,
      lineWidth: 2,
      visible: false,
      lastValueVisible: true,
      priceLineVisible: true,
      priceLineColor: alpha(dim, 0.7),
      priceLineStyle: LineStyle.Dotted,
      priceFormat: { type: "custom", formatter: (p: number) => fmt.current(p), minMove: 1e-12 },
    });
    volume.current = c.addSeries(
      HistogramSeries,
      { priceFormat: { type: "custom", formatter: (v: number) => (v > 0 ? count(v) : ""), minMove: 0.01 }, lastValueVisible: false, priceLineVisible: false },
      1,
    );
    c.priceScale("right", 1).applyOptions({ borderVisible: false, scaleMargins: { top: 0.2, bottom: 0 } });
    const size = () => {
      const h = box.current?.clientHeight ?? 0;
      const [main, vol] = c.panes();
      if (!main || !vol || h <= 0) return;
      const v = volOn.current ? Math.round(Math.min(96, Math.max(48, h * 0.18))) : 1;
      main.setStretchFactor(h - v);
      vol.setStretchFactor(v);
    };
    size();
    sizeFn.current = size;
    const ro = new ResizeObserver(size);
    ro.observe(box.current);
    chart.current = c;
    return () => {
      ro.disconnect();
      c.remove();
      chart.current = null;
      candles.current = null;
      volume.current = null;
      line.current = null;
      lines.current.clear();
    };
  }, []);

  useEffect(() => {
    const c = chart.current;
    if (!c || !candles.current || !volume.current) return;
    const bars = cleanCandles(rows);
    const { up, down } = colors.current;
    barsRef.current = bars;
    line.current?.setData(bars.map((b) => ({ time: b.time as UTCTimestamp, value: b.close })));
    candles.current.setData(bars.map((b) => ({ time: b.time as UTCTimestamp, open: b.open, high: b.high, low: b.low, close: b.close })));
    volume.current.setData(bars.map((b) => ({ time: b.time as UTCTimestamp, value: b.volume ?? 0, color: alpha(b.close >= b.open ? up : down, 0.45) })));
    if (bars.length && fitted.current !== fitKey) {
      fitted.current = fitKey;
      const n = bars.length;
      c.timeScale().setVisibleLogicalRange({ from: Math.max(-RIGHT_BARS, n - OPEN_BARS), to: n - 1 + RIGHT_BARS });
    }
  }, [rows, fitKey]);

  // Chart type and the volume pane.
  useEffect(() => {
    candles.current?.applyOptions({ visible: type === "candles" });
    line.current?.applyOptions({ visible: type === "line" });
  }, [type]);
  useEffect(() => {
    volume.current?.applyOptions({ visible: showVolume });
    sizeFn.current();
  }, [showVolume]);

  // Indicator lines (the Trade API's MA 20, MA 50, VWAP).
  useEffect(() => {
    const c = chart.current;
    if (!c) return;
    for (const [k, s] of lines.current) {
      if (!indicators.includes(k)) {
        c.removeSeries(s);
        lines.current.delete(k);
      }
    }
    for (const k of indicators) {
      let s = lines.current.get(k);
      if (!s) {
        s = c.addSeries(LineSeries, { color: INDICATOR_COLORS[k], lineWidth: 1, lastValueVisible: false, priceLineVisible: false, crosshairMarkerVisible: false });
        lines.current.set(k, s);
      }
      s.setData(indicatorLine(k, barsRef.current).map((p) => ({ time: p.time as UTCTimestamp, value: p.value })));
    }
  }, [indicators.join(","), rows]); // eslint-disable-line react-hooks/exhaustive-deps

  // The crosshair on the cursor bar, scrolled into view.
  useEffect(() => {
    const c = chart.current;
    const bars = barsRef.current;
    const series = type === "line" ? line.current : candles.current;
    if (!c || !series) return;
    if (cursor == null || !bars.length) {
      c.clearCrosshairPosition();
      return;
    }
    const i = clampIndex(cursor, bars.length);
    const r = c.timeScale().getVisibleLogicalRange();
    if (r) {
      const next = showBar(r, i);
      if (next !== r) c.timeScale().setVisibleLogicalRange(next);
    }
    c.setCrosshairPosition(bars[i].close, bars[i].time as UTCTimestamp, series);
  }, [cursor, type, rows]);

  useImperativeHandle(
    ref,
    () => ({
      scrollBy(n) {
        const c = chart.current;
        const r = c?.timeScale().getVisibleLogicalRange();
        if (c && r) c.timeScale().setVisibleLogicalRange(scrollRange(r, n, barsRef.current.length));
      },
      zoom(f) {
        const c = chart.current;
        const r = c?.timeScale().getVisibleLogicalRange();
        if (c && r) c.timeScale().setVisibleLogicalRange(zoomRange(r, f, barsRef.current.length));
      },
    }),
    [],
  );

  return <div ref={box} className={className} />;
});
