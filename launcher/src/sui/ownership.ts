// Game ownership on Sui. rare.market's games catalog (games.json) lists each Sui game's collections
// by Move type. A game counts as owned when the address holds at least one object of one of those
// types: rare.market's holdings (wallet and kiosks) first, then a direct gRPC read for the types it
// doesn't index yet. Rare Store licences (licence.ts) count too once the package is published.

import { create } from "zustand";
import type { SuiGame } from "../lib/rare";
import type { ItemCard } from "../marketplace/api";
import { normalizeStructTag } from "@mysten/sui/utils";

// canonType lives in ./chain with the gRPC client; the client loads only when the chain is actually asked.
function canonType(t: string): string {
  try {
    return normalizeStructTag(t);
  } catch {
    return t.trim();
  }
}
import { rareSui, type HeldCollection } from "./data";

export interface GameOwnership {
  slug: string;
  /** objects held across the game's collections */
  count: number;
  /** the first few items, for art */
  items: ItemCard[];
  /** held collections with their rare.market slug when it has one (for "Your items") */
  collections: Array<{ name: string; slug: string | null; type: string; count: number }>;
}

/**
 * Whether an owned object's type belongs to a catalog collection type. A catalog type with type
 * arguments must match exactly; one without (`0x..::pawtato::LAND`) also matches any instantiation
 * of that struct (`0x..::nonfungible::Nft<...>` stays exact because the catalog spells it out).
 */
export function typeMatches(owned: string, catalog: string): boolean {
  const o = canonType(owned);
  const c = canonType(catalog);
  if (o === c) return true;
  if (c.includes("<")) return false;
  const base = o.split("<")[0];
  return base === c;
}

/** Joins held collections with the catalog: slug -> ownership, only games with something held. */
export function matchOwnership(games: SuiGame[], held: Array<Pick<HeldCollection, "type" | "count" | "slug" | "name" | "items">>): Map<string, GameOwnership> {
  const out = new Map<string, GameOwnership>();
  for (const g of games) {
    for (const col of g.collections) {
      for (const h of held) {
        if (!typeMatches(h.type, col.nftType)) continue;
        const cur = out.get(g.slug) ?? { slug: g.slug, count: 0, items: [], collections: [] };
        cur.count += h.count;
        cur.items.push(...(h.items ?? []).slice(0, Math.max(0, 8 - cur.items.length)));
        cur.collections.push({ name: h.name || col.name, slug: h.slug || null, type: canonType(h.type), count: h.count });
        out.set(g.slug, cur);
      }
    }
  }
  return out;
}

/** Ownership for every catalog game. rare.market first, gRPC for collection types it missed. */
export async function readOwnership(address: string, games: SuiGame[], onFirst?: (m: Map<string, GameOwnership>) => void): Promise<Map<string, GameOwnership>> {
  const held: HeldCollection[] = (await rareSui.collections(address).catch(() => ({ data: [] as HeldCollection[] }))).data;
  const map = matchOwnership(games, held);
  onFirst?.(new Map(map));
  // Types rare.market had nothing for: ask the chain (wallet-held only; kiosks come from rare.market).
  const known = new Set(held.map((h) => canonType(h.type)));
  const missing = games.flatMap((g) => g.collections.filter((c) => ![...known].some((k) => typeMatches(k, c.nftType))).map((c) => ({ g, c })));
  const reads = await Promise.all(
    missing.map(({ c }) =>
      import("./chain")
        .then((ch) => ch.ownedOfType(address, canonType(c.nftType), 8))
        .then((objs) => ({ c, objs }))
        .catch(() => ({ c, objs: [] })),
    ),
  );
  for (const { c, objs } of reads) {
    if (!objs.length) continue;
    const extra = matchOwnership(games, [
      {
        type: objs[0].type,
        count: objs.length,
        slug: "",
        name: c.name,
        items: objs.map((o) => ({ id: o.objectId, name: String(o.display?.name ?? c.name), type: o.type, imageUrl: (o.display?.image_url as string | undefined) ?? null, collection: null })),
      },
    ]);
    for (const [slug, e] of extra) {
      const cur = map.get(slug);
      if (!cur) map.set(slug, e);
      else {
        cur.count += e.count;
        cur.collections.push(...e.collections);
        cur.items.push(...e.items.slice(0, Math.max(0, 8 - cur.items.length)));
      }
    }
  }
  return map;
}

interface OwnState {
  address: string | null;
  bySlug: Map<string, GameOwnership>;
  at: number;
  loading: boolean;
  load: (address: string | null, games: SuiGame[]) => Promise<void>;
}

/** Ownership for the current account, read once a minute at most. */
export const useOwnership = create<OwnState>((set, get) => ({
  address: null,
  bySlug: new Map(),
  at: 0,
  loading: false,
  load: async (address, games) => {
    if (!address) return set({ address: null, bySlug: new Map(), at: 0 });
    const s = get();
    if (s.loading || (s.address === address && Date.now() - s.at < 60_000)) return;
    set({ loading: true, ...(s.address !== address ? { bySlug: new Map(), address } : {}) });
    try {
      // rare.market's answer shows at once; the chain reads for the rest follow.
      const first = (m: Map<string, GameOwnership>) => get().address === address && set({ bySlug: m });
      const all = await readOwnership(address, games, first);
      if (get().address === address) set({ bySlug: all, at: Date.now() });
    } finally {
      set({ loading: false });
    }
  },
}));
