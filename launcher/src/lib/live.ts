// Live Sui data for the shell: SUI price and its 24 h line, network status, the signed-in
// account's portfolio, top collections and the latest sales. One store, polled on a calm
// schedule (slower when idle, see ./idle), and paused while a game has the screen (the launcher
// must not consume then).
//
// Sources: the Trade API's public extension API (/ext/*, CORS open) for prices, candles and
// portfolios; rare.market's /v1 API (through native.rareGet) for status, collections, events.

import { create } from "zustand";
import { native } from "./bridge";
import { poll } from "./idle";
import { afterBoot, persist, seed } from "./state";
import type { CollectionRow, MarketEvent, Page } from "../marketplace/api";
import type { LinePoint } from "../ui/charts";

const TRADE = "https://rare.market/v1/trade";
const SUI_TYPE = "0x2::sui::SUI";
/** the Trade API's SUI perp market: the hourly line behind the SUI price. */
const SUI_PERP = "0xa97dacdd972a0414363290ec5752cb93ac47443c3e7710df145cc7f337575002";

export interface Portfolio {
  address: string;
  totalUsd: number;
  change24hUsd: number;
}

interface Live {
  suiUsd: number | null;
  suiChange24h: number | null;
  suiVolume24h: number | null;
  suiLine: LinePoint[];
  tps: number | null;
  checkpoint: number | null;
  degraded: boolean;
  portfolio: Portfolio | null;
  collections: CollectionRow[] | null;
  sales: MarketEvent[] | null;
  /** Game tokens by coin type: price and 24 h change (Sui Games cards and pages). */
  tokens: Record<string, { priceUsd: number | null; change24h: number | null }>;
  /** Ids of sales that arrived since the last poll, for the one-time tint. */
  fresh: string[];
  loading: boolean;
}

// What is kept between starts (Rust's data cache, `ui:live`): the first frame shows the last price,
// line, collections and sales, then the first tick refreshes them.
type Kept = Omit<Live, "fresh" | "loading" | "degraded">;
const KEPT: Array<keyof Kept> = ["suiUsd", "suiChange24h", "suiVolume24h", "suiLine", "tps", "checkpoint", "portfolio", "collections", "sales", "tokens"];

export const useLive = create<Live>(() => ({
  suiUsd: null,
  suiChange24h: null,
  suiVolume24h: null,
  suiLine: [],
  tps: null,
  checkpoint: null,
  degraded: false,
  portfolio: null,
  collections: null,
  sales: null,
  fresh: [],
  loading: false,
  tokens: {},
}));

afterBoot(() => {
  const kept = seed<Partial<Kept>>("ui:live")?.v;
  if (kept && useLive.getState().suiUsd === null) useLive.setState(kept);
});
useLive.subscribe((s, prev) => {
  if (!KEPT.some((k) => s[k] !== prev[k])) return;
  persist("ui:live", Object.fromEntries(KEPT.map((k) => [k, s[k]])));
});

async function json<T>(url: string, ms = 8000): Promise<T> {
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), ms);
  try {
    const r = await fetch(url, { signal: ctl.signal });
    if (!r.ok) throw new Error(`${r.status}`);
    return (await r.json()) as T;
  } finally {
    clearTimeout(t);
  }
}

async function sui() {
  const t = await json<{ priceUsd: number | null; change24h: number | null; volume24h: number | null }>(`${TRADE}/ext/token/${SUI_TYPE}`);
  useLive.setState({ suiUsd: t.priceUsd, suiChange24h: t.change24h, suiVolume24h: t.volume24h });
}

async function suiLine() {
  const to = Date.now();
  const r = await json<{ bars: Array<{ time: number; close: number }> }>(`${TRADE}/ext/perps/candles?market=${SUI_PERP}&resolution=60&from=${to - 24 * 3600_000}&to=${to}`);
  const line = r.bars.map((b) => ({ t: b.time, v: b.close }));
  if (line.length > 1) useLive.setState({ suiLine: line });
}

async function status() {
  const s = await native.rareGet<{ suiUsd: number; tps: number; checkpoint: number; degraded: boolean }>("/v1/status");
  useLive.setState((st) => ({ tps: s.tps, checkpoint: s.checkpoint, degraded: s.degraded, suiUsd: st.suiUsd ?? s.suiUsd }));
}

