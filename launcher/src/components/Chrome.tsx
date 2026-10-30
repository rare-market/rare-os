// The shell around every screen: top bar (brand, tabs, account, device status), the bottom bar
// (rare.market's status bar on the left, controller hints on the right), toasts and the loading bar.

import { useEffect, useState } from "react";
import { TABS, useApp } from "../lib/store";
import { useLive } from "../lib/live";
import { activity, msToNextMinute } from "../lib/idle";
import type { PadButton } from "../lib/types";
import { IconBattery, IconWifi } from "./icons";
import { useFocusedHints, useHintStack, type Hint } from "../nav/hints";
import { Glyph, Hint as HintItem, LoadBar as Bar, Tabs, Dot } from "../ui/controls";
import { Num } from "../ui/Num";
import { SuiLogo } from "../ui/sui";
import { GasCell, ProfileChip } from "../sui/ui/bits";
import { appByRunId } from "../apps/catalog";
import { streamTitle, useStream } from "../stream/state";
import { NoticeToasts } from "../notify/Toast";
import { MissedCount, UnreadCount } from "../notify/Centre";
import { TrophyPopups } from "../trophies/ui/TrophyPopups";

export { Glyph };

export function BrandMark({ size = "1.7rem" }: { size?: string }) {
  return <img src="/brand/mark.webp" alt="" draggable={false} style={{ height: size, width: "auto" }} />;
}

