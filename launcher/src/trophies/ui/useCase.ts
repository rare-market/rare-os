// The trophy case's data for a screen: loads Steam and chain data for the current account and
// joins them into per-game rows and summaries.

import { useEffect, useMemo } from "react";
import { useApp } from "../../lib/store";
import { useProfile } from "../../sui/profile";
import { caseSummary, gameRows, summarize, type GameSummary, type TrophyRow } from "../model";
import { claimBlocker, useTrophies } from "../store";
import { fmtDay } from "./bits";
import type { ChainGame, SteamGame } from "../types";

export interface CaseGame {
  appid: number;
  name: string;
  steam: SteamGame;
  chain: ChainGame | null;
  rows: TrophyRow[];
  sum: GameSummary;
  blocker: string | null;
}

export type GameSort = "recent" | "completion" | "name";

export function useCaseData(sort: GameSort = "recent") {
  const address = useProfile((s) => s.address);
  const t = useTrophies();
  const library = useApp((s) => s.library);

  useEffect(() => {
    void useProfile.getState().refresh();
    void useTrophies.getState().loadSteam();
  }, []);
  useEffect(() => {
    void useTrophies.getState().loadChain(address);
  }, [address, t.steam === null]); // eslint-disable-line react-hooks/exhaustive-deps

  const games = useMemo<CaseGame[]>(() => {
    const title = (appid: number) => library?.games.find((g) => g.id === `steam:${appid}`)?.title;
    const list = (t.steam ?? []).map((steam) => {
      const chain = t.games[steam.appid] ?? null;
      const rows = gameRows(steam, chain, t.minted);
      return { appid: steam.appid, name: title(steam.appid) || steam.name || chain?.name || `Steam app ${steam.appid}`, steam, chain, rows, sum: summarize(rows), blocker: claimBlocker(t, steam.appid) };
    });
    const by: Record<GameSort, (a: CaseGame, b: CaseGame) => number> = {
      recent: (a, b) => (b.sum.lastAt ?? 0) - (a.sum.lastAt ?? 0) || b.sum.unlocked - a.sum.unlocked,
      completion: (a, b) => b.sum.pct - a.sum.pct || b.sum.unlocked - a.sum.unlocked,
      name: (a, b) => a.name.localeCompare(b.name),
    };
    return list.sort(by[sort]);
  }, [t.steam, t.games, t.minted, t.setup, t.address, t.registry, library, sort]);

  const summary = useMemo(() => caseSummary(t.steam ?? [], t.profile, t.minted), [t.steam, t.profile, t.minted]);
  return { games, summary, address, loading: t.steamLoading || t.chainLoading || t.steam === null, chainOn: !!t.setup, chainError: t.chainError };
}

/** The line under a game card: what to do next, else when it was last played. */
export function cardNote(g: CaseGame): string | null {
  if (g.sum.claimable && !g.blocker) return `${g.sum.claimable} to claim`;
  if (g.sum.minted) return `${g.sum.minted} minted`;
  if (g.sum.lastAt) return `Last on ${fmtDay(g.sum.lastAt)}`;
  return null;
}
