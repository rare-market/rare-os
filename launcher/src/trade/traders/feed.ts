// New trades by followed traders. A calm poll (a few traders per tick, paused while a game runs)
// reads each trader's latest trades; anything newer than what was seen goes out as an event.
//
// Subscribing (for launcher/src/notify or anything else):
//   import { onFollowedTrade } from "../trade/traders/feed";
//   const off = onFollowedTrade((e) => ...);
// or listen for the DOM event "rare:followed-trade" (detail: FollowedTradeEvent).
// Until a notifier takes over (setFollowToasts(false)), each event is also a launcher toast.

import { create } from "zustand";
import { useApp } from "../../lib/store";
import { poll } from "../../lib/idle";
import { feedback } from "../../nav/feedback";
import { numText } from "../../ui";
import { tradeGet } from "../http";
import { traderPaths, type TokenChip, type TradesPage, type TraderTrade } from "./api";
import { useFollows, type Followed } from "./follow";

export interface FollowedTradeEvent {
  trader: { address: string; name: string };
  trade: TraderTrade;
  token: TokenChip | null;
}

export const FOLLOWED_TRADE_EVENT = "rare:followed-trade";

/**
 * The trades in a page newer than `seen` (the newest time already handled), oldest first. With no
 * `seen` yet nothing is new: the first read only sets the mark. Trades before `since` never count.
 */
export function freshTrades(items: TraderTrade[], seen: number | undefined, since = 0): TraderTrade[] {
  if (seen === undefined) return [];
  return items.filter((t) => t.timeMs > seen && t.timeMs >= since).sort((a, b) => a.timeMs - b.timeMs);
}

/** The newest time in a page, or the old mark when the page is empty. */
export const newestTime = (items: TraderTrade[], seen: number | undefined) => items.reduce((m, t) => Math.max(m, t.timeMs), seen ?? 0);

/** One plain sentence for a toast or a notification. */
export function tradeSentence(e: FollowedTradeEvent): string {
  const sym = e.token?.symbol ?? e.trade.coin.split("::").pop() ?? "a coin";
  return `${e.trader.name} ${e.trade.buy ? "bought" : "sold"} ${numText("money", e.trade.usd)} of ${sym}`;
}

type Listener = (e: FollowedTradeEvent) => void;
const listeners = new Set<Listener>();

/** Subscribe to followed traders' new trades. Returns the unsubscribe. */
export function onFollowedTrade(fn: Listener): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

let toasts = true;
/** A notifier that shows these itself turns the fallback toasts off. */
export function setFollowToasts(on: boolean) {
  toasts = on;
}

/** The latest followed trades this session, newest first (the Following list on Traders). */
export const useFollowFeed = create<{ recent: FollowedTradeEvent[] }>(() => ({ recent: [] }));

export function emitFollowedTrade(e: FollowedTradeEvent) {
  useFollowFeed.setState((s) => ({ recent: [e, ...s.recent.filter((r) => r.trade.id !== e.trade.id)].slice(0, 40) }));
  for (const fn of listeners) {
    try {
      fn(e);
    } catch {
      /* one bad subscriber must not stop the rest */
    }
  }
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent<FollowedTradeEvent>(FOLLOWED_TRADE_EVENT, { detail: e }));
  if (toasts) {
    useApp.getState().toast(tradeSentence(e));
    feedback("notify");
  }
}

const seen = new Map<string, number>();
let cursor = 0;
/** Traders read per tick, and the tick. 30 follows are each read about every 50 seconds. */
const PER_TICK = 6;
const TICK_MS = 10_000;

/** One tick: the next few followed traders in turn. Exported for tests. */
export async function pollOnce(list: Followed[], get: <T>(path: string) => Promise<T> = tradeGet): Promise<FollowedTradeEvent[]> {
  if (!list.length) return [];
  const batch = Array.from({ length: Math.min(PER_TICK, list.length) }, (_, i) => list[(cursor + i) % list.length]);
  cursor = (cursor + batch.length) % list.length;
  const out: FollowedTradeEvent[] = [];
  await Promise.all(
    batch.map(async (f) => {
      const page = await get<TradesPage>(traderPaths.trades(f.address, 10)).catch(() => null);
      if (!page) return;
      const mark = seen.get(f.address);
      for (const t of freshTrades(page.items ?? [], mark, f.since)) out.push({ trader: { address: f.address, name: f.name }, trade: t, token: page.tokens?.[t.coin] ?? null });
      seen.set(f.address, newestTime(page.items ?? [], mark));
    }),
  );
  return out.sort((a, b) => a.trade.timeMs - b.trade.timeMs);
}

/** Forgets the marks (tests). */
export function resetFeed() {
  seen.clear();
  cursor = 0;
}

let timer: (() => void) | null = null;

/** Starts the followed-trades poll for the current account. Safe to call more than once. */
export function startTraderFeed(account: () => Promise<string | null>) {
  if (timer) return;
  const tick = async () => {
    // Nothing polls while a game has the screen.
    if (useApp.getState().running.length) return;
    useFollows.getState().use(await account().catch(() => null));
    for (const e of await pollOnce(useFollows.getState().list)) emitFollowedTrade(e);
  };
  void tick();
  // Every 10 s in use, every minute idle, every 2 minutes with another window in front.
  timer = poll(() => tick(), { active: TICK_MS, idle: 60_000, away: 120_000 });
}
