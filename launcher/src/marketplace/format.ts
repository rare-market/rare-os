// Number, price, address and time formatting, as rare.market does it (market/web/lib/format.ts,
// English only). Amounts arrive as MIST decimal strings.

export const DASH = "--";
const MIST_PER_SUI = 1_000_000_000n;

export type MistLike = string | bigint | number | null | undefined;

export function toBigInt(v: MistLike): bigint | null {
  if (v === null || v === undefined || v === "") return null;
  if (typeof v === "bigint") return v;
  if (typeof v === "number") return Number.isFinite(v) ? BigInt(Math.round(v)) : null;
  return /^-?\d+$/.test(v) ? BigInt(v) : null;
}

/** MIST to a JS number of SUI. Display only. */
export function mistToSui(v: MistLike): number | null {
  const b = toBigInt(v);
  if (b === null) return null;
  return Number(b / MIST_PER_SUI) + Number(b % MIST_PER_SUI) / 1e9;
}

const trimZeros = (s: string) => (s.includes(".") ? s.replace(/\.?0+$/, "") : s);

/** SUI amount: `dp` fixed decimals (cards 3), `compact` abbreviates ("16.571K"). Unknown is "--". */
export function fmtSui(v: MistLike, opts: { dp?: number; compact?: boolean; trim?: boolean } = {}): string {
  const n = mistToSui(v);
  if (n === null) return DASH;
  const { dp = 3, compact = false, trim = false } = opts;
  if (compact && Math.abs(n) >= 1_000_000) return `${trimZeros((n / 1_000_000).toFixed(2))}M`;
  if (compact && Math.abs(n) >= 10_000) return `${trimZeros((n / 1_000).toFixed(3))}K`;
  return n.toLocaleString("en-US", {
    minimumFractionDigits: trim ? 0 : n !== 0 && Math.abs(n) < 1000 ? dp : Math.min(dp, 2),
    maximumFractionDigits: Math.abs(n) >= 1000 ? Math.min(dp, 2) : dp,
  });
}

export function fmtUsd(v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return DASH;
  const a = Math.abs(v);
  if (a >= 1_000_000) return `$${trimZeros((v / 1_000_000).toFixed(1))}M`;
  if (a >= 10_000) return `$${trimZeros((v / 1_000).toFixed(1))}K`;
  return `$${v.toLocaleString("en-US", { minimumFractionDigits: a < 1000 ? 2 : 0, maximumFractionDigits: a < 1000 ? 2 : 0 })}`;
}

export function mistToUsd(v: MistLike, suiUsd: number | null | undefined): number | null {
  const n = mistToSui(v);
  return n === null || !suiUsd ? null : n * suiUsd;
}

export function fmtInt(v: number | null | undefined, opts: { compact?: boolean } = {}): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return DASH;
  if (opts.compact && Math.abs(v) >= 1_000_000) return `${trimZeros((v / 1_000_000).toFixed(2))}M`;
  if (opts.compact && Math.abs(v) >= 10_000) return `${trimZeros((v / 1_000).toFixed(2))}K`;
  return Math.round(v).toLocaleString("en-US");
}

export function fmtPct(v: number | null | undefined, dp = 1): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return DASH;
  return `${Math.abs(v).toFixed(dp)}%`;
}

/** "0x7zcR...yUg4" style, 4 + 4 after the 0x. */
export function shortAddr(addr: string | null | undefined): string {
  if (!addr) return DASH;
  const a = addr.startsWith("0x") ? addr.slice(2) : addr;
  if (a.length <= 10) return addr;
  return `0x${a.slice(0, 4)}…${a.slice(-4)}`;
}

/** Terse relative time: "20s", "2h", "1d", "12mo", "2y". */
export function timeAgo(iso: string | number | null | undefined, now = Date.now()): string {
  if (!iso) return DASH;
  const t = typeof iso === "number" ? iso : Date.parse(iso);
  if (!Number.isFinite(t)) return DASH;
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d}d`;
  const mo = Math.floor(d / 30);
  if (mo < 12) return `${mo}mo`;
  return `${Math.floor(d / 365)}y`;
}

/** "#7409" from "Collection Name #7409". */
export function shortName(card: { name: string; collection?: { name: string } | null }): string {
  const prefix = card.collection ? `${card.collection.name} ` : "";
  return prefix && card.name.startsWith(prefix) ? card.name.slice(prefix.length) : card.name;
}

export const VENUES: Record<string, { label: string; icon: string }> = {
  market: { label: "Rare", icon: "/venues/market.png" },
  tradeport: { label: "Tradeport", icon: "/venues/tradeport.svg" },
  drip: { label: "Drip", icon: "/venues/drip.svg" },
  kiosk: { label: "Kiosk", icon: "/venues/kiosk.svg" },
};
export const venueName = (v: string | null | undefined) => (v ? VENUES[v]?.label ?? v : "");

export const EVENT: Record<string, string> = { sale: "Sale", list: "List", delist: "Delist", offer: "Offer", offer_cancel: "Offer cancel", transfer: "Transfer", mint: "Mint" };

export const TIER: Record<string, string> = { common: "Common", uncommon: "Uncommon", rare: "Rare", epic: "Epic", legendary: "Legendary", mythic: "Mythic" };

/** The price a buyer pays for a listing: all-in when the API knows it. */
export const allIn = (l: { priceMist: string; allInMist?: string } | null | undefined) => (l ? l.allInMist ?? l.priceMist : null);

export const itemUrl = (id: string) => `https://rare.market/item/${id}`;
export const collectionUrl = (slug: string) => `https://rare.market/collection/${slug}`;
