// The rows every settings section is built from. Settings.tsx re-exports SettingRow and
// SwitchRow, so other screens keep importing them from "../Settings".
//
// A row can carry an `id`: it ends up as data-setting-id, which is how search jumps to it
// (see index.ts, one entry per id).

import { useEffect, useRef, type ReactNode } from "react";
import { feedback } from "../../nav/feedback";
import { Toggle } from "../../ui";

/** One settings row: title, a muted line, a control on the right. */
export function SettingRow({ id, title, body, children }: { id?: string; title: ReactNode; body?: ReactNode; children?: ReactNode }) {
  return (
    <div data-setting-id={id} className="flex min-h-[4.25rem] items-center gap-[1.25rem] border-t border-hair px-[1.4rem] py-[0.85rem] first:border-t-0">
      <div className="min-w-0 flex-1">
        <div className="text-body font-semibold">{title}</div>
        {body && <div className="text-small text-muted">{body}</div>}
      </div>
      {children}
    </div>
  );
}

/** A row that is itself the switch: A flips it. */
export function SwitchRow({ id, title, body, on, onChange }: { id?: string; title: string; body: string; on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      data-nav
      data-setting-id={id}
      data-hint-a={on ? "Turn off" : "Turn on"}
      onClick={() => {
        feedback("toggle");
        onChange(!on);
      }}
      className="flex min-h-[4.25rem] w-full items-center gap-[1.25rem] border-t border-hair px-[1.4rem] py-[0.85rem] text-left first:border-t-0"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-body font-semibold">{title}</span>
        <span className="block text-small text-muted">{body}</span>
      </span>
      <Toggle on={on} />
    </button>
  );
}

/** A row with a few choices side by side; the picked one is filled. One focus stop per choice. */
export function ChoiceRow<T extends string | number>({ id, title, body, value, choices, onPick }: { id?: string; title: string; body?: ReactNode; value: T | null; choices: Array<{ value: T; label: string }>; onPick: (v: T) => void }) {
  return (
    <SettingRow id={id} title={title} body={body}>
      <div className="flex shrink-0 items-center gap-[0.5rem]" data-nav-group={id ? `choice-${id}` : undefined}>
        {choices.map((c) => (
          <button
            key={String(c.value)}
            data-nav
            data-hint-a="Choose"
            onClick={() => {
              if (c.value === value) return;
              feedback("toggle");
              onPick(c.value);
            }}
            className={`btn h-[2.25rem] min-w-[3.3rem] rounded-chip px-[0.8rem] text-small tabular-nums font-medium ${c.value === value ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
          >
            {c.label}
          </button>
        ))}
      </div>
    </SettingRow>
  );
}

/**
 * A slider row. Focus it and press left or right (Settings turns those into a "nudge" event,
 * the same protocol as Quick settings' sliders).
 */
export function SliderRow({ id, title, body, value, min, max, step, suffix = "%", onChange }: { id?: string; title: string; body?: ReactNode; value: number; min: number; max: number; step: number; suffix?: string; onChange: (v: number) => void }) {
  const pct = ((value - min) / Math.max(1, max - min)) * 100;
  const ref = useRef<HTMLDivElement>(null);
  const live = useRef({ value, onChange });
  live.current = { value, onChange };
  useEffect(() => {
    const el = ref.current;
    const h = (e: Event) => {
      const v = Math.min(max, Math.max(min, live.current.value + (e as CustomEvent<number>).detail * step));
      if (v !== live.current.value) live.current.onChange(v);
    };
    el?.addEventListener("nudge", h);
    return () => el?.removeEventListener("nudge", h);
  }, [min, max, step]);
  return (
    <div ref={ref} data-nav data-slider data-setting-id={id} data-hint-a="" tabIndex={0} className="flex min-h-[4.25rem] items-center gap-[1.25rem] border-t border-hair px-[1.4rem] py-[0.85rem] first:border-t-0">
      <div className="min-w-0 flex-1">
        <div className="text-body font-semibold">{title}</div>
        {body && <div className="text-small text-muted">{body}</div>}
      </div>
      <div
        className="relative h-[0.35rem] w-[14rem] shrink-0 rounded-full bg-l4"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          const v = Math.round((min + ((e.clientX - r.left) / r.width) * (max - min)) / step) * step;
          onChange(Math.min(max, Math.max(min, v)));
        }}
      >
        <div className="absolute inset-y-0 left-0 rounded-full bg-fg" style={{ width: `${pct}%` }} />
        <div className="absolute top-1/2 h-[1.05rem] w-[1.05rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-fg" style={{ left: `${pct}%` }} />
      </div>
      <span className="num w-[3.5rem] shrink-0 text-right text-body font-medium tabular-nums">
        {Math.round(value)}
        {suffix}
      </span>
    </div>
  );
}
