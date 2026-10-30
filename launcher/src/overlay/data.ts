// What the overlay shows for one game, all from rare.market's /v1 API (through native.rareGet)
// and Sui gRPC:
// - Items: the player's objects in the game's collections (wallet and kiosks), each with the
//   collection floor, its best offer and its last sale.
// - Market: the cheapest listings in those collections, to buy without leaving the game.
// - Trophies: other objects the player holds from the game's own Move packages (badges,
//   achievements, soulbound rewards). They're not in the collections, so they aren't for sale.
// - Notifications: the player's sales, purchases, listings and transfers in those collections,
//   plus offers they can fill right now.

import type { CollectionDetail, ItemCard, MarketEvent, Page } from "../marketplace/api";
import { fmtSui } from "../marketplace/format";
import { canonAddr, canonType, grpc } from "../sui/chain";
import { typeMatches } from "../sui/ownership";
import { packageOf, type OverlayGame } from "./game";

export type RareGet = <T>(path: string) => Promise<T>;
const enc = encodeURIComponent;

export interface OverlayItem extends ItemCard {
  /** The collection's floor (cheapest all-in listing), MIST. */
  floorMist: string | null;
}

export interface Floors {
  [nftType: string]: { slug: string | null; name: string; floorMist: string | null; topOfferMist: string | null };
}

/** The player's items in the game's collections, with each collection's floor. */
export async function loadItems(get: RareGet, address: string, game: OverlayGame): Promise<{ items: OverlayItem[]; floors: Floors }> {
  const floors: Floors = {};
  const per = await Promise.all(
    game.collections.map(async (c) => {
      const [page, detail] = await Promise.all([
        get<Page<ItemCard>>(`/v1/accounts/${enc(address)}/items?collection=${enc(c.nftType)}&limit=60`).catch(() => ({ data: [] as ItemCard[], nextCursor: null })),
        get<CollectionDetail>(`/v1/collections/${enc(c.nftType)}`).catch(() => null),
      ]);
      floors[c.nftType] = { slug: detail?.slug ?? null, name: detail?.name ?? c.name, floorMist: detail?.stats?.floorMist ?? null, topOfferMist: detail?.stats?.topOfferMist ?? null };
      return page.data.map((it) => ({ ...it, floorMist: detail?.stats?.floorMist ?? null }));
    }),
  );
  return { items: sortItems(per.flat()), floors };
}

const big = (v: string | null | undefined) => {
  try {
    return v ? BigInt(v) : 0n;
  } catch {
    return 0n;
  }
};

/** Most sellable first: items with an offer (highest first), then listed, then the rest. */
export function sortItems(items: OverlayItem[]): OverlayItem[] {
  const score = (x: OverlayItem) => (x.topOfferMist ? 2 : x.listing ? 1 : 0);
  return [...items].sort((a, b) => score(b) - score(a) || (big(b.topOfferMist) > big(a.topOfferMist) ? 1 : big(b.topOfferMist) < big(a.topOfferMist) ? -1 : 0) || a.name.localeCompare(b.name, undefined, { numeric: true }));
}

/** The cheapest listings in the game's collections (all-in price), a turn per collection. */
export async function loadMarket(get: RareGet, game: OverlayGame, floors: Floors, owner: string | null, limit = 12): Promise<ItemCard[]> {
  const me = owner ? canonAddr(owner) : null;
  const pages = await Promise.all(
    game.collections.map((c) => {
      const slug = floors[c.nftType]?.slug;
      if (!slug) return Promise.resolve([] as ItemCard[]);
      return get<Page<ItemCard>>(`/v1/collections/${enc(slug)}/items?sort=price_asc&limit=${limit}`)
        .then((p) => p.data)
        .catch(() => [] as ItemCard[]);
    }),
  );
  const price = (x: ItemCard) => big(x.listing?.allInMist ?? x.listing?.priceMist);
  const cmp = (a: ItemCard, b: ItemCard) => (price(a) < price(b) ? -1 : price(a) > price(b) ? 1 : 0);
  // Each collection's cheapest first, taken in turns, so one cheap collection doesn't fill the list.
  const lists = pages.map((p) => p.filter((x) => x.listing && !(me && x.listing.seller && canonAddr(x.listing.seller) === me)).sort(cmp));
  const out: ItemCard[] = [];
  for (let i = 0; out.length < limit && lists.some((l) => l.length > i); i++) {
    out.push(...lists.map((l) => l[i]).filter(Boolean).sort(cmp));
  }
  return out.slice(0, limit);
}

// ---------- notifications ----------

export interface OfferLike {
  id: string;
  priceMist: string;
  kind?: string;
  item?: { id: string; name: string; imageUrl?: string | null } | null;
  collection?: { name: string } | null;
  createdAt?: string | null;
}

export interface Note {
  id: string;
  at: string;
  text: string;
  /** positive: money came in. */
  tone: "positive" | "muted";
  imageUrl: string | null;
  itemId: string | null;
}

const sui = (mist: string | null | undefined) => `${fmtSui(mist, { dp: 3, trim: true })} SUI`;

