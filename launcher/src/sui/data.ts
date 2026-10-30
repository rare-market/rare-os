// rare.market reads for the Sui features. All through native.rareGet (the Rust rare_get proxy in
// the app, the Vite proxy in browser dev). Shapes follow market/api/src/routes (accounts.ts,
// account-collections.ts, analytics.ts portfolio, discovery.ts status) and web/lib/launchpad.

import { native } from "../lib/bridge";
import type { ItemCard, Page } from "../marketplace/api";

const enc = encodeURIComponent;

export interface RareStatus {
  suiUsd: number;
  tps: number;
  checkpoint: number;
  degraded: boolean;
}

export interface RareAccountInfo {
  address: string;
  /** What rare.market shows: the Rare username, else the SuiNS name. */
  name: string | null;
  username?: string | null;
  suins?: string | null;
  avatarUrl: string | null;
  totalItems: number;
  listedCount: number;
}

export interface Portfolio {
  valueMist: string;
  costMist: string;
  unrealizedPnlMist: string;
  pnlPct: number | null;
  items: number;
  collections: Array<{ collection: { slug: string; name: string; imageUrl: string | null; verified: boolean }; items: number; valueMist: string }>;
}

export interface HeldCollection {
  slug: string;
  name: string;
  imageUrl: string | null;
  verified: boolean;
  type: string;
  count: number;
  floorMist: string | null;
  valueMist: string | null;
  items: ItemCard[];
}

/** One launchpad launch, as rare.market's /launchpad/launches.json index lists it. */
export interface Launch {
  slug: string;
  name: string;
  imageUrl?: string | null;
  minted?: number;
  supply?: number;
  startsAt?: number | string | null;
  endsAt?: number | string | null;
  phases?: Array<{ name?: string; startsAt?: number | string | null; endsAt?: number | string | null; priceMist?: string | null }>;
}

export const rareSui = {
  status: () => native.rareGet<RareStatus>("/v1/status"),
  account: (a: string) => native.rareGet<RareAccountInfo>(`/v1/accounts/${enc(a)}`),
  portfolio: (a: string) => native.rareGet<Portfolio>(`/v1/accounts/${enc(a)}/portfolio?limit=1`),
  /** Every collection the address holds (kiosks included), all type classes so game items count. */
  collections: (a: string) => native.rareGet<{ data: HeldCollection[] }>(`/v1/accounts/${enc(a)}/collections?class=all`),
  items: (a: string, limit = 60, cls = "collectible") => native.rareGet<Page<ItemCard>>(`/v1/accounts/${enc(a)}/items?limit=${limit}&class=${cls}`),
  launches: async (): Promise<Launch[]> => {
    const r = await native.rareGet<unknown>("/launchpad/launches.json");
    return Array.isArray(r) ? (r as Launch[]) : [];
  },
  drops: () => native.rareGet<Page<Launch>>("/v1/drops"),
  mints: () => native.rareGet<Page<{ id: string; item: { id: string; name: string; imageUrl: string | null } | null; collection: { slug: string; name: string } | null; priceMist: string | null; at: string }>>("/v1/events?kind=mint&limit=12"),
};

const ms = (v: number | string | null | undefined): number | null => (v == null ? null : typeof v === "number" ? (v < 1e12 ? v * 1000 : v) : Date.parse(v) || null);

/** Launches minting now (inside a phase or the launch window, and not sold out). */
export function liveLaunches(list: Launch[], now = Date.now()): Launch[] {
  return list.filter((l) => {
    if (l.supply && l.minted !== undefined && l.minted >= l.supply) return false;
    const windows = l.phases?.length ? l.phases.map((p) => [ms(p.startsAt), ms(p.endsAt)] as const) : [[ms(l.startsAt), ms(l.endsAt)] as const];
    return windows.some(([s, e]) => (s === null || s <= now) && (e === null || now < e) && (s !== null || e !== null));
  });
}
