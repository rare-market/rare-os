// Rare Marketplace (rare.market) data: accounts, collectibles, the Sui Games catalog.
// Types follow market/web/lib/api/types.ts and web/lib/games.ts.

import { native } from "./bridge";
import seed from "./games.seed.json";
import { withBuiltins } from "../games/catalog";
import { artSrc, seed as cachedSeed, sizeFor } from "./state";

export const RARE = "https://rare.market";

export interface ItemCard {
  id: string;
  name: string;
  type: string;
  imageUrl: string | null;
  collection: { slug: string; name: string; imageUrl: string | null; verified: boolean } | null;
  rarity?: { rank: number; tier: string } | null;
  listing?: { priceMist: string; allInMist?: string } | null;
  topOfferMist?: string | null;
  kioskId?: string | null;
}

export interface Page<T> {
  data: T[];
  nextCursor: string | null;
  total?: number;
}

export interface Account {
  address: string;
  name: string | null;
  avatarUrl: string | null;
  totalItems: number;
  listedCount: number;
}

export interface Status {
  suiUsd: number;
  tps: number;
  checkpoint: number;
  degraded: boolean;
}

export type GameStatus = "live" | "beta" | "coming" | "announced" | "paused" | "dead";
export type GamePlatform = "web" | "pc" | "mobile" | "suiplay";

export interface SuiGame {
  slug: string;
  name: string;
  studio: string;
  genre: string;
  status?: GameStatus;
  platforms: GamePlatform[];
  website: string;
  /** Where the game plays in a browser, when it differs from the website. */
  playUrl?: string | null;
  x: string;
  description: string;
  logoUrl: string;
  bannerUrl: string;
  collections: Array<{ nftType: string; name: string }>;
  token: { symbol: string; coinType: string } | null;
  featured: boolean;
}

export const rare = {
  status: () => native.rareGet<Status>("/v1/status"),
  account: (address: string) => native.rareGet<Account>(`/v1/accounts/${address}`),
  items: (address: string, limit = 60) => native.rareGet<Page<ItemCard>>(`/v1/accounts/${address}/items?limit=${limit}`),
  search: (q: string) => native.rareGet<{ collections: unknown[]; accounts: Array<{ address: string; name: string | null }> }>(`/v1/search?q=${encodeURIComponent(q)}`),
};

function clean(raw: unknown): SuiGame[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((g) => g && typeof g.slug === "string" && typeof g.name === "string")
    .map((g) => ({
      studio: "",
      genre: "",
      website: "",
      x: "",
      description: "",
      logoUrl: "",
      bannerUrl: "",
      token: null,
      featured: false,
      ...g,
      platforms: Array.isArray(g.platforms) ? g.platforms : [],
      collections: Array.isArray(g.collections) ? g.collections : [],
    }));
}

let cached: Promise<{ games: SuiGame[]; live: boolean }> | null = null;
let cachedAt = 0;
let latest: SuiGame[] | null = null;

/** The catalog at once, for a first render: this session's last answer, or what Rust's cache held
 *  at start (lib/state.ts). Null only on a device that never loaded it. loadGames() still refreshes. */
export function gamesNow(): SuiGame[] | null {
  if (latest) return latest;
  const s = cachedSeed<unknown>("rare:/v1/games");
  const games = s ? fromApi(s.v) : [];
  return games.length ? (latest = withBuiltins(games)) : null;
}

/** The bundled catalog with the built-ins: what the Sui Games tab shows until rare.market answers. */
export function bundledGames(): SuiGame[] {
  return withBuiltins(clean(seed));
}

/** /v1/games entries in the games.json shape the screens use. Collections a studio has not proven on chain are left
 *  out (the overlay must not map them to the game); null links become empty, as in games.json. */
export function fromApi(raw: unknown): SuiGame[] {
  const data = raw && typeof raw === "object" ? (raw as { data?: unknown }).data : null;
  return clean(
    (Array.isArray(data) ? data : []).map((g) => ({
      ...g,
      x: g?.x ?? "",
      collections: Array.isArray(g?.collections) ? g.collections.filter((c: { verified?: boolean }) => c?.verified !== false) : [],
    })),
  );
}

/** The Sui Games catalog from rare.market's games table (/v1/games, which studios edit in Studio), read live so
 *  changes there show without a launcher release; the static games.json when the API does not answer, then a
 *  bundled copy for offline use. The launcher's built-in web games are added. Shared by every screen for five minutes. */
export function loadGames(): Promise<{ games: SuiGame[]; live: boolean }> {
  if (cached && Date.now() - cachedAt < 5 * 60_000) return cached;
  cachedAt = Date.now();
  cached = (async () => {
    try {
      const games = fromApi(await native.rareGet<unknown>("/v1/games"));
      if (games.length) return { games: (latest = withBuiltins(games)), live: true };
    } catch {
      /* API down or offline */
    }
    try {
      const games = clean(await native.rareGet<unknown>("/games/games.json"));
      if (games.length) return { games: withBuiltins(games), live: true };
    } catch {
      /* offline */
    }
    cachedAt = 0;
    return { games: withBuiltins(clean(seed)), live: false };
  })();
  return cached;
}

/** Image URL for a collectible, drawn about `width` px wide: in the app, through Rust's image cache
 *  at that size (lib/state.ts artSrc). */
export function img(url: string | null | undefined, width = 400): string {
  if (!url) return "";
  if (url.startsWith("ipfs://")) url = `https://ipfs.io/ipfs/${url.slice(7)}`;
  return /^https?:\/\//.test(url) ? artSrc(url, sizeFor(width)) : url;
}

export const mistToSui = (mist: string | number | null | undefined): number => (mist == null ? 0 : Number(mist) / 1e9);
