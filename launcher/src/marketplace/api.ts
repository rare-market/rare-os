// Rare Marketplace data for the native marketplace. Everything goes through native.rareGet (the
// Rust rare_get proxy in the app, the Vite proxy in browser dev): rare.market sends no CORS headers.
// Shapes follow market/api/src/routes (discovery, collections, items, market, holders).
//
// Responses are cached in memory with stale-while-revalidate: a cached answer shows at once and is
// refreshed in the background when older than its TTL, so going back is instant. The cache starts
// out with what Rust's data cache held at launch (lib/state.ts seeds), so a screen seen in an
// earlier session also draws its data in the first frame, and mounted readers re-render when Rust
// revalidates a path and brings different data.

import { useCallback, useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { native } from "../lib/bridge";
import { onStateChange, seed } from "../lib/state";

export interface CollectionRef {
  slug: string;
  name: string;
  imageUrl: string | null;
  verified: boolean;
}

export interface CollectionRow extends CollectionRef {
  rank: number;
  supply: number;
  floorMist: string | null;
  floorChangePct: number | null;
  topOfferMist: string | null;
  volumeMist: string;
  volumeChangePct: number | null;
  sales: number;
  listedCount: number;
  owners: number;
}

export interface HomeFeed {
  featured: Array<{ kind: "new" | "featured"; title: string; imageUrl: string | null; href: string; floorMist: string | null }>;
  movers: Array<CollectionRef & { floorMist: string | null; volumeChangePct: number }>;
}

export interface CollectionDetail extends CollectionRef {
  description: string | null;
  bannerUrl: string | null;
  type: string;
  royaltyBps: number | null;
  stats: {
    floorMist: string | null;
    floorChange24hPct: number | null;
    topOfferMist: string | null;
    volume7dMist: string | null;
    sales7d: number;
    listedCount: number;
    supply: number;
    owners: number;
    uniqueOwnerPct: number;
  };
  traitTypes: Array<{ type: string; valueCount: number }>;
  venues: string[];
}

export interface Listing {
  venue: string;
  priceMist: string;
  allInMist?: string;
  feeMist?: string;
  commissionMist?: string;
  seller?: string;
  listedAt?: string;
  nftType?: string;
  kioskId?: string | null;
  extra?: { royaltyMist?: string; feeBps?: number } & Record<string, unknown>;
}

export interface ItemCard {
  id: string;
  name: string;
  type: string;
  imageUrl: string | null;
  collection: CollectionRef | null;
  rarity?: { rank: number; tier: string } | null;
  listing?: Listing | null;
  lastSaleMist?: string | null;
  topOfferMist?: string | null;
  owner?: string | null;
  kioskId?: string | null;
  /** Set when a staking contract (or an object the owner holds) keeps the item for its owner: it counts as theirs but cannot be listed or sent until unstaked. */
  staked?: { protocol: string | null; name: string | null; via: "staked" | "nested" } | null;
}

/** An offer or bid (GET /v1/offers). Shape of rare.market api/src/routes/market.ts toOffers. Amounts are
 * MIST as decimal strings. With `?item=` the API returns offers on that item plus collection offers it
 * can fill, open and fillable only, highest price first. */
export interface Offer {
  id: string;
  venue: string;
  /** Where the bid lives, e.g. "tradeport/bids", "tradeport/kiosk-bids", "drip/v2-bids". */
  source: string;
  /** Move type the bid is for (null for a few legacy bids). */
  nftType: string | null;
  feeMist: string;
  royaltyMist: string;
  commissionMist: string;
  poolId: string | null;
  /** False when the bid exists but cannot be filled (pool short, frozen store). */
  fillable: boolean;
  extra: Record<string, unknown>;
  kind: "collection" | "item" | "trait";
  collection: CollectionRef;
  item: { id: string; name: string; imageUrl: string } | null;
  trait: { type: string; value: string } | null;
  priceMist: string;
  quantity: number;
  filled: number;
  bidder: string;
  /** Kiosk the bidder receives items in (rare's own offers). */
  destKioskId: string | null;
  createdAt: string;
  expiresAt: string | null;
  status: "open" | "filled" | "cancelled" | "expired";
}

export interface ItemDetail extends ItemCard {
  description?: string | null;
  attributes?: Array<{ type: string; value: string; pct?: number | null; floorMist?: string | null }>;
}

export interface MarketEvent {
  id: string;
  kind: "sale" | "list" | "delist" | "offer" | "offer_cancel" | "transfer" | "mint";
  venue: string | null;
  item: { id: string; name: string; imageUrl: string | null; rarity?: { rank: number; tier: string } | null } | null;
  collection: CollectionRef | null;
  priceMist: string | null;
  from: string | null;
  to: string | null;
  fromName: string | null;
  toName: string | null;
  at: string;
}

export interface Holder {
  rank: number;
  address: string;
  name: string | null;
  count: number;
  pctOfSupply: number;
  listed: number;
  staked: number;
}

export interface HoldersPage {
  data: Holder[];
  total: number;
  header: { owners: number; uniqueOwnersPct: number; top10Pct: number; top100Pct: number; itemsHeld: number; supply: number; stakedOrLocked: Array<{ name: string; count: number }> };
}

export interface TraitValue {
  type: string;
  value: string;
  count: number;
  pct: number;
  floorMist: string | null;
}

export interface Page<T> {
  data: T[];
  nextCursor: string | null;
  total?: number;
}

export interface SearchResult {
  collections: Array<CollectionRef & { floorMist: string | null; volumeMist: string }>;
  accounts: Array<{ address: string; name: string | null }>;
}

export interface Status {
  suiUsd: number;
}

export type SortName = "price_asc" | "price_desc" | "rarity_asc" | "listed_desc";

const enc = encodeURIComponent;

export const paths = {
  status: () => "/v1/status",
  home: () => "/v1/home",
  collections: (window: string, sort = "volume", limit = 20) => `/v1/collections?window=${window}&sort=${sort}&limit=${limit}`,
  collection: (slug: string) => `/v1/collections/${enc(slug)}`,
  items: (slug: string, sort: SortName, traits: string) => `/v1/collections/${enc(slug)}/items?sort=${sort}&limit=36${traits ? `&traits=${enc(traits)}` : ""}`,
  traits: (slug: string) => `/v1/collections/${enc(slug)}/traits`,
  holders: (slug: string) => `/v1/collections/${enc(slug)}/holders?limit=25`,
  item: (id: string) => `/v1/items/${enc(id)}`,
  events: (q: { collection?: string; item?: string; account?: string }, limit = 30) =>
    `/v1/events?limit=${limit}${q.collection ? `&collection=${enc(q.collection)}` : ""}${q.item ? `&item=${enc(q.item)}` : ""}${q.account ? `&account=${enc(q.account)}` : ""}`,
  accountItems: (address: string) => `/v1/accounts/${enc(address)}/items?limit=36`,
  search: (q: string) => `/v1/search?q=${enc(q)}`,
};

// ---------- loading bar ----------

export const useLoading = create<{ n: number }>(() => ({ n: 0 }));
const track = async <T>(p: Promise<T>): Promise<T> => {
  useLoading.setState((s) => ({ n: s.n + 1 }));
  try {
    return await p;
  } finally {
    useLoading.setState((s) => ({ n: Math.max(0, s.n - 1) }));
  }
};

// ---------- cache ----------

interface Entry {
  data?: unknown;
  at: number;
  inflight?: Promise<unknown>;
}
const cache = new Map<string, Entry>();
const MAX_ENTRIES = 300;

function remember(key: string, e: Entry) {
  cache.delete(key);
  cache.set(key, e);
  if (cache.size > MAX_ENTRIES) cache.delete(cache.keys().next().value as string);
}

/** The cache entry for a path, filled from the start-up seed the first time it is asked for. */
function entry(path: string): Entry | undefined {
  const e = cache.get(path);
  if (e) return e;
  const s = seed(`rare:${path}`);
  if (!s) return undefined;
  const fill: Entry = { data: s.v, at: s.at };
  remember(path, fill);
  return fill;
}

// Readers mounted per path, re-rendered when newer data for it lands.
const readers = new Map<string, Set<() => void>>();
function watch(path: string, bump: () => void) {
  let set = readers.get(path);
  if (!set) readers.set(path, (set = new Set()));
  set.add(bump);
  return () => {
    set!.delete(bump);
    if (!set!.size) readers.delete(path);
  };
}
// Rust revalidated a path in the background and its data changed: refresh the ones on screen.
onStateChange((key) => {
  if (!key.startsWith("rare:/")) return;
  const path = key.slice(5);
  const set = readers.get(path);
  if (!set) return;
  void load(path, true).then(() => set.forEach((b) => b()), () => {});
});

/** Fetches a path once at a time; the answer lands in the cache. `quiet`: a refresh of data already
 *  on screen, which does not show the loading bar. */
export function load<T>(path: string, quiet = false): Promise<T> {
  const e = cache.get(path);
  if (e?.inflight) return e.inflight as Promise<T>;
  const get = native.rareGet<T>(path);
  const p = (quiet ? get : track(get)).then(
    (data) => {
      remember(path, { data, at: Date.now() });
      return data;
    },
    (err) => {
      const cur = cache.get(path);
      if (cur) cur.inflight = undefined;
      throw err;
    },
  );
  remember(path, { ...(e ?? { at: 0 }), inflight: p });
  return p;
}

export const cached = <T>(path: string): T | undefined => entry(path)?.data as T | undefined;

/** Stale-while-revalidate read. `data` is the cached answer at once, refreshed when older than ttl. */
export function useRare<T>(path: string | null, ttlMs = 30_000): { data: T | undefined; error: boolean; loading: boolean } {
  const [, bump] = useState(0);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    if (!path) return;
    let live = true;
    const unwatch = watch(path, () => live && bump((n) => n + 1));
    setError(false);
    const e = entry(path);
    if (e?.data !== undefined && Date.now() - e.at < ttlMs) return unwatch;
    // Seeded or stale data stays on screen while it refreshes: no loading state for it.
    setLoading(e?.data === undefined);
    load<T>(path, e?.data !== undefined)
      .then(() => live && bump((n) => n + 1))
      .catch(() => live && setError(true))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
      unwatch();
    };
  }, [path, ttlMs]);
  return { data: path ? cached<T>(path) : undefined, error, loading };
}

