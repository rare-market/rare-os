// The big Buy/Sell panel on a coin's page (the Trade API's TradePanel, made for the pad): the amount in the
// middle with LT/RT stepping it, a row of presets, what you get from the live router quote, and
// one button that opens the review. X flips Buy and Sell.

import { Panel } from "../ui";
import type { RareAccount } from "../wallet";
import { baseToNumber, fromBase, minOut, normalizeType, SUI_TYPE } from "./amount";
import { numText } from "../ui";
import { Action, Chip, Line, SideSwitch, Stepper, cn } from "./parts";
import { venueName } from "./swap";
import type { Coin, Ticket } from "./ticket";

export type TradeSide = "buy" | "sell";

/** The amount's own words: "10 SUI" to buy, "50%" or "All" of the coin to sell. */
export function amountLabel(t: Ticket, side: TradeSide, from: Coin | null) {
  if (!from) return "";
  if (side === "sell") return t.value >= 100 ? "All" : numText("share", t.value, { decimals: 0 });
  return `${trim(t.value)} ${from.symbol}`;
}
const trim = (v: number) => Number(v.toPrecision(6)).toLocaleString("en-US", { maximumFractionDigits: 6 });

export function TradePanel({
  side,
  onSide,
  ticket: t,
  from,
  to,
  account,
  onReview,
}: {
  side: TradeSide;
  onSide: (s: TradeSide) => void;
  ticket: Ticket;
  from: Coin | null;
  to: Coin | null;
  account: RareAccount | null;
  onReview: () => void;
}) {
  const coinIn = from ? baseToNumber(t.amountIn, from.decimals) : 0;
  const usdIn = from?.priceUsd ? coinIn * from.priceUsd : null;
  const out = t.quote && to ? baseToNumber(t.quote.amountOut, to.decimals) : null;
  const nothing = side === "sell" && t.balance === 0n;
  const blocked = !from || !to || t.amountIn <= 0n || t.short || nothing || !t.quote;
  const why = !account ? "Add a wallet first" : nothing ? `No ${from?.symbol ?? ""} to sell` : t.short ? "Not enough balance" : t.quoteError ? t.quoteError : undefined;

  return (
    <Panel className="flex flex-col gap-[1rem] p-[1.25rem]">
      <div data-area="order" className="contents">
      <SideSwitch<TradeSide>
        items={[
          { value: "buy", label: "Buy", tone: "buy" },
          { value: "sell", label: "Sell", tone: "sell" },
        ]}
        value={side}
        onChange={onSide}
      />

      <div>
        <p className="text-small text-secondary">{side === "buy" ? "You pay" : "You sell"}</p>
        <div className="mt-[0.25rem]">
          <Stepper group="tr-amount" label="amount" onStep={(d) => t.bump(d)}>
            <p className="truncate text-center text-display font-semibold tabular-nums">{amountLabel(t, side, from)}</p>
          </Stepper>
        </div>
        <p className="mt-[0.25rem] h-[1.25rem] text-center text-small text-secondary tabular-nums">
          {side === "sell" && from ? (t.amountIn > 0n ? `${numText("amount", coinIn)} ${from.symbol}` : `of your ${from.symbol}`) : ""}
          {side === "sell" && usdIn != null && t.amountIn > 0n ? ", " : ""}
          {usdIn != null && t.amountIn > 0n ? `about ${numText("money", usdIn)}` : ""}
        </p>
      </div>

      <div data-nav-group="tr-presets" data-nav-remember className="flex gap-[0.5rem]">
        {t.presets.map((v) => (
          <Chip key={v} on={Math.abs(t.value - v) < 1e-9} onClick={() => t.set(v)}>
            {side === "sell" ? (v >= 100 ? "Max" : `${v}%`) : trim(v)}
          </Chip>
        ))}
        {side === "buy" && t.max !== Infinity && t.max > 0 && (
          <Chip on={Math.abs(t.value - t.max) < 1e-9} onClick={() => t.set(Number(t.max.toFixed(4)))}>
            Max
          </Chip>
        )}
      </div>

      <div>
        <Line label="You get about">
          <span className={cn("font-semibold", !out && "text-secondary")}>
            {out != null && to ? `${numText("amount", out)} ${to.symbol}` : t.quoting ? "Getting a quote" : t.quoteError ? "No route for this amount" : ""}
          </span>
        </Line>
        {t.quote && to && (
          <>
            <Line label="Minimum received">{`${numText("amount", Number(fromBase(minOut(t.quote.amountOut), to.decimals)))} ${to.symbol}`}</Line>
            <Line label="Route">{`${t.quote.hops} ${t.quote.hops === 1 ? "hop" : "hops"}${t.quote.venues.length ? ` via ${[...new Set(t.quote.venues.map(venueName))].slice(0, 3).join(", ")}` : ""}`}</Line>
            <Line label="Fee">{`${(t.quote.feeBps / 100).toFixed(2)}%`}</Line>
          </>
        )}
        <Line label="Balance">
          {from && t.balance != null ? `${numText("amount", baseToNumber(t.balance, from.decimals))} ${from.symbol}` : account ? "" : "No wallet"}
        </Line>
      </div>

      <Action def tone={side === "buy" ? "buy" : "sell"} disabled={blocked} onClick={onReview} hint={blocked ? why : "Review"} className="w-full">
        {side === "buy" ? `Review buy${to ? ` ${to.symbol}` : ""}` : `Review sell${from ? ` ${from.symbol}` : ""}`}
      </Action>
      {why && !t.quoting && <p className="-mt-[0.5rem] text-center text-label text-secondary">{why}</p>}
      </div>
    </Panel>
  );
}

/** SUI as a Coin, from the lean token answer. */
export function suiCoin(priceUsd: number | null | undefined, logo?: string | null): Coin {
  return { coinType: SUI_TYPE, symbol: "SUI", decimals: 9, logo: logo ?? null, priceUsd: priceUsd ?? null };
}

export const isSui = (t: string) => normalizeType(t) === SUI_TYPE;
