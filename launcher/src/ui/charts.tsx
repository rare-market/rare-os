// Small line charts:
// a table sparkline with no fill, and a card curve with a soft area ending in a dot. Plain SVG, drawn
// once per data change; nothing animates. Green when the line ends above where it started, red below:
// these lines are money.

import { useEffect, useId, useRef, useState } from "react";

export type LinePoint = { t: number; v: number };

type Geometry = { xy: { x: number; y: number }[]; line: string; area: string };

const r1 = (n: number) => Math.round(n * 10) / 10;

/** Points placed by time across the width and by value down the height. null with fewer than two points. */
export function lineGeometry(points: LinePoint[], box: { width: number; height: number; padY?: number }): Geometry | null {
  if (points.length < 2 || box.width <= 0 || box.height <= 0) return null;
  const padY = box.padY ?? 4;
  const t0 = points[0].t;
  const t1 = points[points.length - 1].t;
  let lo = Math.min(...points.map((p) => p.v));
  let hi = Math.max(...points.map((p) => p.v));
  if (hi === lo) {
    hi += 1;
    lo -= 1;
  }
  const plotH = Math.max(1, box.height - padY * 2);
  const x = (t: number) => (t1 === t0 ? box.width : ((t - t0) / (t1 - t0)) * box.width);
  const y = (v: number) => padY + ((hi - v) / (hi - lo)) * plotH;
  const xy = points.map((p) => ({ x: r1(x(p.t)), y: r1(y(p.v)) }));
  const line = xy.map((p, i) => `${i ? "L" : "M"}${p.x} ${p.y}`).join(" ");
  const floor = r1(padY + plotH);
  const area = `${line} L${xy[xy.length - 1].x} ${floor} L${xy[0].x} ${floor} Z`;
  return { xy, line, area };
}

/** Direction of a series: the last point against the first. */
export const rising = (points: LinePoint[]) => (points[points.length - 1]?.v ?? 0) >= (points[0]?.v ?? 0);

const UP = "rgb(var(--text-positive))";
const DOWN = "rgb(var(--text-negative))";

/** The size of a box in CSS pixels, kept current as it resizes. Charts are drawn in real pixels so strokes stay crisp at 1280 and 1920. */
function useSize<E extends HTMLElement>() {
  const ref = useRef<E>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setSize((s) => (s.w === el.clientWidth && s.h === el.clientHeight ? s : { w: el.clientWidth, h: el.clientHeight }));
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, size] as const;
}

/** A tiny line with no fill, for a row. Size it with className (rem). */
export function Spark({ points, className = "h-[1.5rem] w-[5.5rem]" }: { points: LinePoint[]; className?: string }) {
  const [ref, { w, h }] = useSize<HTMLSpanElement>();
  const g = lineGeometry(points, { width: w, height: h, padY: 2 });
  return (
    <span ref={ref} className={`block shrink-0 ${className}`} aria-hidden>
      {g && (
        <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} className="block overflow-visible">
          <path d={g.line} fill="none" stroke={rising(points) ? UP : DOWN} strokeWidth={Math.max(1.5, h / 16)} strokeLinejoin="round" strokeLinecap="round" />
        </svg>
      )}
    </span>
  );
}

/** A small filled line ending in a dot, for a card. */
export function MiniCurve({ points, className = "h-[4rem] w-full", tone }: { points: LinePoint[]; className?: string; tone?: "up" | "down" }) {
  const id = useId();
  const [ref, { w, h }] = useSize<HTMLSpanElement>();
  const g = lineGeometry(points, { width: w, height: h, padY: Math.max(3, h / 16) });
  const color = (tone ?? (rising(points) ? "up" : "down")) === "up" ? UP : DOWN;
  const end = g?.xy[g.xy.length - 1];
  const stroke = Math.max(1.75, h / 36);
  return (
    <span ref={ref} className={`block ${className}`} aria-hidden>
      {g && end && (
        <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} className="block overflow-visible">
          <defs>
            <linearGradient id={id} x1="0" x2="0" y1="0" y2="1">
              <stop offset="0" stopColor={color} stopOpacity={0.22} />
              <stop offset="1" stopColor={color} stopOpacity={0} />
            </linearGradient>
          </defs>
          <path d={g.area} fill={`url(#${id})`} />
          <path d={g.line} fill="none" stroke={color} strokeWidth={stroke} strokeLinejoin="round" strokeLinecap="round" />
          <circle cx={end.x} cy={end.y} r={stroke * 1.7} fill={color} />
        </svg>
      )}
    </span>
  );
}
