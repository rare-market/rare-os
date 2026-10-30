// rare.market's Rare Store API (rare-store docs/OFFCHAIN.md), read through the Rust rare_get proxy
// (rare.market sends no CORS headers). In a plain browser (UI development, screenshots) a failed
// read falls back to ./mock, and ?storedemo answers from it first.

import { isTauri, native } from "../lib/bridge";
import type { Licence, Shelf, StoreConfig, StoreGame, StoreGameDetail, SuiPrice } from "./types";

export const STORE_LIMIT = 24;

type Get = <T>(path: string) => Promise<T>;

async function get<T>(path: string): Promise<T> {
  // Browser only: ?storedemo answers from the mock first (demo uploaded games, owned and buyable states).
  if (!isTauri && new URLSearchParams(location.search).has("storedemo")) {
    const r = (await import("./mock")).answer(path);
    if (r !== undefined) return r as T;
  }
  try {
    return await native.rareGet<T>(path);
  } catch (e) {
    if (isTauri) throw e;
    const mock = await import("./mock");
    const r = mock.answer(path);
    if (r === undefined) throw e;
    return r as T;
  }
}

const str = (v: unknown): string | null => (typeof v === "string" && v.trim() ? v.trim() : typeof v === "number" ? String(v) : null);

/**
 * /v1/store/config. The fields come from the server env (RARE_STORE_PACKAGE, _ORIGIN, _STORE,
 * _STORE_VERSION, _POLICY, _POLICY_VERSION, _USDC_TYPE); accepts snake_case, camelCase and the
 * env names, flat or under `config`. Every id is null until the package is deployed.
 */
export function normalizeConfig(raw: unknown): StoreConfig {
  const o = ((raw as { config?: unknown })?.config ?? raw ?? {}) as Record<string, unknown>;
  const pick = (...keys: string[]) => {
    for (const k of keys) {
      const v = str(o[k]);
      if (v) return v;
    }
    return null;
  };
  return {
    package: pick("package", "package_id", "packageId", "RARE_STORE_PACKAGE"),
    origin: pick("origin", "origin_id", "originId", "original_package", "RARE_STORE_ORIGIN"),
    store: pick("store", "store_id", "storeId", "RARE_STORE_STORE"),
    storeVersion: pick("store_version", "storeVersion", "store_initial_shared_version", "RARE_STORE_STORE_VERSION"),
    policy: pick("policy", "policy_id", "policyId", "RARE_STORE_POLICY"),
    policyVersion: pick("policy_version", "policyVersion", "RARE_STORE_POLICY_VERSION"),
    usdcType: pick("usdc_type", "usdcType", "RARE_STORE_USDC_TYPE"),
    uploadsOpen: o.uploads_open === true || o.uploadsOpen === true,
  };
}

/** The store can sell: package, Store and USDC type are known. Until then buy says "Coming soon". */
export const canSell = (c: StoreConfig | null | undefined): c is StoreConfig & { package: string; store: string; usdcType: string } =>
  !!c && !!c.package && !!c.store && !!c.usdcType;

/** Resale needs the shared TransferPolicy<ResaleLicence> too. */
export const canResell = (c: StoreConfig | null | undefined) => canSell(c) && !!c!.policy;

function listOf<T>(raw: unknown, key: string): { items: T[]; next: string | null } {
  const o = raw as Record<string, unknown> | unknown[] | null;
  const items = Array.isArray(o) ? o : Array.isArray((o as Record<string, unknown>)?.[key]) ? (o as Record<string, unknown>)[key] : Array.isArray((o as Record<string, unknown>)?.data) ? (o as Record<string, unknown>).data : [];
  const r = (o ?? {}) as Record<string, unknown>;
  const next = Array.isArray(o) ? null : str(r.next) ?? str(r.nextCursor) ?? str(r.cursor);
  return { items: items as T[], next };
}

export const storePaths = {
  config: () => "/v1/store/config",
  games: (shelf: Shelf, o: { kind?: "link" | "uploaded"; q?: string; limit?: number; cursor?: string | null } = {}) => {
    const p = new URLSearchParams({ shelf, limit: String(o.limit ?? STORE_LIMIT) });
    if (o.kind) p.set("kind", o.kind);
    if (o.q) p.set("q", o.q);
    if (o.cursor) p.set("cursor", o.cursor);
    return `/v1/store/games?${p}`;
  },
  game: (slug: string) => `/v1/store/games/${encodeURIComponent(slug)}`,
  search: (q: string) => `/v1/store/search?q=${encodeURIComponent(q)}`,
  licences: (owner: string) => `/v1/store/licences?owner=${encodeURIComponent(owner)}`,
  check: (owner: string, slug: string) => `/v1/store/check?owner=${encodeURIComponent(owner)}&slug=${encodeURIComponent(slug)}`,
  suiPrice: () => "/v1/store/price/sui",
};

export function storeApi(g: Get = get) {
  return {
    config: async () => normalizeConfig(await g<unknown>(storePaths.config())),
    games: async (shelf: Shelf, o?: Parameters<typeof storePaths.games>[1]) => listOf<StoreGame>(await g<unknown>(storePaths.games(shelf, o)), "games"),
    game: async (slug: string) => {
      const r = await g<{ game?: StoreGameDetail } | StoreGameDetail>(storePaths.game(slug));
      return ((r as { game?: StoreGameDetail }).game ?? r) as StoreGameDetail;
    },
    search: async (q: string) => listOf<StoreGame>(await g<unknown>(storePaths.search(q)), "games").items,
    licences: async (owner: string) => listOf<Licence>(await g<unknown>(storePaths.licences(owner)), "licences").items,
    check: (owner: string, slug: string) => g<{ owns: boolean; licence_id: string | null; edition: number | null }>(storePaths.check(owner, slug)),
    suiPrice: () => g<SuiPrice>(storePaths.suiPrice()),
  };
}

export const api = storeApi();
