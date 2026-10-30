// Coin search for the Trade tab: the Trade API's own token search (/search), with a gRPC coin-metadata
// fallback so a pasted full coin type opens even when the coin is not indexed. Recent searches
// are kept on the device.

import { isCoinType, normalizeType } from "./amount";
import type { TokenRow } from "./api";
import { grpc } from "./chain";
import { tradeGet } from "./http";

export const searchPath = (q: string) => `/search?q=${encodeURIComponent(q.trim().slice(0, 160))}`;

export interface SearchDeps {
  get: <T>(path: string) => Promise<T>;
  metadata: (coinType: string) => Promise<{ symbol: string; name: string; decimals: number; iconUrl: string | null } | null>;
}

export const realDeps: SearchDeps = {
  get: tradeGet,
  metadata: async (coinType) => (await grpc.core.getCoinMetadata({ coinType })).coinMetadata,
};

/** Pasted text that names a coin: a full type, with or without the short 0x2 form. */
export const pastedType = (q: string) => {
  const t = q.trim().replace(/^["'`]|["'`]$/g, "");
  return isCoinType(t) ? normalizeType(t) : null;
};

/** Coins matching `q` by name, ticker, address or full type. */
export async function searchCoins(q: string, deps: SearchDeps = realDeps): Promise<TokenRow[]> {
  const text = q.trim();
  if (!text) return [];
  const type = pastedType(text);
  const found = await deps.get<{ items?: TokenRow[] }>(searchPath(type ?? text)).then((r) => r.items ?? [], () => [] as TokenRow[]);
  if (!type || found.some((t) => normalizeType(t.coinType) === type)) return found;
  // Not indexed: the Trade API's lean token answer, then the chain's own coin metadata.
  const lean = await deps.get<TokenRow>(`/ext/token/${encodeURIComponent(type)}`).catch(() => null);
  if (lean?.coinType) return [lean, ...found];
  const meta = await deps.metadata(type).catch(() => null);
  if (!meta) return found;
  const row: TokenRow = { coinType: type, symbol: meta.symbol, name: meta.name, decimals: meta.decimals, logo: meta.iconUrl, priceUsd: null };
  return [row, ...found];
}

// ---------- recents ----------

const KEY = "rare:trade:recent-coins";
export interface Recent {
  coinType: string;
  symbol: string;
  name: string;
  logo: string | null;
}

export function recentCoins(): Recent[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((r) => r && typeof r.coinType === "string").slice(0, 8) : [];
  } catch {
    return [];
  }
}

export function rememberCoin(t: Pick<TokenRow, "coinType" | "symbol" | "name" | "logo">) {
  const list = [{ coinType: normalizeType(t.coinType), symbol: t.symbol, name: t.name, logo: t.logo }, ...recentCoins().filter((r) => normalizeType(r.coinType) !== normalizeType(t.coinType))].slice(0, 8);
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* storage blocked: recents just do not stick */
  }
}
