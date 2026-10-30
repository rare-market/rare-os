// The chart as a focus target with a chart mode (A on it). In chart mode the pad drives the chart
// and nothing else: left/right moves the crosshair bar by bar (hold to speed up), up/down zooms,
// the right stick slides time (x) and zooms (y), LT/RT step the timeframe, Y flips candles and
// line, X shows or hides volume, A cycles MA 20 / MA 50 / VWAP, B leaves. The readout under the
// crosshair shows the bar's time, open, high, low, close, change and volume.

import { useEffect, useMemo, useRef, useState } from "react";
import { Glyph, Panel } from "../ui";
import type { Candle } from "./api";
import { Chart, cleanCandles, type ChartHandle } from "./Chart";
import { CHART_TYPES, INDICATOR_LABELS, INDICATOR_STEPS, clampIndex, readout, stickRate, type ChartType } from "./chartmode";
import { useStick, usePadHandler } from "../nav/input";
import { useTradeHints } from "./nav";
import { cn } from "./parts";
import { feedback } from "../nav/feedback";
import { useApp } from "../lib/store";
import type { Hint } from "../nav/hints";

const HINTS: Hint[] = [
  { b: "left", label: "Move" },
  { b: "up", label: "Zoom" },
  { b: "lt", label: "Shorter" },
  { b: "rt", label: "Longer" },
  { b: "y", label: "Candles or line" },
  { b: "x", label: "Volume" },
  { b: "a", label: "Indicators" },
  { b: "b", label: "Leave chart" },
];

export interface ChartAreaProps {
  rows: Candle[];
  format: (n: number) => string;
  fitKey: string;
  /** Timeframe ids in order, the current one, and a step (LT/RT in chart mode). */
  tfs: string[];
  tf: string;
  onTf: (tf: string) => void;
  className?: string;
}

const when = (t: number) => new Date(t * 1000).toLocaleString("en-US", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false });

