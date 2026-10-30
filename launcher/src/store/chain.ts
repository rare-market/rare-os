// Rare Store reads from the chain over gRPC (zero GraphQL): the Game object, the release for a
// licence's edition (Game.editions is a Table<u64, Release>), and licence ownership for the
// launch check when rare.market can't answer. Layouts follow rare-store sources/game.move and
// licence.move field for field.

import { bcs } from "@mysten/sui/bcs";
import { normalizeSuiAddress, toHex } from "@mysten/sui/utils";
import { isTauri } from "../lib/bridge";
import { grpcClient } from "../wallet/grpc";
import { ownedKiosks, type NftChainReader } from "../wallet/tx/nft/context";
import type { ChainGame, ChainRelease } from "./types";

const Build = bcs.struct("Build", { platform: bcs.string(), sha256: bcs.vector(bcs.u8()), size: bcs.u64(), path: bcs.string() });
const Release = bcs.struct("Release", { version: bcs.u64(), label: bcs.string(), edition: bcs.u64(), builds: bcs.vector(Build), published_at_ms: bcs.u64() });
const ResaleTerms = bcs.struct("ResaleTerms", { enabled: bcs.bool(), royalty_bps: bcs.u64(), min_hold_ms: bcs.u64(), min_royalty_mist: bcs.u64(), royalty_to: bcs.Address });
const Table = bcs.struct("Table", { id: bcs.Address, size: bcs.u64() });

/** `game::Game` up to the fields the launcher reads (BCS reads a prefix, the rest is ignored). */
export const GameBcs = bcs.struct("Game", {
  id: bcs.Address,
  version: bcs.u64(),
  studio: bcs.Address,
  slug: bcs.string(),
  name: bcs.string(),
  image_url: bcs.string(),
  metadata_url: bcs.string(),
  price_usd: bcs.u64(),
  sale_price_usd: bcs.u64(),
  sale_ends_ms: bcs.u64(),
  accept_sui: bcs.bool(),
  listed: bcs.bool(),
  blocked: bcs.bool(),
  launch_check: bcs.bool(),
  edition: bcs.u64(),
  latest: bcs.option(Release),
  editions: Table,
  resale: ResaleTerms,
});

export const LicenceBcs = bcs.struct("Licence", {
  id: bcs.Address,
  game: bcs.Address,
  slug: bcs.string(),
  name: bcs.string(),
  image_url: bcs.string(),
  edition: bcs.u64(),
  version: bcs.u64(),
});

type ReleaseJson = ReturnType<typeof Release.parse>;

export function toRelease(r: ReleaseJson): ChainRelease {
  return {
    version: Number(r.version),
    label: r.label,
    edition: Number(r.edition),
    publishedMs: Number(r.published_at_ms),
    builds: r.builds.map((b) => ({ platform: b.platform, sha256: toHex(Uint8Array.from(b.sha256)), size: BigInt(b.size), path: b.path })),
  };
}

export function parseGame(content: Uint8Array): ChainGame {
  const g = GameBcs.parse(content);
  return {
    id: normalizeSuiAddress(g.id),
    slug: g.slug,
    name: g.name,
    priceUsd: BigInt(g.price_usd),
    salePriceUsd: BigInt(g.sale_price_usd),
    saleEndsMs: Number(g.sale_ends_ms),
    acceptSui: g.accept_sui,
    listed: g.listed,
    blocked: g.blocked,
    launchCheck: g.launch_check,
    edition: Number(g.edition),
    latest: g.latest ? toRelease(g.latest) : null,
    editionsTable: normalizeSuiAddress(g.editions.id),
    resale: {
      enabled: g.resale.enabled,
      royaltyBps: Number(g.resale.royalty_bps),
      minHoldMs: Number(g.resale.min_hold_ms),
      minRoyaltyMist: BigInt(g.resale.min_royalty_mist),
      royaltyTo: normalizeSuiAddress(g.resale.royalty_to),
    },
  };
}