// ---------- paged lists ----------

interface Pages<T> {
  items: T[];
  cursor: string | null;
  total?: number;
  done: boolean;
  at: number;
}
const pages = new Map<string, Pages<unknown>>();

/**
 * A cursor-paged list (items grid, activity). The first page is revalidated like useRare; `more()`
 * appends the next page and is safe to call repeatedly (it is a no-op while a page is loading).
 */
const firstPage = (base: string) => `${base}${base.includes("?") ? "&" : "?"}`.replace(/[?&]$/, "");

/** The pages kept for a list, or its first page from the start-up seed (so it draws in the first frame). */
function pagesOf<T>(base: string): Pages<T> | undefined {
  const have = pages.get(base);
  if (have) return have as Pages<T>;
  const e = entry(firstPage(base));
  const p = e?.data as Page<T> | undefined;
  if (!e || !p || !Array.isArray(p.data)) return undefined;
  const fill: Pages<T> = { items: p.data, cursor: p.nextCursor, total: p.total, done: !p.nextCursor, at: e.at };
  pages.set(base, fill as Pages<unknown>);
  return fill;
}

export function usePaged<T>(base: string | null, ttlMs = 30_000) {
  const [, bump] = useState(0);
  const [error, setError] = useState(false);
  const busy = useRef(false);
  const join = (cursor: string | null) => (base ? `${base}${base.includes("?") ? "&" : "?"}${cursor ? `cursor=${encodeURIComponent(cursor)}` : ""}`.replace(/[?&]$/, "") : "");

  useEffect(() => {
    if (!base) return;
    let live = true;
    setError(false);
    // Kept or seeded data is on screen already and refreshes quietly when old.
    const have = pagesOf<T>(base);
    if (have && Date.now() - have.at < ttlMs) return;
    busy.current = true;
    load<Page<T>>(join(null), !!have)
      .then((p) => {
        // Fresh first page; keep later pages only if nothing was cached before.
        pages.set(base, { items: p.data, cursor: p.nextCursor, total: p.total, done: !p.nextCursor, at: Date.now() });
        if (live) bump((n) => n + 1);
      })
      .catch(() => live && setError(true))
      .finally(() => {
        busy.current = false;
      });
    return () => {
      live = false;
    };
  }, [base, ttlMs]); // eslint-disable-line react-hooks/exhaustive-deps

  const more = useCallback(() => {
    if (!base || busy.current) return;
    const cur = pages.get(base);
    if (!cur || cur.done || !cur.cursor) return;
    busy.current = true;
    load<Page<T>>(join(cur.cursor))
      .then((p) => {
        const seen = new Set((cur.items as Array<{ id?: string }>).map((x) => x?.id));
        const add = p.data.filter((x) => !seen.has((x as { id?: string })?.id));
        pages.set(base, { ...cur, items: [...cur.items, ...add], cursor: p.nextCursor, done: !p.nextCursor, total: p.total ?? cur.total });
        bump((n) => n + 1);
      })
      .catch(() => setError(true))
      .finally(() => {
        busy.current = false;
      });
  }, [base]); // eslint-disable-line react-hooks/exhaustive-deps

  const p = base ? pagesOf<T>(base) : undefined;
  return { items: p?.items ?? [], total: p?.total, done: p?.done ?? false, ready: !!p, error, more };
}
