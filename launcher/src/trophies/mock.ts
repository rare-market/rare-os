// Browser-only stand-ins for UI development and screenshots (npm run dev:ui). Never used inside
// the app. mock-steam.json holds six real games' public achievement lists (names, icons, global
// percentages from Steam's community pages); unlock state is made up here.
//
// ?trophychain also fakes a deployed rare_trophies package: a TrophyReader that serves the same
// JSON shapes Sui gRPC returns, at the same derived dynamic-field ids, so chain.ts runs for real
// against it. Its claims mint into this page's memory and never touch a network.

import { normalizeSuiAddress } from "@mysten/sui/utils";
import data from "./mock-steam.json";
import { steamSlug, tableKeyId, type TrophyReader } from "./chain";
import type { TrophyConfig } from "./config";
import type { SteamGame, Tier } from "./types";

const CDN = "https://shared.fastly.steamstatic.com/community_assets/images/apps/";
type Raw = { appid: number; name: string; achievements: Array<{ apiName: string; name: string; description: string; hidden: boolean; icon: string; iconGray: string; globalPercent: number | null }> };

/** Unlock rule per game: achievements at least this common are unlocked, and how long ago the last one was. */
const PLAN: Record<number, { min: number; daysAgo: number }> = {
  367520: { min: 25, daysAgo: 0.1 },
  504230: { min: 30, daysAgo: 2 },
  2379780: { min: 40, daysAgo: 1 },
  1145350: { min: 45, daysAgo: 0.4 },
  1794680: { min: 60, daysAgo: 12 },
  1868140: { min: 101, daysAgo: 30 },
};

const STEAM_ID = "76561197972611406";

export function mockSteam(): SteamGame[] {
  const now = Math.floor(Date.now() / 1000);
  return (data as Raw[]).map((g) => {
    const plan = PLAN[g.appid] ?? { min: 50, daysAgo: 5 };
    let t = now - Math.round(plan.daysAgo * 86400);
    const achievements = g.achievements.map((a) => {
      const unlocked = (a.globalPercent ?? 0) >= plan.min;
      const unlockTime = unlocked ? (t -= 3 * 3600 + (a.apiName.length % 7) * 3600 * 9) : null;
      return { apiName: a.apiName, name: a.name, description: a.description, iconUrl: CDN + a.icon, iconGrayUrl: CDN + a.iconGray, hidden: a.hidden, unlocked, unlockTime, globalPercent: a.globalPercent };
    });
    const times = achievements.flatMap((a) => (a.unlockTime ? [a.unlockTime] : []));
    return {
      appid: g.appid,
      name: g.name,
      steamId: STEAM_ID,
      achievements,
      total: achievements.length,
      unlocked: achievements.filter((a) => a.unlocked).length,
      lastUnlock: times.length ? Math.max(...times) : null,
      source: "stats" as const,
    };
  });
}

// ---------- fake chain (?trophychain) ----------

/** Deterministic fake object id. */
function fakeId(label: string): string {
  let out = "";
  let h = 2166136261;
  for (let round = 0; out.length < 64; round++) {
    for (const c of `${label}#${round}`) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0;
    out += h.toString(16).padStart(8, "0");
  }
  return normalizeSuiAddress(`0x${out.slice(0, 64)}`);
}

export const MOCK_CONFIG: TrophyConfig = { network: "testnet", package: fakeId("package"), registry: fakeId("registry") };

const tierFor = (pct: number | null): Tier => (pct == null || pct >= 35 ? 0 : pct >= 12 ? 1 : 2);
const POINTS = [15, 30, 90, 300];
/** Games registered on the fake chain (the rest show "not on Rare Trophies yet"), and how many unlocks are already minted. */
const REGISTERED: Record<number, { players: number; minted: number }> = { 367520: { players: 1240, minted: 6 }, 504230: { players: 512, minted: 3 }, 2379780: { players: 2210, minted: 2 }, 1145350: { players: 860, minted: 0 } };

type Obj = Record<string, unknown>;
interface MockChain {
  objects: Map<string, Obj>;
  owned: Map<string, Array<{ objectId: string; json: Obj }>>;
  profiles: Map<string, Obj>;
  seq: number;
}
let chain: MockChain | null = null;
let seededFor: string | null = null;

