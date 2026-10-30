// Recent activity for an address from the Trade API's indexer (Sui's gRPC API has no "transactions by
// address" query). the Trade API only indexes mainnet. The detail page reads one transaction over gRPC
// (see txdetail.ts).

import { TRADE_URL } from "./prices";

/** One balance change in the Trade API's /ext/activity answer. */
export interface ActivityChange {
  coinType: string;
  /** signed base units */
  amount: string;
  symbol?: string;
  decimals?: number;
  logo?: string;
}

export interface TradeActivityItem {
  digest: string;
  timestampMs: number;
  kind: "send" | "receive" | "swap" | "other";
  changes: ActivityChange[];
  counterparty?: string;
  status?: "success" | "failure";
  /** transfers rolled into one row where history kept only their sum */
  count?: number;
}

export interface ActivityItem {
  digest: string;
  /** ms since epoch */
  timestamp: number;
  kind: "send" | "receive" | "swap" | "other";
  changes: ActivityChange[];
  /** "1.5 SUI", "215.195 SUI → 502.6154 MAGMA" */
  summary: string;
  counterparty?: string;
  status: "success" | "failed";
  count?: number;
}

export interface ActivityPage {
  items: ActivityItem[];
  /** pass back for the next page; null at the end */
  cursor: string | null;
}

export interface ActivityFeed {
  list(address: string, cursor?: string | null): Promise<ActivityPage>;
}

const PAGE = 30;

export function createActivityFeed(opts: { baseUrl?: string; network?: "mainnet" | "testnet"; fetch?: typeof fetch } = {}): ActivityFeed {
  const base = (opts.baseUrl ?? TRADE_URL).replace(/\/+$/, "");
  // unbound on purpose: a stored bare `fetch` throws "Illegal invocation" in the webview
  const f: typeof fetch = opts.fetch ?? ((...a) => globalThis.fetch(...a));
  return {
    async list(address, cursor) {
      if (opts.network === "testnet") return { items: [], cursor: null };
      const url = `${base}/ext/activity/${address}?limit=${PAGE}${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`;
      const res = await f(url, { headers: { accept: "application/json" } });
      const body = (await res.json().catch(() => null)) as { items?: unknown; cursor?: unknown; error?: unknown } | null;
      if (!res.ok) throw new Error(typeof body?.error === "string" ? body.error : `Activity unavailable (${res.status})`);
      if (!body || !Array.isArray(body.items)) throw new Error("Activity unavailable");
      return { items: (body.items as TradeActivityItem[]).map(toActivityItem), cursor: typeof body.cursor === "string" && body.cursor ? body.cursor : null };
    },
  };
}

const feed = createActivityFeed();

/** One page of activity, newest first. */
export const activity = (address: string, cursor?: string | null): Promise<ActivityPage> => feed.list(address, cursor);

export function toActivityItem(y: TradeActivityItem): ActivityItem {
  const out = y.changes.filter((c) => c.amount.startsWith("-"));
  const inn = y.changes.filter((c) => !c.amount.startsWith("-"));
  const kind: ActivityItem["kind"] = y.kind !== "other" ? y.kind : out.length && inn.length ? "swap" : out.length ? "send" : "receive";
  const legs = (cs: ActivityChange[]) => cs.map(fmt).join(", ");
  let summary = kind === "swap" && out.length && inn.length ? `${legs(out)} → ${legs(inn)}` : legs(y.changes) || "Transaction";
  if (y.count && y.count > 1) summary += ` in ${y.count} transfers`;
  return {
    digest: y.digest,
    kind,
    changes: y.changes,
    summary,
    timestamp: y.timestampMs,
    status: y.status === "failure" ? "failed" : "success",
    ...(y.counterparty && kind !== "swap" ? { counterparty: y.counterparty } : {}),
    ...(y.count ? { count: y.count } : {}),
  };
}

/** "1.5 SUI": the size of a change, unsigned (the row's label says which way). */
function fmt(c: ActivityChange): string {
  const dec = c.decimals ?? 9;
  const digits = c.amount.replace(/^-/, "");
  const sym = c.symbol ?? c.coinType.split("::").pop() ?? "";
  if (!/^\d+$/.test(digits)) return sym;
  const raw = BigInt(digits);
  const unit = 10n ** BigInt(dec);
  const v = Number(raw / unit) + Number(raw % unit) / Number(unit);
  return `${v.toLocaleString(undefined, { maximumFractionDigits: 4 })} ${sym}`;
}
