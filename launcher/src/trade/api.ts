// Market data for the Trade tab: token lists, a token's page (chart, holders, trades), the
// wallet's priced portfolio and the perps markets. Shapes follow the Trade API routes
// (/tokens, /token/:type/*, /ext/*). Answers are cached in memory with
// stale-while-revalidate, so going back is instant and nothing flashes.

import { useEffect, useState } from "react";
import { create } from "zustand";
import { useApp } from "../lib/store";
import { poll } from "../lib/idle";
import { normalizeType } from "./amount";
import { tradeGet } from "./http";

export interface TokenRow {
  coinType: string;
  symbol: string;
  name: string;
  decimals: number;
  logo: string | null;
  priceUsd: number | null;
  change?: { m5?: number; h1?: number; h6?: number; h24?: number } | null;
  /** lean rows (ext) carry the 24h change flat */
  change24h?: number | null;
  volume24h?: number | null;
  liquidity?: number | null;
  mcap?: number | null;
  holders?: number | null;
  buys24h?: number | null;
  sells24h?: number | null;
  createdMs?: number | null;
  verified?: boolean;
}

export interface TokenDetail extends TokenRow {
  fdv?: number | null;
  supply?: number | null;
  description?: string | null;
  links?: Array<{ label: string; url: string }>;
}

export interface Holder {
  address: string;
  amount: number;
  valueUsd: number | null;
  pctSupply: number | null;
  kind: string;
  venue: string | null;
}

export interface HoldersPage {
  count: number | null;
  holders: Holder[];
  top10Pct: number | null;
}

export interface Trade {
  id: string;
  timeMs: number;
  buy: boolean;
  usd: number;
  amount: number;
  priceUsd: number;
  trader: string;
}

export interface Candle {
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume?: number;
}

export interface Holding {
  coinType: string;
  symbol: string;
  name: string;
  decimals: number;
  logo: string | null;
  priceUsd: number | null;
  change24h: number | null;
  verified: boolean;
  amount: number;
  baseUnits: string;
  valueUsd: number;
}

export interface Portfolio {
  totalUsd: number;
  change24hUsd: number;
  change24hPct: number;
  holdings: Holding[];
}

/** A logo as an absolute URL: the Trade API serves some from its own paths (/coins/usdc.png). */
export function logoUrl(u: string | null | undefined): string | null {
  if (!u) return null;
  if (u.startsWith("//")) return `https:${u}`;
  if (u.startsWith("/")) return `https://rare.market/v1/trade/asset${u}`;
  return u;
}

/** Market picture by id, loaded once (a day in the cache) and shared by every perps screen. */
export function usePerpLogos(): (marketId: string, symbol?: string) => string | null {
  const { data } = useApi<{ items?: Array<{ id: string; logo?: string | null; symbol?: string }> }>("/perps?l=1", 6 * 3_600_000);
  const byId = new Map<string, string>();
  const bySym = new Map<string, string>();
  for (const it of data?.items ?? []) {
    if (!it.logo) continue;
    byId.set(it.id.toLowerCase(), it.logo);
    if (it.symbol) bySym.set(it.symbol.toUpperCase(), it.logo);
  }
  return (id, symbol) => byId.get(id.toLowerCase()) ?? (symbol ? bySym.get(symbol.toUpperCase()) : undefined) ?? null;
}

export const change24 = (t: Pick<TokenRow, "change" | "change24h">) => t.change?.h24 ?? t.change24h ?? null;

const enc = encodeURIComponent;
export const tokenPaths = {
  list: (list: "trending" | "new" | "gainers" | "top", limit = 20) => `/tokens?list=${list}&limit=${limit}`,
  pickList: (limit = 60) => `/ext/tokens?limit=${limit}`,
  token: (type: string) => `/token/${enc(normalizeType(type))}`,
  lean: (type: string) => `/ext/token/${enc(normalizeType(type))}`,
  holders: (type: string, limit = 25) => `/token/${enc(normalizeType(type))}/holders?limit=${limit}`,
  trades: (type: string) => `/token/${enc(normalizeType(type))}/trades`,
  candles: (type: string, tf: string, limit = 300) => `/token/${enc(normalizeType(type))}/candles?tf=${tf}&limit=${limit}`,
  portfolio: (address: string) => `/ext/portfolio/${address}`,
  search: (q: string) => `/ext/search?q=${enc(q)}`,
};

