// A coin's page in the Trade API's layout: header with price and change, the chart with its time ranges,
// the stat line, recent trades and holders (LB/RB), and the Buy/Sell panel on the right.
// LT/RT step the amount, X flips Buy and Sell, A on the panel's button opens the review.

import { useRef, useState } from "react";
import { Button, Glyph, HeadRow, Panel, Row, Tabs, numText } from "../ui";
import { AlertSheet } from "../notify";
import { SUI_TYPE, fromBase, minOut, normalizeType, baseToNumber } from "./amount";
import { change24, invalidate, tokenPaths, useApi, type Candle, type HoldersPage, type TokenDetail, type TokenRow, type Trade } from "./api";
import { ChartArea } from "./ChartArea";
import { age, shortAddr, usdPrice } from "./format";
import { H, inOrderPanel, useFocusArea, useLandingFocus, usePageButtons, usePageHints } from "./nav";
import { CoinLogo, Line, Num, Quiet, StatLine, TfChips, cn } from "./parts";
import { ReviewSheet } from "./ReviewSheet";
import { buildSwap, quoteSwap, venueName } from "./swap";
import { useTicket, type Coin } from "./ticket";
import { TradePanel, suiCoin, type TradeSide } from "./TradePanel";
import { useTradeAccount } from "./wallet";

const TIMEFRAMES = [
  { value: "1m", label: "1m" },
  { value: "5m", label: "5m" },
  { value: "1h", label: "1h" },
  { value: "1d", label: "1d" },
] as const;
type Tf = (typeof TIMEFRAMES)[number]["value"];
type Tab = "trades" | "holders";

