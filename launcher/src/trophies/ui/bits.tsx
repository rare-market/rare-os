// Small trophy pieces built from the ui kit: tier as muted text with a dot (no pills), trophy art,
// the completion line, and the game art card used in the grid and on the profile shelf.

import { Art } from "../../components/Art";
import { useApp } from "../../lib/store";
import { Dot, Thumb } from "../../ui";
import { TIER_NAME, type Tier } from "../types";
import type { GameSummary } from "../model";

const cn = (...c: Array<string | false | null | undefined>) => c.filter(Boolean).join(" ");

/** "Gold" or "Gold · 90 points", muted, with a small muted dot. Platinum's dot is white. */
export function TierLabel({ tier, points, className = "" }: { tier: Tier; points?: number | null; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-[0.4rem] whitespace-nowrap text-meta text-muted", className)}>
      <Dot tone={tier === 3 ? "primary" : "muted"} />
      {TIER_NAME[tier]}
      {points != null && <span className="tabular-nums">· {points} points</span>}
    </span>
  );
}

/** Trophy art: colour when earned, grey when locked, a plain covered square when hidden. */
export function TrophyArt({ src, gray, masked, label, size = "2.75rem", className = "" }: { src: string | null; gray?: string | null; masked?: boolean; label: string; size?: string; className?: string }) {
  const box = cn("relative shrink-0 overflow-hidden rounded-chip bg-l3", className);
  if (masked)
    return (
      <span className={cn(box, "flex items-center justify-center text-body font-semibold text-muted")} style={{ width: size, height: size }}>
        ?
      </span>
    );
  const locked = gray !== undefined;
  return (
    <span className={box} style={{ width: size, height: size }}>
      <Thumb src={locked ? gray || src : src} label={label} className={cn("absolute inset-0 h-full w-full", locked && !gray && "opacity-40 grayscale")} />
    </span>
  );
}

/** Completion as a thin line (white on layer 04: progress, not money, so no green). */
export function ProgressLine({ pct, className = "" }: { pct: number; className?: string }) {
  return (
    <span className={cn("relative block h-[3px] w-full overflow-hidden rounded-full bg-l4", className)}>
      <span className="absolute inset-y-0 left-0 rounded-full bg-fg/75" style={{ width: `${Math.max(0, Math.min(100, pct))}%` }} />
    </span>
  );
}

/** Header art for a Steam game: the library's local art first, then Steam's CDN. */
export function useSteamHeader(appid: number): { sources: string[]; title: string | null } {
  const g = useApp((s) => s.library?.games.find((x) => x.id === `steam:${appid}`) ?? null);
  const cdn = `https://cdn.cloudflare.steamstatic.com/steam/apps/${appid}/header.jpg`;
  return { sources: [...(g?.art.header ?? []).filter((p) => p !== cdn), cdn], title: g?.title ?? null };
}

export function GameArt({ appid, title, className = "" }: { appid: number; title: string; className?: string }) {
  const { sources } = useSteamHeader(appid);
  return (
    <span className={cn("relative block aspect-[460/215] w-full overflow-hidden bg-l3", className)}>
      <Art sources={sources} title={title} variant="header" className="absolute inset-0 h-full w-full object-cover" />
    </span>
  );
}

export const fmtPct = (p: number) => (p > 0 && p < 10 ? `${p.toFixed(1)}%` : `${Math.round(p)}%`);

export function fmtDay(ms: number): string {
  const d = new Date(ms);
  const same = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleDateString([], same ? { month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" });
}

/** A game in the trophy case grid or on the profile shelf. */
export function GameCard({
  appid,
  name,
  sum,
  note,
  onClick,
  def,
  className = "",
}: {
  appid: number;
  name: string;
  sum: GameSummary;
  note: string | null;
  onClick: () => void;
  def?: boolean;
  className?: string;
}) {
  return (
    <button data-nav data-hint-a="Trophies" data-nav-default={def ? "" : undefined} onClick={onClick} className={cn("tile tile-wide flex flex-col overflow-hidden rounded-card bg-l1 text-left", className)}>
      <GameArt appid={appid} title={name} />
      <span className="flex flex-col gap-[0.5rem] px-[0.9rem] pb-[0.8rem] pt-[0.7rem]">
        <span className="flex items-baseline gap-[0.75rem]">
          <span className="focus-text min-w-0 flex-1 truncate text-body font-semibold">{name}</span>
          <span className="tabular-nums text-small text-muted">{sum.pct}%</span>
        </span>
        <ProgressLine pct={sum.pct} />
        <span className="flex items-center gap-[0.75rem] text-meta text-muted">
          <span className="tabular-nums">
            {sum.unlocked} of {sum.total}
          </span>
          {note && <span className="ml-auto truncate">{note}</span>}
        </span>
      </span>
    </button>
  );
}
