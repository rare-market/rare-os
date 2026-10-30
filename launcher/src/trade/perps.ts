// Perps: markets, charts, accounts, order previews and unsigned order transactions, from the
// the Trade API's perps routes (the perps venue's request schema, served for wallet clients under
// /ext/perps). Types and helpers follow the perps venue's request schema.
// Nothing here signs.

import { Transaction } from "@mysten/sui/transactions";
import { tradeCall, tradeGet, errorOf } from "./http";

export type Side = "long" | "short";

export interface PerpMarket {
  marketId: string;
  symbol: string;
  pair: string;
  price: number;
  changePct: number | null;
  volumeUsd: number | null;
  maxLeverage: number | null;
  fundingRate: number | null;
  nextFundingMs: number | null;
  openInterestUsd: number | null;
  takerFee: number | null;
  minOrderUsd: number | null;
}

export interface PerpPosition {
  marketId: string;
  symbol: string;
  side: Side;
  size: number;
  notionalUsd: number;
  marginUsd: number;
  leverage: number;
  entryPrice: number;
  markPrice: number | null;
  liquidationPrice: number;
  pnlUsd: number;
}

export interface PerpsAccount {
  accountId: string;
  equityUsd: number;
  availableUsd: number;
  unrealizedPnlUsd: number;
  positions: PerpPosition[];
  orders: Array<{ marketId: string; symbol: string; orderId: string; side: Side; size: number; price?: number | null }>;
  builderApproved: boolean;
  needsApproval?: boolean;
}

export interface OrderPreview {
  size: string;
  sizeUnits: number;
  entryPrice: number;
  liquidationPrice: number | null;
  notionalUsd: number;
  marginUsd: number;
  feeUsd: number;
  builderFeeUsd: number;
}

export interface PerpBar {
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

export type PerpTxRequest =
  | { kind: "create-account" }
  | { kind: "deposit"; usd: number }
  | { kind: "open"; marketId: string; side: Side; marginUsd: number; leverage: number; slippage?: number; depositUsd?: number }
  | { kind: "close"; marketId: string; fraction?: number };

const BASE = "/ext/perps";

export const perpPaths = {
  markets: () => `${BASE}/markets`,
  account: (address: string) => `${BASE}/account?address=${address.toLowerCase()}`,
  candles: (marketId: string, resolution: string, fromMs: number, toMs: number) =>
    `${BASE}/candles?${new URLSearchParams({ market: marketId, resolution, from: String(Math.floor(fromMs)), to: String(Math.ceil(toMs)) })}`,
  preview: (marketId: string, side: Side, marginUsd: number, leverage: number) =>
    `${BASE}/preview?${new URLSearchParams({ market: marketId, side, margin: String(marginUsd), leverage: String(leverage) })}`,
};

/** the Trade API's full perps list, read only for each market's picture (the ext list has none). */
export const perpLogoPath = "/perps?l=1";

/** marketId -> picture, from the Trade API's perps list. */
export function logoMap(body: { items?: Array<{ id: string; logo?: string | null }> } | undefined): Map<string, string> {
  const m = new Map<string, string>();
  for (const it of body?.items ?? []) if (it.id && it.logo) m.set(it.id.toLowerCase(), it.logo);
  return m;
}

export async function fetchPerpMarkets(): Promise<PerpMarket[]> {
  return (await tradeGet<{ items: PerpMarket[] }>(perpPaths.markets())).items;
}

export async function fetchPerpsAccount(address: string): Promise<PerpsAccount | null> {
  return (await tradeGet<{ account: PerpsAccount | null }>(perpPaths.account(address))).account;
}

export async function previewOpen(marketId: string, side: Side, marginUsd: number, leverage: number): Promise<OrderPreview> {
  return tradeGet<OrderPreview>(perpPaths.preview(marketId, side, marginUsd, leverage));
}

/** The unsigned order for this wallet as a Transaction (sender set by the API), for Rare Wallet. */
export async function buildPerpTx(address: string, req: PerpTxRequest, call = tradeCall): Promise<Transaction> {
  const r = await call<{ tx?: string; error?: string }>("POST", `${BASE}/tx`, { address: address.toLowerCase(), req });
  if (r.status !== 200 || !r.body?.tx) throw errorOf(r.status, r.body ?? { error: "Could not build the transaction." });
  const tx = Transaction.from(r.body.tx);
  tx.setSenderIfNotSet(address);
  return tx;
}

/** Candle resolutions the perps chart offers, with the window each one loads. */
export const PERP_TIMEFRAMES = [
  { tf: "5m", res: "5", spanMs: 2 * 86_400_000 },
  { tf: "15m", res: "15", spanMs: 6 * 86_400_000 },
  { tf: "1h", res: "60", spanMs: 21 * 86_400_000 },
  { tf: "4h", res: "240", spanMs: 90 * 86_400_000 },
  { tf: "1d", res: "1D", spanMs: 400 * 86_400_000 },
] as const;

/** What an order does about money: nothing, top up the perps account from the wallet first, or stop. */
export function fundingPlan(account: { availableUsd: number } | null | undefined, marginUsd: number, feeUsd: number, walletUsdc: number | null) {
  const available = account?.availableUsd ?? 0;
  const shortBy = Math.max(0, Math.ceil((marginUsd * 1.02 + feeUsd - available) * 100) / 100);
  const blocked = shortBy > 0 && walletUsdc !== null && walletUsdc + 1e-9 < shortBy;
  return { shortBy, blocked, needsAccount: account === null };
}

/** The open request for a ticket, with the top-up folded in when the account is short. */
export function openRequest(p: { marketId: string; side: Side; marginUsd: number; leverage: number; shortBy: number }): PerpTxRequest {
  return {
    kind: "open",
    marketId: p.marketId,
    side: p.side,
    marginUsd: p.marginUsd,
    leverage: p.leverage,
    slippage: 0.01,
    ...(p.shortBy > 0 ? { depositUsd: p.shortBy } : {}),
  };
}

/** Funding as a percent over `hours`. */
export function fundingPct(rate: number, hours = 1) {
  const pct = rate * hours * 100;
  const abs = Math.abs(pct);
  const digits = abs >= 1 ? 2 : abs >= 0.01 ? 4 : abs >= 0.001 ? 5 : 6;
  return `${pct.toFixed(digits)}%`;
}

/** A perps price: 2 decimals from $100, 4 from $1, else 5 significant digits. */
export const perpPx = (v: number | null | undefined) =>
  v == null || !Number.isFinite(v) || v <= 0
    ? ""
    : `$${v.toLocaleString("en-US", Math.abs(v) >= 100 ? { minimumFractionDigits: 2, maximumFractionDigits: 2 } : Math.abs(v) >= 1 ? { maximumFractionDigits: 4 } : { maximumSignificantDigits: 5 })}`;

/** Price at which a position has made (or lost) `pct` of its margin. */
export function priceAtReturn(p: { side: Side; entryPrice: number; size: number; marginUsd: number }, pct: number) {
  if (!p.size) return null;
  const move = ((pct / 100) * p.marginUsd) / p.size;
  const v = p.side === "long" ? p.entryPrice + move : p.entryPrice - move;
  return v > 0 ? v : null;
}

/** A liquidation price, or plain words when it is effectively unreachable (a 1x long). */
export function liqText(liq: number | null, entry: number) {
  if (liq == null || !Number.isFinite(liq)) return "";
  if (liq <= entry * 1e-3) return "Near $0";
  return perpPx(liq);
}
