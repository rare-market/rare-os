import { useState, type ReactNode } from "react";
import { useApp } from "../lib/store";
import type { Game } from "../lib/types";
import { SOURCE_LABEL } from "../lib/types";
import { Art } from "./Art";

// The shared kit lives in src/ui; these names stay importable from here for older screens.
export { Button, Rail, Empty } from "../ui/controls";
import { Dot } from "../ui/controls";
import { artSrc } from "../lib/state";

export function GameTile({ game, size = "md", onSelect, def, bare }: { game: Game; size?: "md" | "lg"; onSelect?: () => void; def?: boolean; /** Art only, no labels; the page names the focused game. */ bare?: boolean }) {
  const running = useApp((s) => s.running.includes(game.id));
  const w = size === "lg" ? "w-[10.5rem]" : "w-[9rem]";
  const app = game.source === "apps";
  return (
    <div className={`${w} shrink-0`}>
      <button
        data-nav
        data-title={game.title}
        data-game={game.id}
        data-hint-a="Details"
        data-nav-default={def ? "" : undefined}
        data-prefetch={game.art.hero[0] ? artSrc(game.art.hero[0], "hero") : undefined}
        className="tile relative block aspect-[2/3] w-full overflow-hidden rounded-card bg-l3 text-left"
        onClick={onSelect ?? (() => useApp.getState().open({ kind: "game", id: game.id }))}
      >
        {app && !game.art.cover.length ? (
          <AppArt game={game} />
        ) : (
          <Art sources={game.art.cover} title={game.title} variant="cover" className="absolute inset-0 h-full w-full object-cover" />
        )}
        {running && <Dot tone="primary" live className="absolute right-[0.6rem] top-[0.6rem] ring-2 ring-black/40" />}
        {game.foreign && <Dot tone="attention" className="absolute right-[0.5rem] top-[0.5rem]" />}
      </button>
      {!bare && (
        <>
          <div className="focus-text mt-[0.85rem] truncate px-[0.1rem] text-meta font-medium text-fg/85">{game.title}</div>
          <div className="truncate px-[0.1rem] text-label text-muted">{running ? "Playing" : SOURCE_LABEL[game.source]}</div>
        </>
      )}
    </div>
  );
}

/** An app or emulator without cover art: its icon on the layer colour, like rare.market's media tiles. */
export function AppArt({ game }: { game: Game }) {
  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-[0.9rem] bg-l2 p-[1rem]">
      <span className="flex h-[4.5rem] w-[4.5rem] items-center justify-center">
        {game.art.icon?.length ? (
          <Art sources={game.art.icon} title={game.title} variant="logo" className="max-h-full max-w-full object-contain" />
        ) : (
          <span className="text-[2.4rem] font-semibold text-fg/40">{game.title.slice(0, 1).toUpperCase()}</span>
        )}
      </span>
    </div>
  );
}

/** A bordered card on layer 01 (rare.market's card). */
export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`rounded-card border border-hair bg-l1 ${className}`}>{children}</div>;
}

export const fmtSize = (b?: number | null) => (!b ? "" : b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(b / 1e6))} MB`);

export function fmtAgo(t?: number | null) {
  if (!t) return "Never played";
  const s = Date.now() / 1000 - t;
  if (s < 3600) return "Played just now";
  if (s < 86400) return `Played ${Math.round(s / 3600)} h ago`;
  if (s < 86400 * 2) return "Played yesterday";
  if (s < 86400 * 30) return `Played ${Math.round(s / 86400)} days ago`;
  return `Played ${new Date(t * 1000).toLocaleDateString([], { month: "short", day: "numeric" })}`;
}

/** Remote image that falls back to a quiet tile with the name's initial when the host fails. */
export function SafeImg({ src, label, className = "" }: { src: string | null | undefined; label: string; className?: string }) {
  const [bad, setBad] = useState(false);
  if (!src || bad)
    return <div className={`${className} flex items-center justify-center bg-l3 text-[1.6rem] font-semibold text-fg/40`}>{label.slice(0, 1).toUpperCase()}</div>;
  return <img src={src} alt="" draggable={false} loading="lazy" onError={() => setBad(true)} className={`${className} object-cover`} />;
}