export function ChartArea({ rows, format, fitKey, tfs, tf, onTf, className }: ChartAreaProps) {
  const [mode, setMode] = useState(false);
  const [type, setType] = useState<ChartType>("candles");
  const [volume, setVolume] = useState(true);
  const [ind, setInd] = useState(0);
  const [cursor, setCursor] = useState<number | null>(null);
  const chart = useRef<ChartHandle>(null);
  const bars = useMemo(() => cleanCandles(rows), [rows]);
  const n = bars.length;
  const held = useRef(0);

  // A new timeframe or coin: the cursor goes to the newest bar.
  useEffect(() => {
    if (mode) setCursor(Math.max(0, n - 1));
  }, [fitKey]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (mode && cursor != null && cursor > n - 1) setCursor(Math.max(0, n - 1));
  }, [n]); // eslint-disable-line react-hooks/exhaustive-deps

  const enter = () => {
    setMode(true);
    setCursor(Math.max(0, n - 1));
  };
  const leave = () => {
    setMode(false);
    setCursor(null);
  };

  // View turns chart mode on and off, on the chart that is on show (pages under the top one are inert).
  const box = useRef<HTMLDivElement>(null);
  usePadHandler((b, repeat) => {
    if (b !== "select" || repeat || useApp.getState().overlay) return false;
    const el = box.current;
    if (!el || el.closest("[inert]") || !el.offsetParent) return false;
    feedback("toggle");
    if (mode) leave();
    else enter();
    return true;
  });
  usePadHandler((b, repeat) => {
    if (b === "start" || b === "select" || b === "guide" || b === "qam") return false;
    switch (b) {
      case "left":
      case "right": {
        held.current = repeat ? held.current + 1 : 0;
        const step = held.current > 24 ? 6 : held.current > 10 ? 3 : 1;
        setCursor((c) => clampIndex((c ?? n - 1) + (b === "right" ? step : -step), n));
        break;
      }
      case "up":
        chart.current?.zoom(0.8);
        break;
      case "down":
        chart.current?.zoom(1.25);
        break;
      case "lt":
      case "rt": {
        const i = tfs.indexOf(tf) + (b === "rt" ? 1 : -1);
        if (i >= 0 && i < tfs.length) onTf(tfs[i]);
        break;
      }
      case "y":
        if (!repeat) setType((t) => CHART_TYPES[(CHART_TYPES.indexOf(t) + 1) % CHART_TYPES.length]);
        break;
      case "x":
        if (!repeat) setVolume((v) => !v);
        break;
      case "a":
        if (!repeat) setInd((i) => (i + 1) % INDICATOR_STEPS.length);
        break;
      case "b":
        leave();
        break;
    }
    return true;
  }, mode);

  // Right stick: x slides time, y zooms. Applied every frame while it is held; no frame loop
  // while it rests (the native side sends (0, 0) on release).
  const stick = useRef({ x: 0, y: 0 });
  const raf = useRef(0);
  const last = useRef(0);
  const tick = useRef<(now: number) => void>(() => {});
  tick.current = (now: number) => {
    const dt = Math.min(0.05, (now - last.current) / 1000);
    last.current = now;
    const { x, y } = stick.current;
    if (x) chart.current?.scrollBy(stickRate(x, 60) * dt);
    if (y) chart.current?.zoom(Math.pow(1.8, Math.sign(y) * y * y * dt * 3));
    raf.current = x || y ? requestAnimationFrame((t) => tick.current(t)) : 0;
  };
  useStick((x, y) => {
    stick.current = { x, y };
    if ((x || y) && !raf.current) {
      last.current = performance.now();
      raf.current = requestAnimationFrame((t) => tick.current(t));
    }
  }, mode);
  useEffect(() => {
    if (!mode) return;
    return () => {
      cancelAnimationFrame(raf.current);
      raf.current = 0;
      stick.current = { x: 0, y: 0 };
    };
  }, [mode]);

  // The hint bar shows chart mode's buttons while it is on.
  useEffect(() => {
    if (!mode) return;
    useTradeHints.setState({ sheet: HINTS });
    return () => useTradeHints.setState({ sheet: null });
  }, [mode]);

  const i = cursor == null ? null : clampIndex(cursor, n);
  const r = i != null && bars[i] ? readout(bars[i], bars[i - 1] ?? null) : null;
  const indicators = INDICATOR_STEPS[ind];

  return (
    <Panel className={cn("relative", mode && "ring-1 ring-border-light", className)}>
      <div
        ref={box}
        data-nav
        data-area="chart"
        data-hint-a="Control chart"
        tabIndex={0}
        role="button"
        aria-label="Chart"
        onClick={() => !mode && enter()}
        className="absolute inset-0 rounded-card"
      >
        <Chart ref={chart} rows={rows} format={format} fitKey={fitKey} className="size-full px-[0.5rem] pt-[1rem]" type={type} volume={volume} indicators={indicators} cursor={i} />
      </div>
      {r && (
        <div className="num pointer-events-none absolute left-[0.75rem] top-[0.4rem] z-[3] flex flex-wrap items-baseline gap-x-[0.9rem] text-label text-secondary">
          <span className="text-fg">{when(r.time)}</span>
          <span>O {format(r.open)}</span>
          <span>H {format(r.high)}</span>
          <span>L {format(r.low)}</span>
          <span>C {format(r.close)}</span>
          <span className={r.changePct >= 0 ? "text-positive" : "text-negative"}>{`${r.changePct >= 0 ? "+" : ""}${r.changePct.toFixed(2)}%`}</span>
          {volume && <span>Vol {r.volume ? Number(r.volume.toPrecision(3)).toLocaleString("en-US") : "0"}</span>}
        </div>
      )}
      {mode && (
        <div className="pointer-events-none absolute right-[4.5rem] top-[0.4rem] z-[3] flex items-center gap-[0.75rem] text-label text-secondary">
          {indicators.length > 0 && <span>{indicators.map((k) => INDICATOR_LABELS[k]).join(", ")}</span>}
          <span>{type === "candles" ? "Candles" : "Line"}</span>
          <span className="flex items-center gap-[0.35rem]">
            <Glyph b="b" size="sm" /> Leave
          </span>
        </div>
      )}
      {!mode && (
        <div className="pointer-events-none absolute right-[4.5rem] top-[0.5rem] z-[3] flex items-center gap-[0.35rem] text-label text-secondary opacity-0 [[data-area=chart]:focus_&]:opacity-100">
          <Glyph b="a" size="sm" /> Control chart
        </div>
      )}
    </Panel>
  );
}