export function TokenPage({ coinType, initialSide, frame, active }: { coinType: string; initialSide?: TradeSide; frame: number; active: boolean }) {
  const type = normalizeType(coinType);
  const root = useRef<HTMLDivElement>(null);
  const { account } = useTradeAccount();
  const [side, setSide] = useState<TradeSide>(initialSide ?? "buy");
  const [tf, setTf] = useState<Tf>("5m");
  const [tab, setTab] = useState<Tab>("trades");
  const [reviewing, setReviewing] = useState(false);
  const [alerting, setAlerting] = useState(false);

  const token = useApi<TokenDetail>(tokenPaths.token(type), 15_000, active ? 15_000 : 0);
  const sui = useApi<TokenRow>(tokenPaths.lean(SUI_TYPE), 30_000, active ? 30_000 : 0);
  const candles = useApi<{ candles: Candle[] }>(tokenPaths.candles(type, tf), 15_000, active ? 15_000 : 0);
  const trades = useApi<{ items: Trade[] }>(tokenPaths.trades(type), 10_000, active && tab === "trades" ? 10_000 : 0);
  const holders = useApi<HoldersPage>(tab === "holders" ? tokenPaths.holders(type, 20) : null, 60_000);
  const t = token.data;

  const coin: Coin | null = t ? { coinType: type, symbol: t.symbol, decimals: t.decimals, logo: t.logo, priceUsd: t.priceUsd } : null;
  const suiC = suiCoin(sui.data?.priceUsd, sui.data?.logo);
  const from = side === "buy" ? suiC : coin;
  const to = side === "buy" ? coin : suiC;
  const ticket = useTicket({ from, to, owner: account?.address ?? null, mode: side === "buy" ? "units" : "pct", initial: side === "buy" ? 5 : 50 });

  useLandingFocus(root, active && !reviewing && !alerting, !!t);
  const area = useFocusArea(root);
  usePageButtons(frame, (b) => {
    if (reviewing || alerting) return false;
    // LT and RT step the amount only while the order panel has focus.
    if ((b === "lt" || b === "rt") && inOrderPanel()) {
      ticket.bump(b === "rt" ? 1 : -1);
      return true;
    }
    if (b === "x") {
      setSide((s) => (s === "buy" ? "sell" : "buy"));
      return true;
    }
    // Elsewhere LT/RT switch Trades and Holders (LB/RB always stay the launcher's tabs).
    if (b === "lt" || b === "rt") {
      setTab((x) => (x === "trades" ? "holders" : "trades"));
      return true;
    }
    return false;
  });
  const flip = { b: "x" as const, label: side === "buy" ? "Sell" : "Buy" };
  usePageHints(
    active,
    area === "order"
      ? [H.select, { b: "lt", label: "Less" }, { b: "rt", label: "More" }, flip, H.search, H.back]
      : area === "chart"
        ? [{ b: "a", label: "Control chart" }, { b: "select", label: "Chart mode" }, flip, H.search, { b: "ltrt", label: tab === "trades" ? "Holders" : "Trades" }, H.back]
        : [H.select, flip, H.search, { b: "ltrt", label: tab === "trades" ? "Holders" : "Trades" }, H.back],
  );

  if (!t)
    return (
      <div ref={root} className="h-full">
        {token.error && <Quiet className="pt-20">{token.error === "Token not indexed yet" ? "This coin is not in the market data yet." : "Could not load this coin."}</Quiet>}
      </div>
    );

  const fmt = (n: number) => usdPrice(n) || "$0";
  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      <div className="grid grid-cols-[minmax(0,1fr)_24rem] gap-[1.5rem] px-[2.5rem]">
        <div className="min-w-0">
          <div className="flex items-center gap-[1rem]">
            <CoinLogo src={t.logo} symbol={t.symbol} size="3.5rem" />
            <div className="min-w-0">
              <h1 className="flex items-baseline gap-[0.5rem] text-title font-semibold">
                <span className="truncate">{t.name}</span>
                <span className="text-body font-medium text-secondary">{t.symbol}</span>
              </h1>
              <p className="flex items-baseline gap-[0.75rem]">
                <Num kind="price" value={t.priceUsd} className="text-display font-semibold" live />
                <Num kind="change" value={change24(t)} className="text-body" />
                <span className="text-small text-secondary">24h</span>
              </p>
            </div>
            <div className="ml-auto flex items-center gap-[0.75rem]">
              {t.priceUsd != null && t.priceUsd > 0 && (
                <Button size="sm" hint="Set a price alert" onClick={() => setAlerting(true)}>
                  Price alert
                </Button>
              )}
              <TfChips items={[...TIMEFRAMES]} value={tf} onChange={setTf} group={`tr-tf-${frame}`} />
            </div>
          </div>

          <ChartArea className="mt-[1rem] h-[19rem]" rows={candles.data?.candles ?? []} format={fmt} fitKey={`${type}:${tf}`} tfs={TIMEFRAMES.map((x) => x.value)} tf={tf} onTf={(v) => setTf(v as Tf)} />

          <Panel className="mt-[1rem] px-[1.25rem] py-[0.75rem]">
            <StatLine
              items={[
                { label: "Market cap", value: t.mcap ? <Num kind="market" value={t.mcap} /> : null },
                { label: "24h volume", value: t.volume24h ? <Num kind="market" value={t.volume24h} /> : null },
                { label: "Liquidity", value: t.liquidity ? <Num kind="market" value={t.liquidity} /> : null },
                { label: "Holders", value: t.holders ? <Num kind="count" value={t.holders} /> : null },
                { label: "Buys / sells 24h", value: t.buys24h != null && t.sells24h != null ? `${numText("count", t.buys24h)} / ${numText("count", t.sells24h)}` : null },
                { label: "Age", value: t.createdMs ? age(t.createdMs) : null },
              ]}
            />
          </Panel>

          <div className="mt-[1.25rem]">
            <div className="flex h-[2.25rem] items-stretch gap-[1rem] px-[0.25rem]">
              <span className="flex items-center">
                <Glyph b="lt" size="sm" />
              </span>
              <Tabs<Tab>
                items={[
                  { value: "trades", label: "Trades" },
                  { value: "holders", label: "Holders" },
                ]}
                value={tab}
              />
              <span className="flex items-center">
                <Glyph b="rt" size="sm" />
              </span>
            </div>
            <Panel className="mt-[0.5rem] p-[0.75rem]">{tab === "trades" ? <TradesTable rows={trades.data?.items ?? []} symbol={t.symbol} error={trades.error} frame={frame} /> : <HoldersTable page={holders.data} error={holders.error} frame={frame} />}</Panel>
          </div>
        </div>

        <div className="self-start">
          <TradePanel side={side} onSide={setSide} ticket={ticket} from={from} to={to} account={account} onReview={() => setReviewing(true)} />
        </div>
      </div>

      {alerting && t.priceUsd != null && <AlertSheet coinType={type} symbol={t.symbol} priceUsd={t.priceUsd} addr={account?.address} onClose={() => setAlerting(false)} />}

      {reviewing && from && to && (
        <ReviewSheet
          title={side === "buy" ? `Buy ${t.symbol}` : `Sell ${t.symbol}`}
          sub={t.name}
          account={account}
          action={side === "buy" ? "Confirm buy" : "Confirm sell"}
          sponsor="trade"
          tone={side === "buy" ? "buy" : "sell"}
          build={async (sender, o) => {
            // Quotes are bound to the wallet they were asked for: a quote without one is asked again.
            const q = ticket.quote && ticket.quote.expiresAt - Date.now() > 8_000 ? ticket.quote : await quoteSwap({ coinIn: from.coinType, coinOut: to.coinType, amountIn: ticket.amountIn, owner: sender });
            return (await buildSwap(q, sender, undefined, undefined, { sponsored: o.sponsored })).tx;
          }}
          onClose={() => setReviewing(false)}
          onSent={() => invalidate("/ext/portfolio")}
        >
          <Line label={side === "buy" ? "You pay" : "You sell"}>{`${numText("amount", baseToNumber(ticket.amountIn, from.decimals))} ${from.symbol}`}</Line>
          {ticket.quote && (
            <>
              <Line label="You get about" strong>{`${numText("amount", baseToNumber(ticket.quote.amountOut, to.decimals))} ${to.symbol}`}</Line>
              <Line label="Minimum received">{`${numText("amount", Number(fromBase(minOut(ticket.quote.amountOut), to.decimals)))} ${to.symbol}`}</Line>
              <Line label="Price">{usdPrice(t.priceUsd)}</Line>
              <Line label="Route">{`${ticket.quote.hops} ${ticket.quote.hops === 1 ? "hop" : "hops"}${ticket.quote.venues.length ? ` via ${[...new Set(ticket.quote.venues.map(venueName))].slice(0, 3).join(", ")}` : ""}`}</Line>
              <Line label="Fee">{`${(ticket.quote.feeBps / 100).toFixed(2)}%`}</Line>
              <Line label="Slippage">1%</Line>
            </>
          )}
        </ReviewSheet>
      )}
    </div>
  );
}