// ---------- loading bar ----------

export const useTradeLoading = create<{ n: number }>(() => ({ n: 0 }));
async function track<T>(p: Promise<T>): Promise<T> {
  useTradeLoading.setState((s) => ({ n: s.n + 1 }));
  try {
    return await p;
  } finally {
    useTradeLoading.setState((s) => ({ n: Math.max(0, s.n - 1) }));
  }
}

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

/** Fetches a path once at a time; the answer lands in the cache. */
export function load<T>(path: string, fetcher: (p: string) => Promise<T> = tradeGet): Promise<T> {
  const e = cache.get(path);
  if (e?.inflight) return e.inflight as Promise<T>;
  const p = track(fetcher(path)).then(
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

export const cached = <T>(path: string): T | undefined => cache.get(path)?.data as T | undefined;

/**
 * Stale-while-revalidate read. `data` is the cached answer at once; it is refreshed when older
 * than `ttlMs`, and every `pollMs` while the screen is up when that is set.
 */
export function useApi<T>(path: string | null, ttlMs = 30_000, pollMs = 0): { data: T | undefined; error: string | null } {
  const [, bump] = useState(0);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let live = true;
    setError(null);
    const run = () => {
      // Nothing refreshes while a game has the screen.
      if (useApp.getState().running.length) return;
      const e = cache.get(path);
      if (e?.data !== undefined && Date.now() - e.at < ttlMs) return;
      load<T>(path)
        .then(() => live && bump((n) => n + 1))
        .catch((err: unknown) => live && setError(err instanceof Error ? err.message : "Could not load"));
    };
    run();
    const stop = pollMs ? poll(run, slowWhenIdle(pollMs)) : null;
    return () => {
      live = false;
      stop?.();
    };
  }, [path, ttlMs, pollMs]);
  return { data: path ? cached<T>(path) : undefined, error };
}

/** A screen's poll: as asked while in use, at least a minute apart idle, none while away. */
const slowWhenIdle = (ms: number) => ({ active: ms, idle: Math.max(ms * 6, 60_000), away: null });

/** An async value keyed by `key`; null key means nothing to load. Errors become a message. */
export function useAsync<T>(key: string | null, fn: () => Promise<T>, pollMs = 0): { data: T | undefined; error: string | null; loading: boolean } {
  const [state, setState] = useState<{ key: string | null; data?: T; error: string | null; loading: boolean }>({ key: null, error: null, loading: false });
  useEffect(() => {
    if (!key) {
      setState({ key: null, error: null, loading: false });
      return;
    }
    let live = true;
    let first = true;
    const run = () => {
      if (!first && useApp.getState().running.length) return;
      first = false;
      setState((s) => ({ ...s, key, loading: true, ...(s.key !== key ? { data: undefined, error: null } : {}) }));
      track(fn())
        .then((data) => live && setState({ key, data, error: null, loading: false }))
        .catch((e: unknown) => live && setState((s) => ({ ...s, key, error: e instanceof Error ? e.message : "Could not load", loading: false })));
    };
    run();
    const stop = pollMs ? poll(run, slowWhenIdle(pollMs)) : null;
    return () => {
      live = false;
      stop?.();
    };
  }, [key, pollMs]); // eslint-disable-line react-hooks/exhaustive-deps
  return { data: state.key === key ? state.data : undefined, error: state.key === key ? state.error : null, loading: state.loading };
}

/** Drops cached answers whose path starts with `prefix` (after a trade). */
export function invalidate(prefix: string) {
  for (const k of [...cache.keys()]) if (k.startsWith(prefix)) cache.delete(k);
}