/** rare.market events and fillable offers as one list of plain sentences, newest first. */
export function toNotes(address: string, events: MarketEvent[], offers: OfferLike[], limit = 30, maxOffers = 5): Note[] {
  const me = canonAddr(address);
  const mine = (a: string | null | undefined) => !!a && canonAddr(a) === me;
  const notes: Note[] = [];
  const seen = new Set<string>();
  for (const e of events) {
    if (seen.has(e.id)) continue;
    seen.add(e.id);
    const name = e.item?.name ?? e.collection?.name ?? "an item";
    let text: string | null = null;
    let tone: Note["tone"] = "muted";
    switch (e.kind) {
      case "sale":
        if (mine(e.from)) {
          text = `Sold ${name} for ${sui(e.priceMist)}`;
          tone = "positive";
        } else if (mine(e.to)) text = `Bought ${name} for ${sui(e.priceMist)}`;
        break;
      case "list":
        if (mine(e.from)) text = `Listed ${name} for ${sui(e.priceMist)}`;
        break;
      case "delist":
        if (mine(e.from)) text = `Delisted ${name}`;
        break;
      case "transfer":
        if (mine(e.to)) text = `Received ${name}`;
        else if (mine(e.from)) text = `Sent ${name}`;
        break;
      case "mint":
        if (mine(e.to)) text = `Minted ${name}`;
        break;
      case "offer":
        if (mine(e.from)) text = `You offered ${sui(e.priceMist)} for ${name}`;
        break;
    }
    if (text) notes.push({ id: `e:${e.id}`, at: e.at, text, tone, imageUrl: e.item?.imageUrl ?? null, itemId: e.item?.id ?? null });
  }
  // Offers: the best one per item (the API sends them highest first), a handful at most.
  const best = new Map<string, OfferLike>();
  for (const o of offers) {
    const k = o.item?.id ?? `collection:${o.collection?.name ?? ""}`;
    const cur = best.get(k);
    if (!cur || BigInt(o.priceMist || "0") > BigInt(cur.priceMist || "0")) best.set(k, o);
  }
  const top = [...best.values()].sort((x, y) => (BigInt(y.priceMist || "0") > BigInt(x.priceMist || "0") ? 1 : -1)).slice(0, maxOffers);
  for (const o of top) {
    const on = o.item?.name ?? o.collection?.name ?? "your items";
    const text = o.kind === "collection" || !o.item ? `Collection offer of ${sui(o.priceMist)} on ${on}` : `Offer of ${sui(o.priceMist)} on ${on}`;
    notes.push({ id: `o:${o.id}`, at: o.createdAt ?? new Date(0).toISOString(), text, tone: "muted", imageUrl: o.item?.imageUrl ?? null, itemId: o.item?.id ?? null });
  }
  return notes.sort((a, b) => Date.parse(b.at) - Date.parse(a.at)).slice(0, limit);
}

export async function loadNotes(get: RareGet, address: string, game: OverlayGame): Promise<Note[]> {
  const per = await Promise.all(
    game.collections.map((c) =>
      Promise.all([
        get<Page<MarketEvent>>(`/v1/events?limit=20&account=${enc(address)}&collection=${enc(c.nftType)}`).then((p) => p.data).catch(() => [] as MarketEvent[]),
        get<Page<OfferLike>>(`/v1/offers?limit=10&receiver=${enc(address)}&collection=${enc(c.nftType)}`).then((p) => p.data).catch(() => [] as OfferLike[]),
      ]),
    ),
  );
  return toNotes(address, per.flatMap((x) => x[0]), per.flatMap((x) => x[1]));
}

// ---------- trophies ----------

export interface Trophy {
  id: string;
  type: string;
  name: string;
  imageUrl: string | null;
  description: string | null;
}

export interface ChainObject {
  objectId: string;
  type: string;
  display: Record<string, unknown> | null;
}

const str = (v: unknown) => (typeof v === "string" && v.trim() ? v.trim() : null);

/** A struct's own name: `0x..::badges::SeasonOneBadge<..>` is "Season One Badge". */
export function structLabel(type: string): string {
  const s = canonType(type).split("<")[0].split("::").pop() ?? type;
  return s.replace(/_/g, " ").replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/\s+/g, " ").trim();
}

/** Objects from the game's own packages that are not one of its tradeable collections. */
export function pickTrophies(objects: ChainObject[], game: OverlayGame): Trophy[] {
  const pkgs = new Set(game.packages);
  return objects
    .filter((o) => pkgs.has(packageOf(o.type)) && !game.collections.some((c) => typeMatches(o.type, c.nftType)))
    .map((o) => ({
      id: o.objectId,
      type: o.type,
      name: str(o.display?.name) ?? structLabel(o.type),
      imageUrl: str(o.display?.image_url) ?? str(o.display?.img_url) ?? null,
      description: str(o.display?.description),
    }));
}

/** Everything the address owns directly, with Display, up to `max` objects. */
export async function ownedObjects(address: string, max = 300): Promise<ChainObject[]> {
  const out: ChainObject[] = [];
  let cursor: string | null = null;
  do {
    const r: { objects: Array<{ objectId: string; type: string; display?: unknown }>; hasNextPage: boolean; cursor: string | null } = await grpc().core.listOwnedObjects({
      owner: address,
      limit: 50,
      cursor,
      include: { display: true },
    });
    for (const o of r.objects) {
      const d = (o.display ?? null) as { output?: Record<string, unknown> } | Record<string, unknown> | null;
      out.push({ objectId: o.objectId, type: o.type, display: d && "output" in d && d.output ? (d.output as Record<string, unknown>) : (d as Record<string, unknown> | null) });
    }
    cursor = r.hasNextPage ? r.cursor : null;
  } while (cursor && out.length < max);
  return out;
}

export async function loadTrophies(address: string, game: OverlayGame, read: (a: string) => Promise<ChainObject[]> = ownedObjects): Promise<Trophy[]> {
  if (!game.packages.length) return [];
  return pickTrophies(await read(address), game);
}