/** The slice of SuiGrpcClient.core the store reads with (stubbed in tests). */
export interface StoreChainReader {
  getObject(o: { objectId: string; include: { content: true } }): Promise<{ object: { content: Uint8Array; type: string } }>;
  getDynamicField(o: { parentId: string; name: { type: string; bcs: Uint8Array } }): Promise<{ dynamicField: { value: { bcs: Uint8Array } } }>;
}

const core = () => (grpcClient as unknown as { core: StoreChainReader & NftChainReader }).core;

export async function readGame(gameId: string, client: StoreChainReader = core()): Promise<ChainGame> {
  if (!isTauri && gameId.startsWith("0xde")) {
    const g = (await import("./mock")).chainGame(gameId);
    if (g) return g;
  }
  const r = await client.getObject({ objectId: gameId, include: { content: true } });
  return parseGame(r.object.content);
}

/**
 * The release a licence of `edition` may download: `Game.editions[edition]`, never `latest`.
 * Null when that edition has no release.
 */
export async function releaseFor(game: ChainGame, edition: number, client: StoreChainReader = core()): Promise<ChainRelease | null> {
  if (!isTauri && game.id.startsWith("0xde") && game.latest?.edition === edition) return game.latest;
  try {
    const r = await client.getDynamicField({ parentId: game.editionsTable, name: { type: "u64", bcs: bcs.u64().serialize(edition).toBytes() } });
    return toRelease(Release.parse(r.dynamicField.value.bcs));
  } catch (e) {
    if (/not ?found|does not exist/i.test((e as Error)?.message ?? "")) return null;
    throw e;
  }
}

/** The price a purchase pays at `now` (game::current_price_usd). */
export function currentPriceUsd(g: Pick<ChainGame, "priceUsd" | "salePriceUsd" | "saleEndsMs">, now = Date.now()): bigint {
  return g.salePriceUsd > 0n && now < g.saleEndsMs ? g.salePriceUsd : g.priceUsd;
}

/**
 * Whether `owner` holds a licence for the game on chain: an owned `Licence`, or a `ResaleLicence`
 * in a kiosk the owner holds the cap for. The launch check's fallback when rare.market is down.
 */
export async function ownsOnChain(owner: string, gameId: string, origin: string, client: NftChainReader = core()): Promise<boolean> {
  const want = normalizeSuiAddress(gameId);
  const holds = (content?: Uint8Array) => {
    if (!content) return false;
    try {
      return normalizeSuiAddress(LicenceBcs.parse(content).game) === want;
    } catch {
      return false;
    }
  };
  let cursor: string | null = null;
  for (let page = 0; page < 10; page++) {
    const r = await client.listOwnedObjects({ owner, type: `${origin}::licence::Licence`, cursor, limit: 50, include: { content: true } });
    if (r.objects.some((o) => holds(o.content))) return true;
    if (!r.hasNextPage || !r.cursor) break;
    cursor = r.cursor;
  }
  const resaleType = `${normalizeSuiAddress(origin)}::licence::ResaleLicence`;
  const { kiosks } = await ownedKiosks(client, owner);
  for (const k of kiosks) {
    let c: string | null = null;
    for (let page = 0; page < 10; page++) {
      const r = await client.listDynamicFields({ parentId: k.kioskId, cursor: c, limit: 50 });
      const ids = r.dynamicFields.filter((f) => f.valueType && normType(f.valueType) === resaleType && f.childId).map((f) => f.childId!);
      if (ids.length) {
        const objs = await client.getObjects({ objectIds: ids, include: { content: true } });
        if (objs.objects.some((o) => o && !(o instanceof Error) && holds(o.content))) return true;
      }
      if (!r.hasNextPage || !r.cursor) break;
      c = r.cursor;
    }
  }
  return false;
}

function normType(t: string): string {
  const [addr, ...rest] = t.split("::");
  return [normalizeSuiAddress(addr), ...rest].join("::");
}
