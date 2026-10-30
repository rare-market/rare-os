// Pure inventory logic: grouping, optimistic operations and when the indexer has caught up.

import type { SuiGame } from "../lib/rare";
import { canonType } from "../sui/chain";
import type { InvGroup, InvItem, InvOp, Inventory, Pending } from "./types";

const big = (v: string | null | undefined): bigint => {
  try {
    return v ? BigInt(v) : 0n;
  } catch {
    return 0n;
  }
};

/** Held for the player by a staking contract or one of their own objects: counted and valued, but not listable, sellable or sendable. */
export const isStaked = (i: Pick<InvItem, "staked">): boolean => !!i.staked;

/** The plain line shown where list and send would be. */
export const unstakeLine = (i: Pick<InvItem, "collection" | "name">): string => `Unstake on ${i.collection?.name ?? "its project"} to list or send`;

export const sumFloors = (items: InvItem[]): string => items.reduce((s, i) => s + big(i.floorMist), 0n).toString();

const makeGroup = (game: InvGroup["game"], items: InvItem[]): InvGroup => ({ game, items, floorValueMist: sumFloors(items) });

/** Groups items under the game whose collections hold their type; games keep catalog order, the rest come last. */
export function groupItems(items: InvItem[], games: SuiGame[], floors: Record<string, string | null> = {}): InvGroup[] {
  const owner = new Map<string, SuiGame>();
  for (const g of games) for (const c of g.collections) owner.set(canonType(c.nftType), g);
  const buckets = new Map<string, InvItem[]>();
  for (const it of items) {
    const type = canonType(it.type);
    const g = owner.get(type);
    const key = g?.slug ?? "";
    const list = buckets.get(key) ?? [];
    list.push({ ...it, floorMist: it.floorMist ?? floors[type] ?? null });
    buckets.set(key, list);
  }
  const out: InvGroup[] = [];
  for (const g of games) {
    const list = buckets.get(g.slug);
    if (list?.length) out.push(makeGroup({ slug: g.slug, name: g.name, logoUrl: g.logoUrl }, list));
  }
  const rest = buckets.get("");
  if (rest?.length) out.push(makeGroup(null, rest));
  return out;
}

/** The API's grouped answer (or anything shaped like it), made safe to render. */
export function normalizeInventory(raw: { data?: unknown; total?: number } | null | undefined): Inventory {
  const list = Array.isArray(raw?.data) ? (raw!.data as InvGroup[]) : [];
  const groups = list
    .filter((g) => g && Array.isArray(g.items) && g.items.length)
    .map((g) => makeGroup(g.game ?? null, g.items));
  return { groups, total: groups.reduce((n, g) => n + g.items.length, 0) };
}

const rebuild = (groups: InvGroup[]): Inventory => {
  const kept = groups.filter((g) => g.items.length).map((g) => makeGroup(g.game, g.items));
  return { groups: kept, total: kept.reduce((n, g) => n + g.items.length, 0) };
};

const mapItems = (inv: Inventory, f: (i: InvItem) => InvItem): Inventory =>
  rebuild(inv.groups.map((g) => ({ ...g, items: g.items.map(f) })));

/** One optimistic operation on an inventory. */
export function applyOp(inv: Inventory, op: InvOp): Inventory {
  switch (op.kind) {
    case "remove": {
      const ids = new Set(op.ids);
      return rebuild(inv.groups.map((g) => ({ ...g, items: g.items.filter((i) => !ids.has(i.id)) })));
    }
    case "list":
      return mapItems(inv, (i) => (i.id === op.id && !isStaked(i) ? { ...i, listing: { venue: "kiosk", priceMist: op.priceMist, allInMist: op.priceMist, kioskId: i.kioskId ?? null } } : i));
    case "delist":
      return mapItems(inv, (i) => (i.id === op.id ? { ...i, listing: null } : i));
  }
}

export const applyPending = (base: Inventory, pending: Pending[]): Inventory => pending.reduce((inv, p) => applyOp(inv, p.op), base);

const find = (inv: Inventory, id: string): InvItem | undefined => {
  for (const g of inv.groups) for (const i of g.items) if (i.id === id) return i;
};

/** Whether a fetched inventory already shows what the operation did. */
export function reflects(inv: Inventory, op: InvOp): boolean {
  switch (op.kind) {
    case "remove":
      return op.ids.every((id) => !find(inv, id));
    case "list": {
      const l = find(inv, op.id)?.listing;
      return !!l && big(l.priceMist) === big(op.priceMist);
    }
    case "delist": {
      const it = find(inv, op.id);
      return !it || !it.listing;
    }
  }
}

/** Pending operations the fetched inventory has caught up with, or that waited too long, are dropped. */
export function settle(pending: Pending[], fetched: Inventory, now: number, maxAgeMs = 90_000): Pending[] {
  return pending.filter((p) => now - p.at < maxAgeMs && !reflects(fetched, p.op));
}

/** Notices that mean the player's holdings changed on chain. */
export const MOVES = new Set(["sold", "bought", "nft_received", "offer_received"]);

export const totalFloorMist = (inv: Inventory): string => inv.groups.reduce((s, g) => s + big(g.floorValueMist), 0n).toString();

export interface GameValue {
  slug: string;
  name: string;
  logoUrl: string;
  items: number;
  valueMist: string;
}

export interface InvSummary {
  items: number;
  valueMist: string;
  /** Items that have an open offer on them. */
  offers: number;
  /** Per game, most valuable first. Collectibles that belong to no listed game are not a game and are left out of this list. */
  games: GameValue[];
}

/** What Home shows: the total floor value of the player's game items, and per game. */
export function summarize(inv: Inventory): InvSummary {
  const games: GameValue[] = inv.groups
    .filter((g) => g.game)
    .map((g) => ({ slug: g.game!.slug, name: g.game!.name, logoUrl: g.game!.logoUrl, items: g.items.length, valueMist: g.floorValueMist }))
    .sort((a, b) => (big(b.valueMist) > big(a.valueMist) ? 1 : big(b.valueMist) < big(a.valueMist) ? -1 : b.items - a.items));
  const inGames = inv.groups.filter((g) => g.game);
  return {
    items: inGames.reduce((n, g) => n + g.items.length, 0),
    valueMist: inGames.reduce((s, g) => s + big(g.floorValueMist), 0n).toString(),
    offers: inGames.reduce((n, g) => n + g.items.filter((i) => big(i.topOfferMist) > 0n).length, 0),
    games,
  };
}
