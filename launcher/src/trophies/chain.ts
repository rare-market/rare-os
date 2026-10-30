// Rare Trophies chain reads. Sui gRPC only (src/sui/chain.ts grpc()), zero GraphQL.
//
// Table lookups never page through a table: a `Table<K, V>` entry is a dynamic field of the
// table's UID, so its object id is derived from (table id, key type, BCS key) with
// deriveDynamicFieldID and read directly. The field object's JSON is `{ id, name, value }`.
//
//   Registry.games    Table<String, ID>   "steam:<appid>" -> Game id
//   Registry.profiles Table<address, ID>  player -> Profile id
//   Game.defs         Table<u64, TrophyDef> trophy id -> definition (ids are 0..size-1)

import { bcs } from "@mysten/sui/bcs";
import { deriveDynamicFieldID, normalizeSuiAddress } from "@mysten/sui/utils";
import type { Network } from "../sui/chain";
import { trophyType } from "./config";
import type { ChainDef, ChainGame, ChainProfile, ChainTrophy, Tier, TrophySource } from "./types";

type Json = Record<string, unknown>;

/** The reads the trophy case needs; tests pass fakes. */
export interface TrophyReader {
  /** JSON of each object, null where it does not exist */
  objects: (ids: string[]) => Promise<Array<Json | null>>;
  /** objects of `type` owned by `owner` */
  owned: (owner: string, type: string) => Promise<Array<{ objectId: string; json: Json | null }>>;
}

export function grpcReader(network: Network): TrophyReader {
  // The gRPC client loads on the first read, not with the launcher.
  const client = async () => (await import("../sui/chain")).grpc(network);
  return {
    objects: async (ids) => {
      const out: Array<Json | null> = [];
      for (let i = 0; i < ids.length; i += 50) {
        const r = await (await client()).core.getObjects({ objectIds: ids.slice(i, i + 50), include: { json: true } });
        for (const o of r.objects) out.push(o instanceof Error ? null : ((o.json as Json | null) ?? null));
      }
      return out;
    },
    owned: async (owner, type) => {
      const out: Array<{ objectId: string; json: Json | null }> = [];
      let cursor: string | null = null;
      do {
        const r: { objects: Array<{ objectId: string; json?: unknown }>; hasNextPage: boolean; cursor: string | null } = await (await client()).core.listOwnedObjects({ owner, type, limit: 50, cursor, include: { json: true } });
        for (const o of r.objects) out.push({ objectId: o.objectId, json: (o.json as Json | null) ?? null });
        cursor = r.hasNextPage ? r.cursor : null;
      } while (cursor && out.length < 2000);
      return out;
    },
  };
}

// ---------- JSON helpers (gRPC renders u64 as strings, IDs as strings or { id } / { bytes }) ----------

export function idOf(v: unknown): string | null {
  if (typeof v === "string" && /^0x[0-9a-fA-F]+$/.test(v)) return normalizeSuiAddress(v);
  if (v && typeof v === "object") {
    const o = v as Json;
    return idOf(o.id ?? o.bytes ?? null);
  }
  return null;
}

export function num(v: unknown): number {
  if (typeof v === "number") return v;
  if (typeof v === "string" && v.trim() !== "") return Number(v);
  return 0;
}

const str = (v: unknown) => (typeof v === "string" ? v : "");
const bool = (v: unknown) => v === true || v === "true";

/** Option<T>: null, the value itself, `{ vec: [v] }` or `[v]`. */
export function opt(v: unknown): unknown {
  if (v == null) return null;
  if (Array.isArray(v)) return v.length ? v[0] : null;
  if (typeof v === "object" && "vec" in (v as Json)) return opt((v as Json).vec);
  return v;
}

/** `{ id, size }` of a Table field. */
const tableId = (v: unknown) => idOf(v);

// ---------- dynamic field ids ----------

export type KeyKind = "string" | "address" | "u64";
const KEY_TYPE: Record<KeyKind, string> = { string: "0x1::string::String", address: "address", u64: "u64" };

export function tableKeyId(table: string, kind: KeyKind, key: string | number | bigint): string {
  const bytes =
    kind === "string" ? bcs.string().serialize(String(key)).toBytes() : kind === "address" ? bcs.Address.serialize(String(key)).toBytes() : bcs.u64().serialize(BigInt(key)).toBytes();
  return deriveDynamicFieldID(table, KEY_TYPE[kind], bytes);
}

// ---------- registry ----------

export interface RegistryInfo {
  id: string;
  paused: boolean;
  /** attested claims need an issuer key on the registry */
  issuerKeySet: boolean;
  gamesTable: string;
  profilesTable: string;
}

