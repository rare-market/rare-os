// Store pieces in the kit's shapes: the logo-first store tile used by Sui Games,
// a price line, and a small cached loader for API reads.

import { useEffect, useState } from "react";
import { Dot } from "../ui";
import { GameLogo } from "../screens/SuiGames";
import { useApp } from "../lib/store";
import { onSale, platformText, priceNow, usdText } from "./logic";
import { useStore } from "./state";
import type { StoreGame } from "./types";

const cache = new Map<string, { at: number; data: unknown }>();

/** Reads once per key and keeps the answer for a minute, so B back to the tab is instant. */
export function useLoad<T>(key: string | null, load: () => Promise<T>, ttl = 60_000): { data: T | undefined; error: boolean; loading: boolean } {
  const hit = key ? (cache.get(key) as { at: number; data: T } | undefined) : undefined;
  const [state, setState] = useState<{ key: string | null; data: T | undefined; error: boolean; loading: boolean }>({ key, data: hit?.data, error: false, loading: false });
  useEffect(() => {
    if (!key) return;
    const h = cache.get(key) as { at: number; data: T } | undefined;
    if (h && Date.now() - h.at < ttl) {
      setState({ key, data: h.data, error: false, loading: false });
      return;
    }
    let live = true;
    setState((s) => ({ key, data: s.key === key ? s.data : h?.data, error: false, loading: true }));
    load().then(
      (data) => {
        cache.set(key, { at: Date.now(), data });
        if (live) setState({ key, data, error: false, loading: false });
      },
      () => live && setState((s) => ({ ...s, error: true, loading: false })),
    );
    return () => {
      live = false;
    };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  return state.key === key ? state : { data: hit?.data, error: false, loading: !!key };
}

export const logoOf = (g: StoreGame) => ({ name: g.name, logoUrl: g.images.logo ?? "" });

/** Where a link listing lives, for its tile and page. */
export function linkWhere(g: StoreGame): string {
  if (g.links.steam) return "On Steam";
  if (g.links.epic) return "On Epic Games";
  if (g.links.play) return "Plays in the browser";
  return "Website";
}

/** "$14.99", or the sale price with the old one struck through and muted. */
export function PriceText({ game, className = "" }: { game: StoreGame; className?: string }) {
  const now = priceNow(game);
  if (now == null) return null;
  return (
    <span className={`flex items-baseline gap-[0.45rem] whitespace-nowrap tabular-nums ${className}`}>
      <span className="font-medium text-fg">{usdText(now)}</span>
      {onSale(game) && <span className="text-muted line-through">{usdText(BigInt(game.price!.usd))}</span>}
    </span>
  );
}

const STATUS: Record<string, string> = { live: "Live", beta: "Beta" };

/** The logo-first store tile (Sui Games' card): logo, name, studio and genre; status and price below. */
export function StoreCard({ game, def, wide }: { game: StoreGame; def?: boolean; wide?: boolean }) {
  const owned = useStore((s) => s.licences.some((l) => l.slug === game.slug));
  const status = [game.status ? STATUS[game.status] : "", game.kind === "link" ? linkWhere(game) : platformText(game.platforms)].filter(Boolean).join(" · ");
  return (
    <button
      data-nav
      data-hint-a="Details"
      data-nav-default={def ? "" : undefined}
      onClick={() => useApp.getState().open({ kind: "storegame", slug: game.slug })}
      className={`tile flex h-[8rem] shrink-0 flex-col justify-between rounded-card border border-hair bg-l1 p-[1rem] text-left ${wide ? "tile-wide w-full" : "w-[20rem]"}`}
    >
      <span className="flex min-w-0 items-center gap-[1rem]">
        <GameLogo game={logoOf(game)} size="3.5rem" />
        <span className="min-w-0">
          <span className="block truncate text-row font-semibold text-fg">{game.name}</span>
          <span className="block truncate text-small text-muted">{[game.studio.name, game.genre].filter(Boolean).join(" · ")}</span>
        </span>
      </span>
      <span className="flex min-w-0 items-center justify-between gap-[0.75rem] text-meta text-muted">
        <span className="flex min-w-0 items-center gap-[0.45rem]">
          {game.status === "live" && <Dot tone="primary" />}
          <span className="truncate">{status}</span>
        </span>
        {owned ? <span className="whitespace-nowrap">In your library</span> : <PriceText game={game} />}
      </span>
    </button>
  );
}
