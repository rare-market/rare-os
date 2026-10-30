// Shared helpers for the wallet's home, coin, collectible and activity pages: formatting,
// small cached loaders (instant on the way back, refreshed when the wallet's epoch bumps) and
// the account kind labels.

import { useEffect, useRef, useState } from "react";
import type { Hint } from "../../../components/Chrome";
import { useWalletHints } from "../store";
import { activity, type ActivityItem } from "../../data/activity";
import { portfolio, type Portfolio } from "../../data/balances";
import { walletNfts, type ItemCard } from "../../data/nfts";
import type { RareAccount } from "../../index";
import { kept, persist } from "../../../lib/state";

export const isSuiType = (t: string) => /^0x0*2::sui::SUI$/.test(t);

/** Token amount: grouped, with decimals that fit the size ("1,204.5", "0.000123"). */
export function fmtAmount(n: number | null | undefined, max = 6): string {
  if (n == null || !Number.isFinite(n)) return "--";
  const a = Math.abs(n);
  const dp = a >= 1000 ? 2 : a >= 1 ? 4 : max;
  return n.toLocaleString("en-US", { maximumFractionDigits: dp });
}

/** Base units (string, maybe signed) to a number of whole tokens. */
export function unitsToNumber(raw: string, decimals: number): number {
  const neg = raw.startsWith("-");
  const digits = raw.replace(/^-/, "");
  if (!/^\d+$/.test(digits)) return 0;
  const v = BigInt(digits);
  const unit = 10n ** BigInt(decimals);
  const n = Number(v / unit) + Number(v % unit) / Number(unit);
  return neg ? -n : n;
}

export const shortDigest = (d: string) => (d.length > 12 ? `${d.slice(0, 6)}...${d.slice(-4)}` : d);

export function dayLabel(ts: number): string {
  const d = new Date(ts);
  if (d.toDateString() === new Date().toDateString()) return "Today";
  if (d.toDateString() === new Date(Date.now() - 864e5).toDateString()) return "Yesterday";
  return d.toLocaleDateString([], { month: "short", day: "numeric", ...(d.getFullYear() !== new Date().getFullYear() ? { year: "numeric" } : {}) });
}

/** How the account signs, as a short plain label. */
export function kindLabel(a: Pick<RareAccount, "kind" | "hd">): string {
  switch (a.kind) {
    case "watch":
      return "Watch only";
    case "ledger":
      return "Ledger";
    case "keystone":
      return "Keystone";
    case "passkey":
      return "Passkey";
    case "social":
      return "Social";
    default:
      return a.hd ? "Recovery phrase" : "Private key";
  }
}

/** Which Settings group an account falls under on the Accounts page. */
export const kindGroup = (a: Pick<RareAccount, "kind">): "rare" | "hardware" | "watch" =>
  a.kind === "watch" ? "watch" : a.kind === "ledger" || a.kind === "keystone" ? "hardware" : "rare";

// ---------- loaders ----------

const store = new Map<string, unknown>();

/**
 * Loads once per key and again when `dep` changes (the wallet epoch). The last answer for the key
 * is returned at once, so going back to a page does not flash empty; it is also kept in Rust's
 * cache (`ui:w:<key>`), so the wallet shows the last balances in the first frame after a start.
 */
export function useLoad<T>(key: string | null, load: () => Promise<T>, dep?: unknown) {
  const [, force] = useState(0);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(false);
  const fn = useRef(load);
  fn.current = load;
  useEffect(() => {
    if (!key) return;
    let live = true;
    setLoading(true);
    setError(false);
    fn.current()
      .then((v) => {
        if (!live) return;
        store.set(key, v);
        persist(`ui:w:${key}`, v);
        force((n) => n + 1);
      })
      .catch(() => live && setError(true))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [key, dep]);
  let data = key ? (store.get(key) as T | undefined) : undefined;
  if (key && data === undefined) {
    data = kept<T>(`ui:w:${key}`);
    if (data !== undefined) store.set(key, data);
  }
  return { data, loading, error };
}

export const usePortfolio = (address: string | undefined, epoch: number) =>
  useLoad<Portfolio>(address ? `pf:${address}` : null, () => portfolio(address!), epoch);

/** Wallet NFTs from rare.market. */
export function useNfts(address: string | undefined, epoch: number) {
  const q = useLoad<{ items: ItemCard[]; total?: number }>(
    address ? `nft:${address}` : null,
    async () => {
      const p = await walletNfts(address!, { limit: 60 });
      return { items: p.data, total: p.total };
    },
    epoch,
  );
  return { items: q.data?.items ?? [], total: q.data?.total, ready: q.data !== undefined, loading: q.loading, error: q.error };
}

// ---------- activity (paged) ----------

interface Feed {
  items: ActivityItem[];
  cursor: string | null;
  done: boolean;
}
const feeds = new Map<string, Feed>();
/** Detail pages open an item the list already has. */
const seen = new Map<string, ActivityItem>();
export const knownActivity = (digest: string) => seen.get(digest);

export function useActivity(address: string | undefined, epoch: number) {
  const [, force] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);
  const busy = useRef(false);
  const [tries, setTries] = useState(0);

  const take = (items: ActivityItem[]) => {
    for (const i of items) seen.set(i.digest, i);
    return items;
  };

  useEffect(() => {
    if (!address) return;
    let live = true;
    busy.current = true;
    setLoading(true);
    setError(false);
    activity(address)
      .then((p) => {
        feeds.set(address, { items: take(p.items), cursor: p.cursor, done: !p.cursor });
        if (live) force((n) => n + 1);
      })
      .catch(() => live && setError(true))
      .finally(() => {
        busy.current = false;
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [address, epoch, tries]); // eslint-disable-line react-hooks/exhaustive-deps

  const more = () => {
    if (!address || busy.current) return;
    setError(false);
    const cur = feeds.get(address);
    if (!cur || cur.done) return;
    busy.current = true;
    setLoading(true);
    activity(address, cur.cursor)
      .then((p) => {
        const have = new Set(cur.items.map((i) => i.digest));
        feeds.set(address, { items: [...cur.items, ...take(p.items.filter((i) => !have.has(i.digest)))], cursor: p.cursor, done: !p.cursor });
        force((n) => n + 1);
      })
      .catch(() => setError(true))
      .finally(() => {
        busy.current = false;
        setLoading(false);
      });
  };

  const f = address ? feeds.get(address) : undefined;
  return { items: f?.items ?? [], ready: !!f, done: f?.done ?? false, loading, error, more, retry: () => (f ? more() : setTries((n) => n + 1)) };
}

/** The thin loading bar from the Marketplace (no skeletons). */
export function LoadBar({ on }: { on: boolean }) {
  return <div className={`pointer-events-none h-[2px] w-full ${on ? "load-bar" : ""}`} />;
}

/** Bottom-bar hints while this page is on top. */
export function useHints(active: boolean, hints: Hint[]) {
  const sig = hints.map((h) => h.b + h.label).join("|");
  useEffect(() => {
    if (active) useWalletHints.setState({ hints });
  }, [active, sig]); // eslint-disable-line react-hooks/exhaustive-deps
}