function defOf(c: MockChain, gameId: string, trophyId: number): Obj {
  const g = c.objects.get(gameId)!;
  const defsTable = (g.defs as Obj).id as string;
  return c.objects.get(tableKeyId(defsTable, "u64", trophyId))!.value as Obj;
}

function build(): MockChain {
  const c: MockChain = { objects: new Map(), owned: new Map(), profiles: new Map(), seq: 0 };
  const gamesTable = fakeId("games-table");
  const profilesTable = fakeId("profiles-table");
  const steam = mockSteam();
  let gameCount = 0;
  for (const g of steam) {
    const reg = REGISTERED[g.appid];
    if (!reg) continue;
    gameCount++;
    const gameId = fakeId(`game-${g.appid}`);
    const defsTable = fakeId(`defs-${g.appid}`);
    const defs = g.achievements.map((a, i) => ({
      key: a.apiName,
      name: a.name,
      description: a.description,
      image_url: a.iconUrl,
      tier: tierFor(a.globalPercent),
      points: String(POINTS[tierFor(a.globalPercent)]),
      hidden: a.hidden,
      in_platinum_set: true,
      unlocked: String(Math.round((reg.players * (a.globalPercent ?? 10)) / 100 * 0.7)),
      _i: i,
    }));
    // Every game gets a platinum after its regular trophies.
    defs.push({ key: "PLATINUM", name: `${g.name} platinum`, description: "Unlock every other trophy.", image_url: "", tier: 3, points: "300", hidden: false, in_platinum_set: false, unlocked: String(Math.round(reg.players * 0.02)), _i: defs.length });
    for (const d of defs) {
      const { _i, ...value } = d;
      c.objects.set(tableKeyId(defsTable, "u64", _i), { id: fakeId(`def-${g.appid}-${_i}`), name: String(_i), value });
    }
    c.objects.set(gameId, {
      id: gameId,
      version: "1",
      slug: steamSlug(g.appid),
      name: g.name,
      platform: "steam",
      image_url: "",
      attested: true,
      paused: false,
      defs: { id: defsTable, size: String(defs.length) },
      keys: { id: fakeId(`keys-${g.appid}`), size: String(defs.length) },
      platinum: String(defs.length - 1),
      platinum_set: [],
      unlocks: "0",
      players: String(reg.players),
    });
    c.objects.set(tableKeyId(gamesTable, "string", steamSlug(g.appid)), { id: fakeId(`gamefield-${g.appid}`), name: steamSlug(g.appid), value: gameId });
  }
  c.objects.set(MOCK_CONFIG.registry, {
    id: MOCK_CONFIG.registry,
    version: "1",
    paused: false,
    issuer_key: Array.from({ length: 32 }, (_, i) => i),
    games: { id: gamesTable, size: String(gameCount) },
    profiles: { id: profilesTable, size: "0" },
  });
  return c;
}

/** The player's already-minted trophies, once per address. */
function seed(c: MockChain, player: string) {
  if (seededFor === player) return;
  seededFor = player;
  const steam = mockSteam();
  for (const g of steam) {
    const reg = REGISTERED[g.appid];
    if (!reg?.minted) continue;
    const ids = g.achievements.flatMap((a, i) => (a.unlocked ? [i] : [])).slice(0, reg.minted);
    mint(c, player, fakeId(`game-${g.appid}`), ids, ids.map((_, k) => (g.appid === 2379780 && k === 0 ? 0 : 1)));
  }
}

