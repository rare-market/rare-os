// Pure view logic for the trophy case: joins local Steam achievements, the chain's trophy
// definitions and the player's minted trophies into rows and summaries. No I/O here.

import { rarity } from "./chain";
import type { ChainDef, ChainGame, ChainProfile, ChainTrophy, SteamGame, Tier, TrophySource } from "./types";

export type RowState = "minted" | "unlocked" | "locked";

export interface TrophyRow {
  /** Steam API name or the chain def's key */
  key: string;
  name: string;
  description: string;
  iconUrl: string | null;
  iconGrayUrl: string | null;
  /** locked hidden trophy: name, text and art stay covered */
  masked: boolean;
  state: RowState;
  /** unix ms of the unlock (local) or the mint */
  at: number | null;
  tier: Tier | null;
  points: number | null;
  /** share of Rare players with it (chain), else Steam's global share */
  rarity: number | null;
  rarityFrom: "rare" | "steam" | null;
  def: ChainDef | null;
  minted: ChainTrophy | null;
  source: TrophySource | null;
}

const ORDER: Record<RowState, number> = { minted: 0, unlocked: 1, locked: 2 };

/** Every trophy of one game: Steam's list first, then chain-only defs (the platinum). */
export function gameRows(steam: SteamGame | null, chain: ChainGame | null, minted: ChainTrophy[]): TrophyRow[] {
  const mine = chain ? minted.filter((t) => t.game === chain.id) : [];
  const byId = new Map(mine.map((t) => [t.trophyId, t]));
  const defs = new Map((chain?.defs ?? []).map((d) => [d.key, d]));
  const rows: TrophyRow[] = [];
  const seen = new Set<string>();
  for (const a of steam?.achievements ?? []) {
    const def = defs.get(a.apiName) ?? null;
    const m = def ? byId.get(def.trophyId) ?? null : null;
    const state: RowState = m ? "minted" : a.unlocked ? "unlocked" : "locked";
    const rare = def && chain ? rarity(def, chain) : null;
    seen.add(a.apiName);
    rows.push({
      key: a.apiName,
      name: a.name,
      description: a.description,
      iconUrl: a.iconUrl ?? def?.imageUrl ?? null,
      iconGrayUrl: a.iconGrayUrl,
      masked: a.hidden && state === "locked",
      state,
      at: m ? m.unlockedAtMs : a.unlockTime ? a.unlockTime * 1000 : null,
      tier: def ? def.tier : null,
      points: def ? def.points : null,
      rarity: rare ?? a.globalPercent,
      rarityFrom: rare != null ? "rare" : a.globalPercent != null ? "steam" : null,
      def,
      minted: m,
      source: m ? m.source : null,
    });
  }
  for (const def of chain?.defs ?? []) {
    if (seen.has(def.key)) continue;
    const m = byId.get(def.trophyId) ?? null;
    const rare = chain ? rarity(def, chain) : null;
    rows.push({
      key: def.key,
      name: def.name,
      description: def.description,
      iconUrl: def.imageUrl || null,
      iconGrayUrl: null,
      masked: def.hidden && !m,
      state: m ? "minted" : "locked",
      at: m ? m.unlockedAtMs : null,
      tier: def.tier,
      points: def.points,
      rarity: rare,
      rarityFrom: rare != null ? "rare" : null,
      def,
      minted: m,
      source: m ? m.source : null,
    });
  }
  return rows;
}

export type RowFilter = "all" | "unlocked" | "locked";

/** Minted first, then unlocked (newest first), then locked (most common first, masked last). */
export function sortRows(rows: TrophyRow[], filter: RowFilter = "all"): TrophyRow[] {
  return rows
    .filter((r) => filter === "all" || (filter === "locked" ? r.state === "locked" : r.state !== "locked"))
    .map((r, i) => ({ r, i }))
    .sort((x, y) => {
      const a = x.r;
      const b = y.r;
      if (ORDER[a.state] !== ORDER[b.state]) return ORDER[a.state] - ORDER[b.state];
      if (a.state !== "locked") return (b.at ?? 0) - (a.at ?? 0) || x.i - y.i;
      if (a.masked !== b.masked) return a.masked ? 1 : -1;
      return (b.rarity ?? -1) - (a.rarity ?? -1) || x.i - y.i;
    })
    .map((x) => x.r);
}

export interface GameSummary {
  total: number;
  unlocked: number;
  minted: number;
  /** unlocked locally, a trophy on chain, not minted yet */
  claimable: number;
  /** unlocked locally, not minted (claimable once the game is on chain) */
  unminted: number;
  points: number;
  pct: number;
  lastAt: number | null;
}

export function summarize(rows: TrophyRow[]): GameSummary {
  const total = rows.length;
  const unlocked = rows.filter((r) => r.state !== "locked").length;
  const minted = rows.filter((r) => r.state === "minted").length;
  const unminted = rows.filter((r) => r.state === "unlocked").length;
  const claimable = rows.filter((r) => r.state === "unlocked" && r.def && r.def.tier !== 3).length;
  const at = rows.flatMap((r) => (r.state !== "locked" && r.at ? [r.at] : []));
  return {
    total,
    unlocked,
    minted,
    claimable,
    unminted,
    points: rows.reduce((s, r) => s + (r.state === "minted" ? r.points ?? 0 : 0), 0),
    pct: total ? Math.round((unlocked / total) * 100) : 0,
    lastAt: at.length ? Math.max(...at) : null,
  };
}

/** Rows that Claim would send for a game (local unlocks with a chain def, not minted, not the platinum). */
export const claimableKeys = (rows: TrophyRow[]) => rows.filter((r) => r.state === "unlocked" && r.def && r.def.tier !== 3).map((r) => r.key);

export interface CaseSummary {
  /** from the chain Profile when there is one */
  points: number | null;
  trophies: number;
  tiers: Record<Tier, number>;
  /** share of the score that is self-reported, 0..100 */
  selfReportedPct: number | null;
  steamUnlocked: number;
  steamTotal: number;
  games: number;
}

export function caseSummary(steam: SteamGame[], profile: ChainProfile | null, minted: ChainTrophy[]): CaseSummary {
  const tiers: Record<Tier, number> = { 0: 0, 1: 0, 2: 0, 3: 0 };
  if (profile) {
    tiers[0] = profile.bronze;
    tiers[1] = profile.silver;
    tiers[2] = profile.gold;
    tiers[3] = profile.platinum;
  } else for (const t of minted) tiers[t.tier]++;
  const trophies = profile ? profile.trophies : minted.length;
  const points = profile ? profile.points : minted.length ? minted.reduce((s, t) => s + t.points, 0) : null;
  const selfPts = profile ? profile.selfReportedPoints : minted.filter((t) => t.source === 1).reduce((s, t) => s + t.points, 0);
  return {
    points,
    trophies,
    tiers,
    selfReportedPct: points ? Math.round((selfPts / points) * 100) : null,
    steamUnlocked: steam.reduce((s, g) => s + g.unlocked, 0),
    steamTotal: steam.reduce((s, g) => s + g.total, 0),
    games: steam.filter((g) => g.unlocked > 0).length,
  };
}
