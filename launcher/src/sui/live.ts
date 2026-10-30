// Live Sui for the Home strip: SUI price and TPS (rare.market status), reference gas (gRPC), the
// player's portfolio and its change, and what is minting on rare.market's launchpad. Polled on a
// calm schedule and paused while a game has the screen.
//
// Portfolio change: rare.market values collectibles but keeps no per-wallet history, so the
// launcher keeps its own small series of USD snapshots on this device and compares against the
// one nearest 24 hours ago (or the oldest it has, labelled with its time).

import { create } from "zustand";
import { useLive } from "../lib/live";
import { useApp } from "../lib/store";
import { poll } from "../lib/idle";
import { liveLaunches, rareSui, type Launch } from "./data";
import { useProfile } from "./profile";

export interface Mint {
  key: string;
  name: string;
  imageUrl: string | null;
  sub: string;
  href: string;
}

interface LiveState {
  suiUsd: number | null;
  tps: number | null;
  gasMist: number | null;
  degraded: boolean;
  portfolioUsd: number | null;
  change: { usd: number; pct: number | null; since: number } | null;
  mints: Mint[] | null;
}

export const useSuiLive = create<LiveState>(() => ({ suiUsd: null, tps: null, gasMist: null, degraded: false, portfolioUsd: null, change: null, mints: null }));

// ---------- portfolio snapshots ----------

export interface Snapshot {
  t: number;
  usd: number;
}

const DAY = 86_400_000;
const SNAP_KEY = (a: string) => `rare.sui.snaps.${a}`;

/** Adds a snapshot (at most one per 10 minutes) and drops those older than two days. */
export function addSnapshot(list: Snapshot[], s: Snapshot): Snapshot[] {
  const kept = list.filter((x) => s.t - x.t < 2 * DAY);
  const last = kept[kept.length - 1];
  if (last && s.t - last.t < 10 * 60_000) return kept;
  return [...kept, s];
}

/** Change against the snapshot nearest 24 h ago (or the oldest when none is that old). Needs 30 minutes of history. */
export function changeSince(list: Snapshot[], now: Snapshot): { usd: number; pct: number | null; since: number } | null {
  const old = list.filter((x) => now.t - x.t >= 30 * 60_000);
  if (!old.length) return null;
  const target = now.t - DAY;
  const base = old.reduce((best, x) => (Math.abs(x.t - target) < Math.abs(best.t - target) ? x : best));
  const usd = now.usd - base.usd;
  return { usd, pct: base.usd > 0 ? (usd / base.usd) * 100 : null, since: base.t };
}

function readSnaps(a: string): Snapshot[] {
  try {
    const v = JSON.parse(localStorage.getItem(SNAP_KEY(a)) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x?.t === "number" && typeof x?.usd === "number") : [];
  } catch {
    return [];
  }
}

function writeSnaps(a: string, list: Snapshot[]) {
  try {
    localStorage.setItem(SNAP_KEY(a), JSON.stringify(list));
  } catch {
    /* private mode */
  }
}

// ---------- polling ----------

const quiet = (p: Promise<unknown>) => p.catch(() => {});

async function status() {
  const s = await rareSui.status();
  useSuiLive.setState({ suiUsd: s.suiUsd, tps: s.tps, degraded: s.degraded });
}

async function gas() {
  useSuiLive.setState({ gasMist: await (await import("./chain")).gasPrice() });
}

/**
 * Total value in USD: coins (the shell's portfolio from the Trade API when it has this account, else SUI
 * alone) plus collectibles at rare.market's value.
 */
export function totalUsd(o: { suiMist: bigint | null; nftMist: bigint | null; suiUsd: number | null; coinsUsd: number | null }): number | null {
  if (!o.suiUsd || (o.suiMist === null && o.nftMist === null && o.coinsUsd === null)) return null;
  const coins = o.coinsUsd ?? (Number(o.suiMist ?? 0n) / 1e9) * o.suiUsd;
  return coins + (Number(o.nftMist ?? 0n) / 1e9) * o.suiUsd;
}

export function coinsUsdFor(address: string | null): number | null {
  const p = useLive.getState().portfolio;
  return p && address && p.address.toLowerCase() === address.toLowerCase() ? p.totalUsd : null;
}

async function portfolio() {
  await useProfile.getState().refresh();
  const p = useProfile.getState();
  const usd = totalUsd({ suiMist: p.suiMist, nftMist: p.nftMist, suiUsd: useSuiLive.getState().suiUsd ?? p.suiUsd, coinsUsd: coinsUsdFor(p.address) });
  if (!p.address || usd === null) return useSuiLive.setState({ portfolioUsd: null, change: null });
  const now = { t: Date.now(), usd };
  const list = readSnaps(p.address);
  const change = changeSince(list, now);
  writeSnaps(p.address, addSnapshot(list, now));
  useSuiLive.setState({ portfolioUsd: usd, change });
}

const launchMint = (l: Launch): Mint => ({
  key: `launch:${l.slug}`,
  name: l.name,
  imageUrl: l.imageUrl ?? null,
  sub: l.supply ? `${(l.minted ?? 0).toLocaleString("en-US")} of ${l.supply.toLocaleString("en-US")} minted` : "Minting now",
  href: `https://rare.market/launchpad/${l.slug}`,
});

async function mints() {
  const [launches, drops, events] = await Promise.all([
    rareSui.launches().catch(() => [] as Launch[]),
    rareSui.drops().then((r) => r.data).catch(() => [] as Launch[]),
    rareSui.mints().then((r) => r.data).catch(() => []),
  ]);
  const live = liveLaunches([...launches, ...drops]).map(launchMint);
  const recent: Mint[] = events
    .filter((e) => e.item)
    .slice(0, 6)
    .map((e) => ({ key: `mint:${e.id}`, name: e.item!.name, imageUrl: e.item!.imageUrl, sub: e.collection?.name ?? "New mint", href: `https://rare.market/item/${e.item!.id}` }));
  useSuiLive.setState({ mints: [...live, ...recent].slice(0, 8) });
}

let started = false;
let lastMints = 0;
async function tick(all = false) {
  if (useApp.getState().running.length > 0) return;
  // Each tick: status (TPS), gas and the portfolio. Every 5 minutes: mints.
  const jobs = [status(), gas(), portfolio()];
  if (all || Date.now() - lastMints >= 5 * 60_000 - 2_000) {
    lastMints = Date.now();
    jobs.push(mints());
  }
  await Promise.all(jobs.map(quiet));
}

export function startSuiLive() {
  if (started) return;
  started = true;
  void tick(true);
  // The footer's TPS and gas: a slow ticker. Every minute in use, every 5 minutes idle, none away.
  poll(() => tick(), { active: 60_000, idle: 5 * 60_000, away: null });
  // Back from a game: catch up at once.
  let was = useApp.getState().running.length;
  useApp.subscribe((s) => {
    if (was > 0 && s.running.length === 0) void tick(true);
    was = s.running.length;
  });
}
