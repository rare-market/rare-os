// A swap ticket for the pad: the amount (in coin units, or a share of the balance), the balance it
// draws from (gRPC), LT/RT stepping, presets, and a live quote that follows the amount.

import { useEffect, useMemo, useState } from "react";
import { PCT_LADDER, SUI_LADDER, USD_LADDER, baseToNumber, pctOf, spendable, step, toBase, SUI_TYPE, normalizeType } from "./amount";
import { useAsync } from "./api";
import { coinBalance } from "./chain";
import { quoteSwap, type SwapQuote } from "./swap";

export interface Coin {
  coinType: string;
  symbol: string;
  decimals: number;
  logo: string | null;
  priceUsd: number | null;
}

export type AmountMode = "units" | "pct";

/** Ladders by what the amount is counted in. */
export function ladderFor(mode: AmountMode, coin: Coin | null): number[] {
  if (mode === "pct") return PCT_LADDER;
  if (!coin || normalizeType(coin.coinType) === SUI_TYPE) return SUI_LADDER;
  // Dollar coins step in dollars; anything else in round counts of the coin.
  if ((coin.priceUsd ?? 0) > 0.5 && (coin.priceUsd ?? 0) < 2) return USD_LADDER;
  const unit = coin.priceUsd && coin.priceUsd > 0 ? 1 / coin.priceUsd : 1;
  // About $1, $5, $10 ... worth of the coin, rounded to 2 significant digits.
  return USD_LADDER.map((usd) => Number((usd * unit).toPrecision(2)));
}

export function presetsFor(mode: AmountMode, coin: Coin | null): number[] {
  if (mode === "pct") return [25, 50, 75, 100];
  const l = ladderFor(mode, coin);
  if (l === SUI_LADDER) return [1, 5, 10, 25, 50];
  return [l[2], l[3], l[5], l[7], l[9]].filter((v) => v != null);
}

export function useTicket(p: { from: Coin | null; to: Coin | null; owner: string | null; mode: AmountMode; initial: number }) {
  const { from, to, owner, mode } = p;
  const [value, setValue] = useState(p.initial);
  // A new coin or mode starts over at its default.
  const sig = `${from?.coinType}|${mode}`;
  useEffect(() => setValue(p.initial), [sig]); // eslint-disable-line react-hooks/exhaustive-deps

  const bal = useAsync(owner && from ? `bal:${owner}:${from.coinType}` : null, () => coinBalance(owner!, from!.coinType), 20_000);
  const balance = bal.data ?? null;
  const usable = balance != null && from ? spendable(balance, normalizeType(from.coinType)) : null;
  const max = mode === "pct" ? 100 : usable != null && from ? baseToNumber(usable, from.decimals) : Infinity;

  const amountIn = useMemo(() => {
    if (!from) return 0n;
    if (mode === "pct") return usable != null ? pctOf(usable, value) : 0n;
    return toBase(value, from.decimals);
  }, [from, mode, value, usable]);

  const short = mode === "units" && usable != null && amountIn > usable;
  const ladder = ladderFor(mode, from);

  // Quote 300 ms after the amount settles.
  const [settled, setSettled] = useState<string | null>(null);
  const qKey = from && to && amountIn > 0n ? `${from.coinType}|${to.coinType}|${amountIn}|${owner ?? ""}` : null;
  useEffect(() => {
    const t = setTimeout(() => setSettled(qKey), 300);
    return () => clearTimeout(t);
  }, [qKey]);
  const quote = useAsync<SwapQuote>(settled && settled === qKey ? settled : null, () => quoteSwap({ coinIn: from!.coinType, coinOut: to!.coinType, amountIn, owner }), 15_000);

  return {
    value,
    amountIn,
    balance,
    usable,
    max,
    short,
    ladder,
    presets: presetsFor(mode, from),
    quote: quote.data && qKey === settled ? quote.data : undefined,
    quoting: !!qKey && (qKey !== settled || quote.loading) && !quote.data,
    quoteError: quote.error,
    set: (v: number) => setValue(v),
    /** LT / RT. The balance caps the ladder once it is known and not empty. */
    bump: (dir: 1 | -1) => setValue((v) => step(ladder, v, dir, max > 0 ? max : Infinity)),
  };
}

export type Ticket = ReturnType<typeof useTicket>;
