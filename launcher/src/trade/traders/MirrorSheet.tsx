// Mirror one trade: the same swap at this account's size, through the Trade review sheet (router
// quote, unsigned build, gRPC dry run) and then Rare Wallet's own review. Nothing signs here.

import { useState } from "react";
import type { RareAccount } from "../../wallet";
import { numText } from "../../ui";
import { SUI_TYPE, USDC_TYPE, baseToNumber, fromBase, minOut, normalizeType } from "../amount";
import { cached, invalidate, load, tokenPaths, type TokenRow } from "../api";
import { coinBalance } from "../chain";
import { Line } from "../parts";
import { ReviewSheet } from "../ReviewSheet";
import { buildSwap, quoteSwap, type SwapQuote } from "../swap";
import type { TokenChip, TraderTrade } from "./api";
import { mirrorPlan, type MirrorPlan, type MirrorSize } from "./mirror";

/** Decimals and symbol for a coin: SUI and USDC known, anything else from the coin's page. */
async function coinInfo(coinType: string): Promise<{ decimals: number; symbol: string }> {
  const t = normalizeType(coinType);
  if (t === SUI_TYPE) return { decimals: 9, symbol: "SUI" };
  if (t === USDC_TYPE) return { decimals: 6, symbol: "USDC" };
  const path = tokenPaths.lean(t);
  const row = cached<TokenRow>(path) ?? (await load<TokenRow>(path));
  if (!row || typeof row.decimals !== "number") throw new Error("Could not read this coin.");
  return { decimals: row.decimals, symbol: row.symbol };
}

export function MirrorSheet({ trade, token, traderName, size, account, onClose }: { trade: TraderTrade; token: TokenChip | null; traderName: string; size: MirrorSize; account: RareAccount | null; onClose: () => void }) {
  const [plan, setPlan] = useState<Extract<MirrorPlan, { ok: true }> | null>(null);
  const [quote, setQuote] = useState<SwapQuote | null>(null);
  const [names, setNames] = useState<{ inSym: string; outSym: string; outDec: number } | null>(null);
  const sym = token?.symbol ?? trade.coin.split("::").pop() ?? "coin";

  return (
    <ReviewSheet
      title={`Mirror ${trade.buy ? "buy" : "sell"} of ${sym}`}
      sub={`${traderName} ${trade.buy ? "bought" : "sold"} ${numText("money", trade.usd)}. Your size: ${trade.buy ? `${size.buySui} SUI` : `${size.sellPct}% of your ${sym}`}.`}
      account={account}
      action={trade.buy ? "Confirm buy" : "Confirm sell"}
      tone={trade.buy ? "buy" : "sell"}
      build={async (sender) => {
        const coin = await coinInfo(trade.coin);
        const held = trade.buy ? null : await coinBalance(sender, trade.coin);
        const p = mirrorPlan(trade, size, coin.decimals, held);
        if (!p.ok) throw new Error(p.reason);
        const [inInfo, outInfo] = await Promise.all([coinInfo(p.coinIn), coinInfo(p.coinOut)]);
        setPlan(p);
        setNames({ inSym: inInfo.symbol, outSym: outInfo.symbol, outDec: outInfo.decimals });
        const q = await quoteSwap({ coinIn: p.coinIn, coinOut: p.coinOut, amountIn: p.amountIn, owner: sender });
        setQuote(q);
        return (await buildSwap(q, sender)).tx;
      }}
      onClose={onClose}
      onSent={() => invalidate("/ext/portfolio")}
    >
      <Line label="Trader">{`${trade.buy ? "Bought" : "Sold"} ${numText("money", trade.usd)} of ${sym}`}</Line>
      {plan && names && <Line label="You pay">{`${numText("amount", baseToNumber(plan.amountIn, plan.decimalsIn))} ${names.inSym}`}</Line>}
      {quote && names && (
        <>
          <Line label="You get about" strong>{`${numText("amount", baseToNumber(quote.amountOut, names.outDec))} ${names.outSym}`}</Line>
          <Line label="Minimum received">{`${numText("amount", Number(fromBase(minOut(quote.amountOut), names.outDec)))} ${names.outSym}`}</Line>
          <Line label="Fee">{`${(quote.feeBps / 100).toFixed(2)}%`}</Line>
          <Line label="Slippage">1%</Line>
        </>
      )}
    </ReviewSheet>
  );
}
