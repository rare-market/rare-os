import { describe, expect, it } from "vitest";
import type { BoardExtras, LeaderRow } from "./api";
import { MIN_TRADES, isFlagged, linkedSizes, styleWords, topTraders } from "./board";

const addr = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;
const row = (n: number, o: Partial<LeaderRow> = {}): LeaderRow => ({
  rank: n,
  user: { address: addr(n), username: null, displayName: null, avatarUrl: null },
  pnlUsd: 1000 - n,
  volumeUsd: 50_000,
  trades: 40,
  winRate: 0.5,
  topTokens: [],
  tokenCount: 3,
  ...o,
});
const extras = (tags: Record<number, string[]>): BoardExtras => ({
  available: true,
  rows: Object.fromEntries(Object.entries(tags).map(([n, keys]) => [addr(Number(n)), { tags: keys.map((key) => ({ key, label: key, reason: "" })), curve30d: [0, 1, 2], copyPnlUsd: null }])),
});

describe("topTraders", () => {
  it("keeps only traders over the activity minimum", () => {
    const rows = [row(1), row(2, { trades: MIN_TRADES - 1 }), row(3, { volumeUsd: 500 }), row(4, { trades: MIN_TRADES })];
    expect(topTraders(rows, undefined, { sort: "pnl" }).map((t) => t.row.rank)).toEqual([1, 4]);
  });

  it("leaves out bot-like and wash-flagged wallets", () => {
    const rows = [row(1), row(2), row(3)];
    const out = topTraders(rows, extras({ 1: ["bot"], 2: ["wash"], 3: ["scalper"] }), { sort: "pnl" });
    expect(out.map((t) => t.row.rank)).toEqual([3]);
    expect(out[0].curve).toEqual([0, 1, 2]);
    expect(isFlagged([{ key: "bot", label: "", reason: "" }])).toBe(true);
    expect(styleWords(out[0].tags)).toEqual(["scalper"]);
  });

  it("sorts by realized P/L, or by win rate among profitable traders", () => {
    const rows = [row(1, { pnlUsd: 10, winRate: 0.9 }), row(2, { pnlUsd: 500, winRate: 0.4 }), row(3, { pnlUsd: -50, winRate: 1 }), row(4, { pnlUsd: 80, winRate: null }), row(5, { pnlUsd: 20, winRate: 0.9 })];
    expect(topTraders(rows, undefined, { sort: "pnl" }).map((t) => t.row.rank)).toEqual([2, 4, 5, 1, 3]);
    // losing (3) and unknown (4) win rates drop out; ties go to the bigger P/L
    expect(topTraders(rows, undefined, { sort: "winrate" }).map((t) => t.row.rank)).toEqual([5, 1, 2]);
  });

  it("marks funding-linked groups from the team board and drops repeated wallets", () => {
    const teams = linkedSizes([row(9, { team: { members: 3, entity: "e" } }), row(8, { team: { members: 1, entity: "f" } })]);
    expect(teams.get(addr(9))).toBe(3);
    expect(teams.has(addr(8))).toBe(false);
    const out = topTraders([row(9), row(9), row(1)], undefined, { sort: "pnl", teams });
    expect(out.map((t) => [t.row.rank, t.linked])).toEqual([
      [1, null],
      [9, 3],
    ]);
  });
});
