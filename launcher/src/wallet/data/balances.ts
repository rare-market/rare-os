// Balances and the portfolio. Amounts are always from Sui gRPC (listBalances + getCoinMetadata);
// USD prices and 24h change are layered on from the Trade API's public API when it answers, so the
// wallet keeps working without prices when the Trade API is down.

import { normalizeStructTag } from "@mysten/sui/utils";
import { grpcClient, SUI_TYPE } from "../grpc";
import { trade as defaultTrade, tradeKey, type PriceClient, type TradePortfolio } from "./prices";

export interface Balance {
  coinType: string;
  symbol: string;
  name: string;
  decimals: number;
  iconUrl: string | null;
  raw: bigint;
  amount: number;
}

export interface PortfolioToken extends Balance {
  priceUsd: number | null;
  valueUsd: number | null;
  /** percent over 24h */
  change24h: number | null;
  /** the Trade API lists it as a verified token */
  verified: boolean;
}

export interface Portfolio {
  /** null when no token has a price (the Trade API down) */
  totalUsd: number | null;
  change24hUsd: number | null;
  tokens: PortfolioToken[];
  /** false when prices could not be loaded */
  priced: boolean;
}

/** The slice of the gRPC client used here (stubbed in tests). */
export interface BalanceClient {
  core: {
    listBalances(o: { owner: string; limit?: number; cursor?: string | null }): Promise<{ balances: Array<{ coinType: string; balance: string }>; hasNextPage: boolean; cursor: string | null }>;
    getCoinMetadata(o: { coinType: string }): Promise<{ coinMetadata: { decimals: number; symbol: string; name: string; iconUrl?: string | null } | null }>;
  };
}

const normType = (t: string) => {
  try {
    return normalizeStructTag(t);
  } catch {
    return t;
  }
};

const toAmount = (raw: bigint, decimals: number) => {
  const unit = 10n ** BigInt(decimals);
  return Number(raw / unit) + Number(raw % unit) / Number(unit);
};

export async function balances(address: string, client: BalanceClient = grpcClient as unknown as BalanceClient): Promise<Balance[]> {
  const nodes: Array<{ coinType: string; balance: string }> = [];
  let cursor: string | null = null;
  for (let page = 0; page < 10; page++) {
    const res = await client.core.listBalances({ owner: address, limit: 50, cursor });
    nodes.push(...res.balances);
    if (!res.hasNextPage || !res.cursor) break;
    cursor = res.cursor;
  }
  const metas = await Promise.all(
    nodes.map((n) =>
      client.core
        .getCoinMetadata({ coinType: n.coinType })
        .then((r) => r.coinMetadata)
        .catch(() => null),
    ),
  );
  const out = nodes.map((n, i): Balance => {
    const m = metas[i];
    const decimals = m?.decimals ?? 9;
    const raw = BigInt(n.balance);
    return {
      coinType: n.coinType,
      symbol: m?.symbol || (n.coinType.split("::").pop() ?? "?"),
      name: m?.name ?? "",
      decimals,
      iconUrl: m?.iconUrl ?? null,
      raw,
      amount: toAmount(raw, decimals),
    };
  });
  // SUI first, then larger holdings; zero balances dropped.
  return out.filter((b) => b.raw > 0n).sort((a, b) => (isSui(a.coinType) ? -1 : isSui(b.coinType) ? 1 : b.amount - a.amount));
}

const isSui = (t: string) => normType(t) === SUI_TYPE;

/** Merge gRPC balances with the Trade API prices (pure; exported for tests). */
export function mergePortfolio(list: Balance[], y: TradePortfolio | null): Portfolio {
  const prices = new Map((y?.holdings ?? []).map((h) => [tradeKey(normType(h.coinType)), h]));
  let total = 0;
  let change = 0;
  let any = false;
  const tokens = list.map((b): PortfolioToken => {
    const h = prices.get(tradeKey(normType(b.coinType)));
    const priceUsd = h?.priceUsd ?? null;
    const valueUsd = priceUsd != null ? b.amount * priceUsd : null;
    const change24h = h?.change24h ?? null;
    if (valueUsd != null) {
      any = true;
      total += valueUsd;
      // value 24h ago = now / (1 + pct); the difference is today's move in USD
      if (change24h != null && change24h > -100) change += valueUsd - valueUsd / (1 + change24h / 100);
    }
    return {
      ...b,
      iconUrl: b.iconUrl ?? h?.logo ?? null,
      priceUsd,
      valueUsd,
      change24h,
      verified: h?.verified ?? false,
    };
  });
  // SUI first, then by USD value; unpriced tokens after priced ones, by amount
  tokens.sort((a, b) => {
    if (isSui(a.coinType)) return -1;
    if (isSui(b.coinType)) return 1;
    if (a.valueUsd != null && b.valueUsd != null) return b.valueUsd - a.valueUsd;
    if (a.valueUsd != null) return -1;
    if (b.valueUsd != null) return 1;
    return b.amount - a.amount;
  });
  return { totalUsd: any ? total : null, change24hUsd: any ? change : null, tokens, priced: y != null };
}

/** Balances from gRPC with USD prices from the Trade API when it answers. */
export async function portfolio(address: string, opts: { client?: BalanceClient; trade?: PriceClient } = {}): Promise<Portfolio> {
  const y = opts.trade ?? defaultTrade;
  const [list, prices] = await Promise.all([balances(address, opts.client), y.portfolio(address).catch(() => null)]);
  return mergePortfolio(list, prices);
}
