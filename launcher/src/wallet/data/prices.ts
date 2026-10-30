// Browser-safe client for the Trade API's public extension API (/ext/*, CORS *). Prices, 24h change
// and token safety only; balances themselves always come from gRPC. Everything here is optional:
// callers catch and carry on without prices when the Trade API is down.

export const TRADE_URL = "https://rare.market/v1/trade";

export interface TokenMeta {
  coinType: string;
  symbol: string;
  name: string;
  decimals: number;
  logo: string | null;
  priceUsd: number | null;
  /** percent over 24h */
  change24h: number | null;
  verified: boolean;
}

export interface TokenSafety {
  spam: boolean;
  honeypot: boolean;
}

/** /ext/token/<type>: meta plus market and safety detail. */
export interface TokenDetail extends TokenMeta {
  mcap: number | null;
  volume24h: number | null;
  liquidityUsd: number | null;
  holders: number | null;
  safety: TokenSafety | null;
}

export interface TradeHolding extends TokenMeta {
  /** whole tokens */
  amount: number;
  baseUnits: string;
  valueUsd: number | null;
}

export interface TradePortfolio {
  address: string;
  totalUsd: number;
  change24hUsd: number;
  holdings: TradeHolding[];
}

export interface PriceOptions {
  baseUrl?: string;
  fetch?: typeof fetch;
  timeoutMs?: number;
}

/** the Trade API keys coin types by the short outer address (`0x2::sui::SUI`). */
export const tradeKey = (t: string) => t.replace(/^0x0*/, "0x");

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** Relative logo paths resolve against the Trade API origin. */
function logoUrl(base: string, logo: unknown): string | null {
  if (typeof logo !== "string" || !logo) return null;
  if (/^(https?:|data:|ipfs:)/.test(logo)) return logo;
  if (logo.startsWith("//")) return `https:${logo}`;
  return `${base}${logo.startsWith("/") ? "" : "/"}${logo}`;
}

export class PriceClient {
  readonly baseUrl: string;
  private readonly f: typeof fetch;
  private readonly timeoutMs: number;
  private readonly cache = new Map<string, TokenMeta>();

  constructor(o: PriceOptions = {}) {
    this.baseUrl = (o.baseUrl ?? TRADE_URL).replace(/\/+$/, "");
    // unbound on purpose: a stored bare `fetch` throws "Illegal invocation" in WebKit/Chrome
    this.f = o.fetch ?? ((...a) => globalThis.fetch(...a));
    this.timeoutMs = o.timeoutMs ?? 8000;
  }

  async getJson(path: string): Promise<unknown> {
    const ctl = new AbortController();
    const timer = setTimeout(() => ctl.abort(), this.timeoutMs);
    try {
      const r = await this.f(`${this.baseUrl}${path}`, { signal: ctl.signal, headers: { accept: "application/json" } });
      if (!r.ok) throw new Error(`trade ${path} -> ${r.status}`);
      return await r.json();
    } finally {
      clearTimeout(timer);
    }
  }

  private toMeta(raw: Record<string, unknown>): TokenMeta {
    const coinType = String(raw.coinType ?? "");
    return {
      coinType,
      symbol: typeof raw.symbol === "string" && raw.symbol ? raw.symbol : coinType.split("::").pop() ?? "?",
      name: typeof raw.name === "string" ? raw.name : "",
      decimals: num(raw.decimals) ?? 9,
      logo: logoUrl(this.baseUrl, raw.logo),
      priceUsd: num(raw.priceUsd),
      change24h: num(raw.change24h),
      verified: raw.verified === true,
    };
  }

  /** GET /ext/portfolio/<address>. Seeds the meta cache. */
  async portfolio(address: string): Promise<TradePortfolio> {
    const j = (await this.getJson(`/ext/portfolio/${encodeURIComponent(address)}`)) as Record<string, unknown>;
    const rows = Array.isArray(j.holdings) ? (j.holdings as Record<string, unknown>[]) : [];
    const holdings = rows.map((h): TradeHolding => {
      const meta = this.toMeta(h);
      this.cache.set(tradeKey(meta.coinType), meta);
      return {
        ...meta,
        amount: num(h.amount) ?? 0,
        baseUnits: typeof h.baseUnits === "string" && /^\d+$/.test(h.baseUnits) ? h.baseUnits : "0",
        valueUsd: num(h.valueUsd),
      };
    });
    return { address, totalUsd: num(j.totalUsd) ?? 0, change24hUsd: num(j.change24hUsd) ?? 0, holdings };
  }

  /** GET /ext/token/<type>: full detail incl. safety. Null when not indexed or the Trade API is down. */
  async tokenDetail(coinType: string): Promise<TokenDetail | null> {
    try {
      const j = (await this.getJson(`/ext/token/${encodeURIComponent(coinType)}`)) as Record<string, unknown>;
      const meta = this.toMeta({ ...j, coinType: j.coinType ?? coinType });
      this.cache.set(tradeKey(coinType), meta);
      const s = j.safety as Record<string, unknown> | null | undefined;
      return {
        ...meta,
        mcap: num(j.mcap),
        volume24h: num(j.volume24h),
        liquidityUsd: num(j.liquidityUsd),
        holders: num(j.holders),
        safety: s && typeof s === "object" ? { spam: s.spam === true, honeypot: s.honeypot === true } : null,
      };
    } catch {
      return null;
    }
  }

  /** Cached meta, else /ext/token/<type>. */
  async tokenMeta(coinType: string): Promise<TokenMeta | null> {
    return this.cache.get(tradeKey(coinType)) ?? this.tokenDetail(coinType);
  }

  /** Batch tokenMeta; unknown coins are left out. Keys are the input coin types. */
  async tokens(coinTypes: string[]): Promise<Record<string, TokenMeta>> {
    const out: Record<string, TokenMeta> = {};
    await Promise.all(
      [...new Set(coinTypes)].map(async (t) => {
        const m = await this.tokenMeta(t);
        if (m) out[t] = m;
      }),
    );
    return out;
  }

  /** Cache-only lookup, for synchronous renders. */
  cached(coinType: string): TokenMeta | undefined {
    return this.cache.get(tradeKey(coinType));
  }
}

export const trade = new PriceClient();

/** Plain warning for a token's safety flags, or null when clean or unknown. */
export function safetyWarning(s: TokenSafety | null | undefined): string | null {
  if (!s) return null;
  if (s.honeypot) return "Possible honeypot. You may not be able to sell this token.";
  if (s.spam) return "Flagged as spam.";
  return null;
}