function mint(c: MockChain, player: string, gameId: string, trophyIds: number[], sources?: number[]) {
  const registry = c.objects.get(MOCK_CONFIG.registry)!;
  const profilesTable = (registry.profiles as Obj).id as string;
  const fieldId = tableKeyId(profilesTable, "address", player);
  let profileId = (c.objects.get(fieldId)?.value as string | undefined) ?? null;
  if (!profileId) {
    profileId = fakeId(`profile-${player}`);
    c.objects.set(fieldId, { id: fakeId(`profilefield-${player}`), name: player, value: profileId });
    c.objects.set(profileId, { id: profileId, player, points: "0", trophies: "0", bronze: "0", silver: "0", gold: "0", platinum: "0", games: "0", self_reported: "0", self_reported_points: "0", last_unlock_ms: "0" });
  }
  const p = c.objects.get(profileId)!;
  const g = c.objects.get(gameId)!;
  const list = c.owned.get(player) ?? [];
  const hadGame = list.some((o) => o.json.game === gameId);
  trophyIds.forEach((trophyId, k) => {
    const d = defOf(c, gameId, trophyId);
    if (list.some((o) => o.json.game === gameId && Number(o.json.trophy_id) === trophyId)) return;
    d.unlocked = String(Number(d.unlocked) + 1);
    const source = sources?.[k] ?? 1;
    const tier = Number(d.tier);
    const objectId = fakeId(`trophy-${player}-${gameId}-${trophyId}-${++c.seq}`);
    list.push({
      objectId,
      json: {
        id: objectId,
        game: gameId,
        game_name: g.name,
        trophy_id: String(trophyId),
        name: d.name,
        description: d.description,
        image_url: d.image_url,
        tier,
        tier_name: ["Bronze", "Silver", "Gold", "Platinum"][tier],
        points: d.points,
        source,
        source_name: source === 0 ? "Studio issued" : "Self-reported",
        number: d.unlocked,
        unlocked_at_ms: String(Date.now() - (trophyIds.length - k) * 3600_000 * 20),
      },
    });
    const add = (k2: string, n: number) => (p[k2] = String(Number(p[k2]) + n));
    add("points", Number(d.points));
    add("trophies", 1);
    add(["bronze", "silver", "gold", "platinum"][tier], 1);
    if (source === 1) {
      add("self_reported", 1);
      add("self_reported_points", Number(d.points));
    }
  });
  if (!hadGame && list.some((o) => o.json.game === gameId)) p.games = String(Number(p.games) + 1);
  c.owned.set(player, list);
}

function ensure(player?: string): MockChain {
  chain ??= build();
  if (player) seed(chain, normalizeSuiAddress(player));
  return chain;
}

export const mockReader: TrophyReader = {
  objects: async (ids) => {
    await new Promise((r) => setTimeout(r, 120));
    const c = ensure();
    return ids.map((id) => structuredClone(c.objects.get(normalizeSuiAddress(id)) ?? null));
  },
  owned: async (owner) => {
    await new Promise((r) => setTimeout(r, 120));
    return structuredClone(ensure(owner).owned.get(normalizeSuiAddress(owner)) ?? []);
  },
};

/** Seeds the fake chain for `player` (their earlier claims) before the first read. */
export const mockSeed = (player: string) => void ensure(player);

/** The attestation service, faked: answers for registered games and unlocked achievements. */
export async function mockAttest(body: unknown): Promise<{ status: number; body: unknown }> {
  await new Promise((r) => setTimeout(r, 300));
  const b = body as { player: string; game: string; achievement: string };
  const c = ensure();
  const appid = Number(b.game.replace("steam:", ""));
  const gameId = fakeId(`game-${appid}`);
  const g = c.objects.get(gameId);
  if (!g) return { status: 404, body: { error: "This game is not on Rare Trophies yet" } };
  const defsTable = (g.defs as Obj).id as string;
  const size = Number((g.defs as Obj).size);
  for (let i = 0; i < size; i++) {
    const d = c.objects.get(tableKeyId(defsTable, "u64", i))!.value as Obj;
    if (d.key === b.achievement)
      return { status: 200, body: { registry: MOCK_CONFIG.registry, game: gameId, trophyId: String(i), nonce: String(Date.now() + i), expiresMs: String(Date.now() + 600_000), signature: btoa(String.fromCharCode(...new Uint8Array(64).fill(7))) } };
  }
  return { status: 404, body: { error: "No trophy for this achievement" } };
}

/** The fake chain's "transaction": mints what the claim asked for. */
export async function mockSubmit(player: string, gameId: string, trophyIds: number[]): Promise<{ digest: string }> {
  await new Promise((r) => setTimeout(r, 900));
  mint(ensure(player), normalizeSuiAddress(player), gameId, trophyIds);
  return { digest: fakeId(`digest-${Date.now()}`).slice(0, 46) };
}