async function portfolio(address: string | null) {
  if (!address) return useLive.setState({ portfolio: null });
  const p = await json<{ totalUsd?: number; change24hUsd?: number }>(`${TRADE}/ext/portfolio/${address}`);
  useLive.setState({ portfolio: { address, totalUsd: p.totalUsd ?? 0, change24hUsd: p.change24hUsd ?? 0 } });
}

async function collections() {
  const r = await native.rareGet<Page<CollectionRow>>("/v1/collections?window=24h&sort=volume&limit=6&verified=true");
  useLive.setState({ collections: r.data });
}

async function sales() {
  const r = await native.rareGet<Page<MarketEvent>>("/v1/events?limit=30&kind=sale");
  // Zero-price transfers between a game's own kiosks are not market sales.
  r.data = r.data.filter((e) => e.priceMist && BigInt(e.priceMist) > 0n).slice(0, 8);
  const prev = new Set((useLive.getState().sales ?? []).map((e) => e.id));
  const fresh = useLive.getState().sales ? r.data.filter((e) => !prev.has(e.id)).map((e) => e.id) : [];
  useLive.setState({ sales: r.data, fresh });
}

let account: string | null = null;
/** The account whose portfolio shows on Home and in the top bar (watch or signed in). */
export function setLiveAccount(address: string | null) {
  if (address === account) return;
  account = address;
  void portfolio(address).catch(() => {});
}

const quiet = (p: Promise<unknown>) => p.catch(() => {});

let started = false;
let paused = false;
/** Pause while a game has the screen. */
export function pauseLive(on: boolean) {
  const was = paused;
  paused = on;
  if (was && !on) void tick(true);
}

let held = false;
/**
 * After a resume Wi-Fi needs a few seconds. Keep what is on screen, skip the polls that would only
 * fail, and refresh in full at 3 s, 8 s and 20 s (each skipped once a refresh has worked).
 */
export function resumeLive() {
  held = true;
  let ok = false;
  for (const ms of [3000, 8000, 20000]) {
    setTimeout(() => {
      if (ok || paused) return;
      held = false;
      void tick(true).then(() => {
        ok = useLive.getState().tps !== null && !useLive.getState().degraded;
      });
    }, ms);
  }
  setTimeout(() => (held = false), 20_500);
}

// What is due when, by time (the tick itself runs every 20 s in use, every 2 minutes idle).
const EVERY = { status: 60_000, minute: 60_000, line: 5 * 60_000 };
const lastRun = { status: 0, minute: 0, line: 0 };
const due = (k: keyof typeof EVERY, now: number, all: boolean) => {
  // A little slack so a 20 s tick doesn't slip a whole period behind.
  if (!all && now - lastRun[k] < EVERY[k] - 2_000) return false;
  lastRun[k] = now;
  return true;
};

async function tick(all = false) {
  if (paused || (held && !all)) return;
  const now = Date.now();
  // Each tick: price and sales. Every minute: network status (the footer's TPS), portfolio and
  // top collections. Every 5 minutes: the line.
  const jobs = [sui(), sales()];
  if (due("status", now, all)) jobs.push(status());
  if (due("minute", now, all)) jobs.push(portfolio(account), collections());
  if (due("line", now, all)) jobs.push(suiLine());
  await Promise.all(jobs.map(quiet));
}

export function startLive() {
  if (started) return;
  started = true;
  useLive.setState({ loading: true });
  void tick(true).finally(() => useLive.setState({ loading: false }));
  // Every 20 s while in use, every 2 minutes idle, nothing while a game or another window is in front.
  poll(() => tick(), { active: 20_000, idle: 120_000, away: null });
}

const tokenAsked = new Map<string, number>();
/** Prices for these coin types (game tokens), fetched at most every 2 minutes each. */
export function loadTokens(types: string[]) {
  const now = Date.now();
  for (const t of types) {
    if (now - (tokenAsked.get(t) ?? 0) < 120_000) continue;
    tokenAsked.set(t, now);
    void json<{ priceUsd: number | null; change24h: number | null }>(`${TRADE}/ext/token/${t}`)
      .then((r) => useLive.setState((s) => ({ tokens: { ...s.tokens, [t]: { priceUsd: r.priceUsd, change24h: r.change24h } } })))
      .catch(() => {});
  }
}
