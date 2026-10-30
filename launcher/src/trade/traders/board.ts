// Top traders from the ledger's board: a minimum of activity so one lucky trade does not rank,
// wallets the ledger flags as wash or bot-like left out, then sorted by realized P/L or win rate.
// Pure, unit tested.

import type { BoardExtras, BoardTag, LeaderRow } from "./api";

export type BoardSort = "pnl" | "winrate";

/** At least this many trades in the window... */
export const MIN_TRADES = 10;
/** ...and this much volume, in USD. */
export const MIN_VOLUME_USD = 1_000;

/**
 * Tags that mean the numbers are not real trading. The ledger tags bot-like wallets (very many
 * trades with almost no edge); a `wash` tag counts the same if the ledger adds one.
 */
const FLAGGED = new Set(["bot", "wash"]);

export interface TopTrader {
  address: string;
  row: LeaderRow;
  tags: BoardTag[];
  /** cumulative realized P/L by day over 30 days, when the board has it */
  curve: number[];
  /** wallets in this trader's funding-linked group, itself included; null when not linked */
  linked: number | null;
}

export const isFlagged = (tags: BoardTag[]) => tags.some((t) => FLAGGED.has(t.key));

/** Enough activity to count: trades and volume over the minimums. */
export const isActive = (r: Pick<LeaderRow, "trades" | "volumeUsd">, minTrades = MIN_TRADES, minVolume = MIN_VOLUME_USD) => r.trades >= minTrades && r.volumeUsd >= minVolume;

/** Funding-linked group sizes by wallet, from the team board (its best wallet stands for it). */
export function linkedSizes(teams: LeaderRow[] | undefined): Map<string, number> {
  const out = new Map<string, number>();
  for (const t of teams ?? []) if (t.team && t.team.members > 1) out.set(t.user.address.toLowerCase(), t.team.members);
  return out;
}

export function topTraders(
  rows: LeaderRow[],
  extras: BoardExtras | undefined,
  opts: { sort: BoardSort; teams?: Map<string, number>; minTrades?: number; minVolume?: number; limit?: number },
): TopTrader[] {
  const seen = new Set<string>();
  const out: TopTrader[] = [];
  for (const row of rows) {
    const address = row.user.address.toLowerCase();
    if (seen.has(address)) continue;
    seen.add(address);
    const x = extras?.rows[row.user.address] ?? extras?.rows[address];
    const tags = x?.tags ?? [];
    if (isFlagged(tags)) continue;
    if (!isActive(row, opts.minTrades, opts.minVolume)) continue;
    out.push({ address, row, tags, curve: x?.curve30d ?? [], linked: opts.teams?.get(address) ?? null });
  }
  if (opts.sort === "winrate") {
    // Win rate only means something for a trader who made money and has a known rate.
    return out
      .filter((t) => t.row.winRate != null && t.row.pnlUsd > 0)
      .sort((a, b) => (b.row.winRate ?? 0) - (a.row.winRate ?? 0) || b.row.pnlUsd - a.row.pnlUsd)
      .slice(0, opts.limit ?? 50);
  }
  return out.sort((a, b) => b.row.pnlUsd - a.row.pnlUsd).slice(0, opts.limit ?? 50);
}

/** Style tags worth a word in a row (the flagged ones never reach a row). */
export const styleWords = (tags: BoardTag[]) => tags.filter((t) => !FLAGGED.has(t.key)).map((t) => t.label);
