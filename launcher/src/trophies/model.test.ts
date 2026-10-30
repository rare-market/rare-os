import { describe, expect, it } from "vitest";
import { caseSummary, claimableKeys, gameRows, sortRows, summarize } from "./model";
import type { ChainGame, ChainTrophy, SteamGame } from "./types";

const ach = (apiName: string, unlocked: boolean, o: Partial<SteamGame["achievements"][number]> = {}) => ({
  apiName,
  name: apiName,
  description: `${apiName} text`,
  iconUrl: `https://i/${apiName}.jpg`,
  iconGrayUrl: null,
  hidden: false,
  unlocked,
  unlockTime: unlocked ? 1000 : null,
  globalPercent: 50,
  ...o,
});
const steam: SteamGame = {
  appid: 1,
  name: "G",
  steamId: "1",
  total: 4,
  unlocked: 2,
  lastUnlock: 2000,
  source: "stats",
  achievements: [ach("A", true), ach("B", true, { unlockTime: 2000 }), ach("C", false, { globalPercent: 80 }), ach("D", false, { hidden: true, globalPercent: 90 })],
};
const def = (trophyId: number, key: string, tier: 0 | 1 | 2 | 3, unlocked = 5) => ({ trophyId, key, name: key, description: "", imageUrl: "", tier, points: [15, 30, 90, 300][tier], hidden: false, inPlatinumSet: true, unlocked });
const chain: ChainGame = { id: "0xg", slug: "steam:1", name: "G", attested: true, paused: false, players: 20, platinum: 4, defs: [def(0, "A", 0), def(1, "B", 2), def(2, "C", 1), def(3, "D", 0), def(4, "PLAT", 3, 1)] };
const minted: ChainTrophy[] = [{ objectId: "0xt", game: "0xg", gameName: "G", trophyId: 0, name: "A", description: "", imageUrl: "", tier: 0, points: 15, source: 1, number: 3, unlockedAtMs: 5000 }];

describe("gameRows", () => {
  it("is Steam only without a chain: unlocked and locked, hidden locked masked", () => {
    const rows = gameRows(steam, null, []);
    expect(rows.map((r) => [r.key, r.state, r.masked])).toEqual([
      ["A", "unlocked", false],
      ["B", "unlocked", false],
      ["C", "locked", false],
      ["D", "locked", true],
    ]);
    expect(rows[0]).toMatchObject({ tier: null, rarity: 50, rarityFrom: "steam", at: 1_000_000 });
    expect(claimableKeys(rows)).toEqual([]);
  });

  it("joins chain defs and minted trophies, rarity from Rare players, platinum appended", () => {
    const rows = gameRows(steam, chain, minted);
    expect(rows.map((r) => [r.key, r.state, r.tier])).toEqual([
      ["A", "minted", 0],
      ["B", "unlocked", 2],
      ["C", "locked", 1],
      ["D", "locked", 0],
      ["PLAT", "locked", 3],
    ]);
    expect(rows[0]).toMatchObject({ rarity: 25, rarityFrom: "rare", source: 1, at: 5000 });
    expect(claimableKeys(rows)).toEqual(["B"]);
    const s = summarize(rows);
    expect(s).toMatchObject({ total: 5, unlocked: 2, minted: 1, claimable: 1, unminted: 1, points: 15, pct: 40 });
  });

  it("ignores trophies minted in other games", () => {
    const rows = gameRows(steam, chain, [{ ...minted[0], game: "0xother" }]);
    expect(rows[0].state).toBe("unlocked");
  });
});

describe("sortRows", () => {
  it("minted, then newest unlocks, then common locked, masked last", () => {
    const rows = gameRows(steam, chain, minted);
    expect(sortRows(rows).map((r) => r.key)).toEqual(["A", "B", "C", "PLAT", "D"]);
    expect(sortRows(rows, "locked").map((r) => r.key)).toEqual(["C", "PLAT", "D"]);
    expect(sortRows(rows, "unlocked").map((r) => r.key)).toEqual(["A", "B"]);
  });
});

describe("caseSummary", () => {
  it("uses the chain profile when there is one", () => {
    const s = caseSummary([steam], { id: "0xp", points: 300, trophies: 4, bronze: 2, silver: 1, gold: 1, platinum: 0, games: 1, selfReported: 3, selfReportedPoints: 150 }, minted);
    expect(s).toMatchObject({ points: 300, trophies: 4, selfReportedPct: 50, steamUnlocked: 2, steamTotal: 4, games: 1 });
    expect(s.tiers).toEqual({ 0: 2, 1: 1, 2: 1, 3: 0 });
  });
  it("has no points before any trophy", () => {
    const s = caseSummary([steam], null, []);
    expect(s.points).toBe(null);
    expect(s.selfReportedPct).toBe(null);
  });
});
