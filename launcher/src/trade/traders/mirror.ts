// Mirror a trader's trade: the same direction on the same coin, at the size this account chose
// (SUI per buy, a share of what it holds per sell). The swap goes through the normal Trade flow:
// router quote, unsigned build, gRPC dry run in the review sheet, then Rare Wallet's own review.
// Pure planning here, unit tested; nothing signs or sends.

import { create } from "zustand";
import { SUI_TYPE, USDC_TYPE, normalizeType, pctOf, spendable, toBase } from "../amount";
import type { TraderTrade } from "./api";

export interface MirrorSize {
  /** SUI spent on each mirrored buy */
  buySui: number;
  /** share of the holding sold on each mirrored sell, percent */
  sellPct: number;
}

export const DEFAULT_SIZE: MirrorSize = { buySui: 5, sellPct: 100 };
export const BUY_SIZES = [1, 5, 10, 25, 50];
export const SELL_SIZES = [25, 50, 100];

export type MirrorPlan =
  | { ok: true; side: "buy" | "sell"; coinIn: string; coinOut: string; amountIn: bigint; decimalsIn: number }
  | { ok: false; reason: string };

/**
 * The swap that mirrors `trade` at `size`.
 * - Buy a coin: pay `buySui` SUI for it. Buying SUI itself pays USDC worth `buySui` SUI.
 * - Sell a coin: sell `sellPct` of what `held` says the account has, for SUI (SUI sells for USDC).
 * `held` is the account's balance of the trade's coin in base units (null when unknown).
 */
export function mirrorPlan(trade: Pick<TraderTrade, "buy" | "coin" | "priceUsd">, size: MirrorSize, coinDecimals: number, held: bigint | null): MirrorPlan {
  const coin = normalizeType(trade.coin);
  const isSui = coin === SUI_TYPE;
  if (trade.buy) {
    if (!(size.buySui > 0)) return { ok: false, reason: "Choose a size first." };
    if (isSui) {
      if (!(trade.priceUsd > 0)) return { ok: false, reason: "No price for SUI right now." };
      return { ok: true, side: "buy", coinIn: USDC_TYPE, coinOut: SUI_TYPE, amountIn: toBase((size.buySui * trade.priceUsd).toFixed(6), 6), decimalsIn: 6 };
    }
    return { ok: true, side: "buy", coinIn: SUI_TYPE, coinOut: coin, amountIn: toBase(size.buySui, 9), decimalsIn: 9 };
  }
  if (held == null) return { ok: false, reason: "Checking your balance." };
  const usable = spendable(held, coin);
  const amountIn = pctOf(usable, size.sellPct);
  if (amountIn <= 0n) return { ok: false, reason: "You hold none of this coin." };
  return { ok: true, side: "sell", coinIn: coin, coinOut: isSui ? USDC_TYPE : SUI_TYPE, amountIn, decimalsIn: coinDecimals };
}

const sizeKey = (account: string | null) => `rare.traders.mirror.${account ? account.toLowerCase() : "device"}`;

export function readSize(account: string | null): MirrorSize {
  try {
    const v = JSON.parse(localStorage.getItem(sizeKey(account)) ?? "null") as Partial<MirrorSize> | null;
    return { buySui: Number(v?.buySui) > 0 ? Number(v!.buySui) : DEFAULT_SIZE.buySui, sellPct: Number(v?.sellPct) > 0 ? Math.min(100, Number(v!.sellPct)) : DEFAULT_SIZE.sellPct };
  } catch {
    return DEFAULT_SIZE;
  }
}

/** The account's mirror size, kept on this device. */
export const useMirrorSize = create<{ account: string | null; size: MirrorSize; use: (a: string | null) => void; set: (s: Partial<MirrorSize>) => void }>((set, get) => ({
  account: null,
  size: DEFAULT_SIZE,
  use: (a) => {
    const account = a?.toLowerCase() ?? null;
    if (account !== get().account || get().size === DEFAULT_SIZE) set({ account, size: readSize(account) });
  },
  set: (s) => {
    const size = { ...get().size, ...s };
    try {
      localStorage.setItem(sizeKey(get().account), JSON.stringify(size));
    } catch {
      /* kept for this session */
    }
    set({ size });
  },
}));
