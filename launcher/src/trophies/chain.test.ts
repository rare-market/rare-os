import { bcs } from "@mysten/sui/bcs";
import { deriveDynamicFieldID, normalizeSuiAddress } from "@mysten/sui/utils";
import { describe, expect, it } from "vitest";
import { idOf, lookupGame, lookupProfile, opt, ownedTrophies, readGame, readRegistry, rarity, tableKeyId, type TrophyReader } from "./chain";
import { trophyConfig } from "./config";

const T = (n: number) => normalizeSuiAddress(`0x${n.toString(16)}`);

describe("tableKeyId", () => {
  it("derives the same ids as the SDK with explicit type tags", () => {
    expect(tableKeyId(T(1), "string", "steam:620")).toBe(deriveDynamicFieldID(T(1), { struct: { address: "0x1", module: "string", name: "String", typeParams: [] } }, bcs.string().serialize("steam:620").toBytes()));
    expect(tableKeyId(T(1), "u64", 7)).toBe(deriveDynamicFieldID(T(1), { u64: true }, bcs.u64().serialize(7n).toBytes()));
    expect(tableKeyId(T(1), "address", T(9))).toBe(deriveDynamicFieldID(T(1), { address: true }, bcs.Address.serialize(T(9)).toBytes()));
  });
  it("differs by key and by key type", () => {
    expect(tableKeyId(T(1), "string", "steam:620")).not.toBe(tableKeyId(T(1), "string", "steam:621"));
    expect(tableKeyId(T(1), "u64", 1)).not.toBe(tableKeyId(T(2), "u64", 1));
  });
});

describe("JSON helpers", () => {
  it("reads ids in every shape gRPC uses", () => {
    expect(idOf("0x2")).toBe(T(2));
    expect(idOf({ id: "0x2" })).toBe(T(2));
    expect(idOf({ id: { id: "0x2" } })).toBe(T(2));
    expect(idOf({ bytes: "0x2" })).toBe(T(2));
    expect(idOf(5)).toBe(null);
  });
  it("reads options", () => {
    expect(opt(null)).toBe(null);
    expect(opt("4")).toBe("4");
    expect(opt({ vec: ["4"] })).toBe("4");
    expect(opt({ vec: [] })).toBe(null);
    expect(opt(["4"])).toBe("4");
  });
});

// A fake chain with the shapes Sui gRPC returns.
function fakeChain() {
  const reg = T(0x100);
  const games = T(0x101);
  const profiles = T(0x102);
  const game = T(0x200);
  const defs = T(0x201);
  const profile = T(0x300);
  const player = T(0x999);
  const objects = new Map<string, Record<string, unknown>>([
    [reg, { id: reg, version: "1", paused: false, issuer_key: [1, 2, 3], games: { id: games, size: "1" }, profiles: { id: profiles, size: "1" } }],
    [tableKeyId(games, "string", "steam:620"), { id: T(1), name: "steam:620", value: game }],
    [tableKeyId(profiles, "address", player), { id: T(2), name: player, value: profile }],
    [
      game,
      { id: game, slug: "steam:620", name: "Portal 2", attested: true, paused: false, players: "40", platinum: "1", defs: { id: defs, size: "2" }, keys: { id: T(0x202), size: "2" } },
    ],
    [tableKeyId(defs, "u64", 0), { id: T(3), name: "0", value: { key: "ACH_WAKE_UP", name: "Wake Up Call", description: "d", image_url: "https://i", tier: 1, points: "30", hidden: false, in_platinum_set: true, unlocked: "10" } }],
    [tableKeyId(defs, "u64", 1), { id: T(4), name: "1", value: { key: "PLATINUM", name: "Platinum", description: "", image_url: "", tier: 3, points: "300", hidden: false, in_platinum_set: false, unlocked: "1" } }],
  ]);
  const reader: TrophyReader = {
    objects: async (ids) => ids.map((i) => objects.get(normalizeSuiAddress(i)) ?? null),
    owned: async (owner, type) =>
      owner === player && type === `${T(0xabc)}::trophy::Trophy`
        ? [{ objectId: T(0x500), json: { id: T(0x500), game, game_name: "Portal 2", trophy_id: "0", name: "Wake Up Call", description: "d", image_url: "https://i", tier: 1, tier_name: "Silver", points: "30", source: 1, source_name: "Self-reported", number: "10", unlocked_at_ms: "1700000000000" } }]
        : [],
  };
  return { reader, reg, game, profile, player };
}

describe("chain reads", () => {
  it("walks registry -> game -> defs and registry -> profile through derived field ids", async () => {
    const c = fakeChain();
    const r = await readRegistry(c.reader, c.reg);
    expect(r.issuerKeySet).toBe(true);
    expect(await lookupGame(c.reader, r, "steam:620")).toBe(c.game);
    expect(await lookupGame(c.reader, r, "steam:1")).toBe(null);
    expect(await lookupProfile(c.reader, r, c.player)).toBe(c.profile);
    const g = (await readGame(c.reader, c.game))!;
    expect(g).toMatchObject({ slug: "steam:620", players: 40, platinum: 1, attested: true });
    expect(g.defs.map((d) => [d.trophyId, d.key, d.tier, d.points, d.unlocked])).toEqual([
      [0, "ACH_WAKE_UP", 1, 30, 10],
      [1, "PLATINUM", 3, 300, 1],
    ]);
    expect(rarity(g.defs[0], g)).toBe(25);
    expect(rarity(g.defs[0], { players: 0 })).toBe(null);
  });

  it("reads owned trophies", async () => {
    const c = fakeChain();
    const t = await ownedTrophies(c.reader, T(0xabc), c.player);
    expect(t).toEqual([
      { objectId: T(0x500), game: c.game, gameName: "Portal 2", trophyId: 0, name: "Wake Up Call", description: "d", imageUrl: "https://i", tier: 1, points: 30, source: 1, number: 10, unlockedAtMs: 1_700_000_000_000 },
    ]);
  });
});

describe("trophyConfig", () => {
  it("is off until a deployment is filled in, mainnet first", () => {
    expect(trophyConfig({ mainnet: { package: "", registry: "" }, testnet: { package: "", registry: "" } })).toBe(null);
    expect(trophyConfig({ mainnet: { package: "", registry: "" }, testnet: { package: "0x1", registry: "0x2" } })).toEqual({ network: "testnet", package: "0x1", registry: "0x2" });
    expect(trophyConfig({ mainnet: { package: "0x3", registry: "0x4" }, testnet: { package: "0x1", registry: "0x2" } })?.network).toBe("mainnet");
  });
  it("ships the mainnet deployment", () => {
    expect(trophyConfig()?.network).toBe("mainnet");
  });
});