/** Wakes once a minute, on the minute (exact, and a quarter of the old 15 s ticks); none while away. */
export function Clock() {
  const [t, setT] = useState(() => new Date());
  useEffect(() => {
    let id: ReturnType<typeof setTimeout> | null = null;
    const plan = () => {
      if (id) clearTimeout(id);
      id = null;
      if (activity.get() === "away") return;
      setT(new Date());
      id = setTimeout(plan, msToNextMinute(Date.now()));
    };
    plan();
    const unsub = activity.subscribe((a, prev) => {
      if (a === "away" || prev === "away") plan();
    });
    return () => {
      unsub();
      if (id) clearTimeout(id);
    };
  }, []);
  return <span className="whitespace-nowrap tabular-nums text-body font-medium text-fg">{t.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</span>;
}

export function TopBar() {
  const tab = useApp((s) => s.tab);
  const device = useApp((s) => s.device);
  return (
    <header className="relative z-20 flex h-[4.25rem] shrink-0 items-center gap-[1.5rem] px-[2.5rem]">
      <div className="flex items-center gap-[0.6rem]">
        <BrandMark />
        <span className="text-[1.35rem] font-semibold tracking-[-0.02em]">rare</span>
      </div>
      <nav className="ml-[0.5rem] flex h-full items-center gap-[1rem]">
        <Glyph b="lb" size="sm" />
        <Tabs className="h-full" items={TABS.map((t) => ({ value: t.id, label: t.label }))} value={tab} />
        <Glyph b="rb" size="sm" />
      </nav>
      <div className="ml-auto flex items-center gap-[1.25rem] text-fg/85">
        <ProfileChip />
        <span className="flex items-center gap-[0.9rem]">
          {device && device.wifi === null ? <span className="text-small text-muted">Offline</span> : <IconWifi size="1.15rem" />}
          {device?.battery && (
            <span className="flex items-center gap-[0.35rem]">
              <IconBattery size="1.35rem" percent={device.battery.percent} charging={device.battery.charging} />
              <span className="tabular-nums text-small">{device.battery.percent}%</span>
            </span>
          )}
        </span>
        <Clock />
      </div>
    </header>
  );
}

export type { Hint } from "../nav/hints";

const ORDER: PadButton[] = ["a", "b", "x", "y", "lbrb", "ltrt", "lt", "rt", "select", "start", "quick", "rare", "exithold"];

/** rare.market's status bar: SUI price, network throughput, what is running. Each cell has a hairline after it. */
function Status() {
  const suiUsd = useLive((s) => s.suiUsd);
  const change = useLive((s) => s.suiChange24h);
  const tps = useLive((s) => s.tps);
  const degraded = useLive((s) => s.degraded);
  const running = useApp((s) => s.running);
  const library = useApp((s) => s.library);
  const game = running.length ? library?.games.find((g) => g.id === running[0]) : null;
  const app = running.length ? appByRunId(running[0]) : undefined;
  // Re-render when PC games are listed, so a stream shows its game's name.
  useStream((s) => s.apps);
  const streamed = running.length ? streamTitle(running[0]) : null;
  const cell = "flex h-full items-center gap-[0.5rem] whitespace-nowrap border-r border-hair px-[1rem]";
  return (
    <div className="flex h-full items-stretch text-meta">
      <span className={`${cell} pl-[2.5rem]`}>
        <SuiLogo size="0.95rem" />
        <span className="font-medium text-fg">SUI</span>
        <Num kind="price" value={suiUsd} live className="text-fg" />
        <Num kind="change" value={change} className="text-label" />
      </span>
      {tps != null && (
        <span className={`${cell} text-muted`}>
          TPS
          <span className={`tabular-nums ${degraded ? "text-attention" : "text-fg"}`}>{tps.toLocaleString("en-US")}</span>
        </span>
      )}
      <GasCell />
      {running.length > 0 && (
        <span className={`${cell} text-fg`}>
          <Dot tone="brand" live />
          {app ? `${app.name} open` : game ? `Playing ${game.title}` : streamed ? `Streaming ${streamed}` : "Game running"}
          <MissedCount />
        </span>
      )}
    </div>
  );
}

/**
 * The bottom bar. A comes from the focused element, B shows whenever B goes somewhere,
 * X and Y come from the focused element or the topmost screen, the rest from `base`.
 * `override` (the Marketplace's own page hints) replaces everything.
 */
export function HintBar({ override, base, canBack }: { override: Hint[] | null; base: Hint[]; canBack: boolean }) {
  const stack = useHintStack((s) => s.stack);
  const f = useFocusedHints();
  let hints: Hint[];
  // Screens bring their own list; it is shown in the same order on every screen (A B X Y, Tabs, Sections, Quick settings).
  if (override) hints = [...override, ...base.filter((h) => (h.b === "quick" || h.b === "start" || h.b === "y") && !override.some((o) => o.b === h.b || (h.b !== "y" && (o.b === "quick" || o.b === "start" || o.b === "qam"))))].sort((p, q) => (ORDER.indexOf(p.b) + 100) % 100 - (ORDER.indexOf(q.b) + 100) % 100);
  else {
    const by = new Map<PadButton, string>();
    for (const h of base) by.set(h.b, h.label);
    for (const e of stack) for (const h of e.hints) by.set(h.b, h.label);
    if (f.x) by.set("x", f.x);
    if (f.y) by.set("y", f.y);
    if (f.menu && !by.has("start")) by.set("start", f.menu);
    if (f.focused) by.set("a", f.a ?? by.get("a") ?? "Select");
    if (canBack && !by.has("b")) by.set("b", "Back");
    hints = ORDER.filter((b) => by.has(b) && by.get(b)).map((b) => ({ b, label: by.get(b)! }));
  }
  return (
    <footer className="relative z-40 flex h-[3rem] shrink-0 items-stretch border-t border-hair bg-bg text-small text-fg/80">
      <Status />
      <div className={`ml-auto flex items-center pr-[2.5rem] ${hints.length > 6 ? "gap-[1rem]" : "gap-[1.4rem]"}`}>
        {hints.map((h) => (
          <HintItem key={h.b + h.label} b={h.b}>
            {h.label}
            {(h.b === "start" || h.b === "rare") && <UnreadCount />}
          </HintItem>
        ))}
      </div>
    </footer>
  );
}

export function Toasts() {
  const toasts = useApp((s) => s.toasts);
  return (
    <div className="pointer-events-none fixed left-1/2 top-[5rem] z-50 flex -translate-x-1/2 flex-col items-center gap-[0.5rem]">
      <NoticeToasts />
      <TrophyPopups />
      {toasts.map((t) => (
        <div key={t.id} className="pop flex items-center gap-[0.6rem] rounded-card border border-border-light bg-l3 px-[1.1rem] py-[0.6rem] text-small text-fg shadow-xl shadow-black/30">
          <Dot tone={t.tone === "error" ? "negative" : "brand"} />
          {t.text}
        </div>
      ))}
    </div>
  );
}

/** Thin loading bar under the top bar (no skeletons). */
export function LoadBar({ on }: { on: boolean }) {
  return <Bar on={on} className="top-[4.25rem]" />;
}