export async function readRegistry(read: TrophyReader, registry: string): Promise<RegistryInfo> {
  const [j] = await read.objects([registry]);
  if (!j) throw new Error("Rare Trophies registry not found");
  const games = tableId(j.games);
  const profiles = tableId(j.profiles);
  if (!games || !profiles) throw new Error("Unexpected registry shape");
  const key = j.issuer_key;
  return { id: normalizeSuiAddress(registry), paused: bool(j.paused), issuerKeySet: Array.isArray(key) ? key.length > 0 : typeof key === "string" && key.length > 0, gamesTable: games, profilesTable: profiles };
}

/** Value of one table entry (an ID), or null when the key is not in the table. */
async function tableIdValue(read: TrophyReader, table: string, kind: KeyKind, key: string): Promise<string | null> {
  const [field] = await read.objects([tableKeyId(table, kind, key)]);
  return field ? idOf(field.value) : null;
}

export const steamSlug = (appid: number) => `steam:${appid}`;

export const lookupGame = (read: TrophyReader, reg: RegistryInfo, slug: string) => tableIdValue(read, reg.gamesTable, "string", slug);
export const lookupProfile = (read: TrophyReader, reg: RegistryInfo, player: string) => tableIdValue(read, reg.profilesTable, "address", normalizeSuiAddress(player));

// ---------- game, profile, trophies ----------

export function parseDef(trophyId: number, j: Json): ChainDef {
  return {
    trophyId,
    key: str(j.key),
    name: str(j.name),
    description: str(j.description),
    imageUrl: str(j.image_url),
    tier: Math.min(3, Math.max(0, num(j.tier))) as Tier,
    points: num(j.points),
    hidden: bool(j.hidden),
    inPlatinumSet: bool(j.in_platinum_set),
    unlocked: num(j.unlocked),
  };
}

/** A game with every trophy definition (one batched read for the defs). */
export async function readGame(read: TrophyReader, gameId: string): Promise<ChainGame | null> {
  const [j] = await read.objects([gameId]);
  if (!j) return null;
  const defsTable = tableId(j.defs);
  const count = num((j.defs as Json | undefined)?.size);
  const ids = Array.from({ length: count }, (_, i) => i);
  const fields = defsTable && count ? await read.objects(ids.map((i) => tableKeyId(defsTable, "u64", i))) : [];
  const defs = fields.flatMap((f, i) => (f && f.value && typeof f.value === "object" ? [parseDef(i, f.value as Json)] : []));
  const plat = opt(j.platinum);
  return {
    id: normalizeSuiAddress(gameId),
    slug: str(j.slug),
    name: str(j.name),
    attested: bool(j.attested),
    paused: bool(j.paused),
    players: num(j.players),
    platinum: plat == null ? null : num(plat),
    defs,
  };
}

export function parseProfile(id: string, j: Json): ChainProfile {
  return {
    id: normalizeSuiAddress(id),
    points: num(j.points),
    trophies: num(j.trophies),
    bronze: num(j.bronze),
    silver: num(j.silver),
    gold: num(j.gold),
    platinum: num(j.platinum),
    games: num(j.games),
    selfReported: num(j.self_reported),
    selfReportedPoints: num(j.self_reported_points),
  };
}

export async function readProfile(read: TrophyReader, profileId: string): Promise<ChainProfile | null> {
  const [j] = await read.objects([profileId]);
  return j ? parseProfile(profileId, j) : null;
}

export function parseTrophy(objectId: string, j: Json): ChainTrophy | null {
  const game = idOf(j.game);
  if (!game) return null;
  return {
    objectId,
    game,
    gameName: str(j.game_name),
    trophyId: num(j.trophy_id),
    name: str(j.name),
    description: str(j.description),
    imageUrl: str(j.image_url),
    tier: Math.min(3, Math.max(0, num(j.tier))) as Tier,
    points: num(j.points),
    source: (num(j.source) === 0 ? 0 : 1) as TrophySource,
    number: num(j.number),
    unlockedAtMs: num(j.unlocked_at_ms),
  };
}

export async function ownedTrophies(read: TrophyReader, pkg: string, owner: string): Promise<ChainTrophy[]> {
  const list = await read.owned(owner, trophyType(pkg));
  return list.flatMap((o) => {
    const t = o.json ? parseTrophy(o.objectId, o.json) : null;
    return t ? [t] : [];
  });
}

/** Rarity on Rare: share of the game's players who unlocked this trophy. */
export function rarity(def: Pick<ChainDef, "unlocked">, game: Pick<ChainGame, "players">): number | null {
  return game.players > 0 ? Math.min(100, (def.unlocked / game.players) * 100) : null;
}