const TRADE_COLS = "grid-cols-[4rem_4rem_minmax(0,1fr)_7rem_7rem_8rem] text-right";

export function TradesTable({ rows, symbol, error, frame }: { rows: Trade[]; symbol: string; error: string | null; frame: number }) {
  if (!rows.length) return <Quiet>{error ? "Couldn't load trades." : ""}</Quiet>;
  return (
    <>
      <HeadRow cols={TRADE_COLS}>
        <span className="text-left">Age</span>
        <span className="text-left">Side</span>
        <span>Amount</span>
        <span>Value</span>
        <span>Price</span>
        <span>Trader</span>
      </HeadRow>
      <div data-nav-group={`tr-trades-${frame}`}>
        {rows.slice(0, 30).map((r) => (
          <Row key={r.id} cols={TRADE_COLS} className="cv-row h-[2.75rem]">
            <span className="text-left text-secondary">{age(r.timeMs)}</span>
            <span className={cn("text-left font-medium", r.buy ? "text-positive" : "text-negative")}>{r.buy ? "Buy" : "Sell"}</span>
            <span className="tabular-nums">{`${numText("amount", r.amount)} ${symbol}`}</span>
            <Num kind="money" value={r.usd} />
            <span className="tabular-nums text-secondary">{usdPrice(r.priceUsd)}</span>
            <span className="num text-secondary">{shortAddr(r.trader)}</span>
          </Row>
        ))}
      </div>
    </>
  );
}

const HOLDER_COLS = "grid-cols-[2.5rem_minmax(0,1fr)_8rem_7rem_5rem] text-right";

export function HoldersTable({ page, error, frame }: { page: HoldersPage | undefined; error: string | null; frame: number }) {
  if (!page?.holders.length) return <Quiet>{error ? "Couldn't load holders." : page ? "No holders seen yet." : ""}</Quiet>;
  return (
    <>
      <HeadRow cols={HOLDER_COLS}>
        <span className="text-left">#</span>
        <span className="text-left">{page.count ? `${numText("count", page.count)} holders` : "Holder"}</span>
        <span>Amount</span>
        <span>Value</span>
        <span>Share</span>
      </HeadRow>
      <div data-nav-group={`tr-holders-${frame}`}>
        {page.holders.map((h, i) => (
          <Row key={h.address} cols={HOLDER_COLS} className="cv-row h-[2.75rem]">
            <span className="text-left tabular-nums text-secondary">{i + 1}</span>
            <span className="flex min-w-0 items-center gap-[0.5rem] text-left">
              <span className="num truncate">{shortAddr(h.address)}</span>
              {h.kind !== "wallet" && <span className="text-label text-secondary">{h.venue ? venueName(h.venue) : h.kind === "pool" ? "Pool" : h.kind}</span>}
            </span>
            <span className="tabular-nums">{numText("amount", h.amount)}</span>
            <Num kind="money" value={h.valueUsd} compact />
            <span className="tabular-nums">{h.pctSupply != null ? numText("share", h.pctSupply) : ""}</span>
          </Row>
        ))}
      </div>
    </>
  );
}
