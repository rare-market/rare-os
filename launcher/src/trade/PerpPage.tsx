// A perps market, made for the pad: price and chart, the market's
// figures, your position with Close, and the order panel. LT/RT set the leverage, X flips Long and
// Short, the presets set the margin, A on the panel's button opens the review.

import { useEffect, useRef, useState } from "react";
import { Panel } from "../ui";
import { MARGIN_PRESETS, USDC_TYPE, baseToNumber, fmtLeverage, leverageStops, step } from "./amount";
import { invalidate, useApi, useAsync, usePerpLogos } from "./api";
import { coinBalance } from "./chain";
import { ChartArea } from "./ChartArea";
import { H, inOrderPanel, useFocusArea, useLandingFocus, usePageButtons, usePageHints } from "./nav";
import { Action, Chip, CoinLogo, Line, Num, Quiet, SideSwitch, Stepper, StatLine, TfChips, cn } from "./parts";
import { PERP_TIMEFRAMES, buildPerpTx, fundingPct, fundingPlan, liqText, openRequest, perpPaths, perpPx, previewOpen, type OrderPreview, type PerpBar, type PerpMarket, type PerpsAccount, type Side } from "./perps";
import { ReviewSheet } from "./ReviewSheet";
import { useTradeAccount } from "./wallet";
import { tradeGet } from "./http";
import { numText } from "../ui";

type Tf = (typeof PERP_TIMEFRAMES)[number]["tf"];

export function PerpPage({ marketId, frame, active }: { marketId: string; frame: number; active: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const { account } = useTradeAccount();
  const [side, setSide] = useState<Side>("long");
  const [margin, setMargin] = useState(10);
  const [lev, setLev] = useState(5);
  const [tf, setTf] = useState<Tf>("15m");
  const [sheet, setSheet] = useState<"open" | "close" | null>(null);

  const markets = useApi<{ items: PerpMarket[] }>(perpPaths.markets(), 5_000, active ? 5_000 : 0);
  const m = markets.data?.items.find((x) => x.marketId === marketId) ?? null;
  const span = PERP_TIMEFRAMES.find((x) => x.tf === tf)!;
  const bars = useAsync<PerpBar[]>(
    `${marketId}:${tf}`,
    async () => {
      const now = Date.now();
      return (await tradeGet<{ bars: PerpBar[] }>(perpPaths.candles(marketId, span.res, now - span.spanMs, now))).bars;
    },
    active ? 15_000 : 0,
  );
  const acct = useApi<{ account: PerpsAccount | null }>(account ? perpPaths.account(account.address) : null, 10_000, active ? 10_000 : 0);
  const usdc = useAsync(account ? `usdc:${account.address}` : null, async () => baseToNumber(await coinBalance(account!.address, USDC_TYPE), 6), 30_000);
  const position = acct.data?.account?.positions.find((p) => p.marketId === marketId) ?? null;

  const logo = usePerpLogos();
  const stops = leverageStops(m?.maxLeverage);
  const maxLev = stops[stops.length - 1];
  useEffect(() => {
    if (m && lev > maxLev) setLev(maxLev);
  }, [m, maxLev]); // eslint-disable-line react-hooks/exhaustive-deps

  // Preview 300 ms after the ticket settles.
  const [settled, setSettled] = useState("");
  const key = `${marketId}|${side}|${margin}|${lev}`;
  useEffect(() => {
    const t = setTimeout(() => setSettled(key), 300);
    return () => clearTimeout(t);
  }, [key]);
  const preview = useAsync<OrderPreview>(m && settled === key ? key : null, () => previewOpen(marketId, side, margin, lev));

  const plan = fundingPlan(acct.data ? acct.data.account : undefined, margin, preview.data?.feeUsd ?? 0, usdc.data ?? null);
  const minOrder = m?.minOrderUsd ?? 1;
  const tooSmall = margin * lev < minOrder;
  const blocked = !m || !preview.data || plan.blocked || tooSmall;
  const why = !account ? "Add a wallet first" : plan.needsAccount ? "One time, before your first order" : plan.blocked ? "Not enough USDC in your wallet" : tooSmall ? `Minimum order ${perpPx(minOrder)}` : preview.error ?? undefined;

  useLandingFocus(root, active && !sheet, !!m);
  const area = useFocusArea(root);
  usePageButtons(frame, (b) => {
    if (sheet) return false;
    // LT and RT set the leverage only while the order panel has focus.
    if ((b === "lt" || b === "rt") && inOrderPanel()) {
      setLev((v) => step(stops, v, b === "rt" ? 1 : -1, maxLev, 1));
      return true;
    }
    if (b === "x") {
      setSide((s) => (s === "long" ? "short" : "long"));
      return true;
    }
    return false;
  });
  const flip = { b: "x" as const, label: side === "long" ? "Short" : "Long" };
  usePageHints(
    active,
    area === "order"
      ? [H.select, { b: "lt", label: "Less leverage" }, { b: "rt", label: "More leverage" }, flip, H.search, H.back]
      : area === "chart"
        ? [{ b: "a", label: "Control chart" }, { b: "select", label: "Chart mode" }, flip, H.search, H.back]
        : [H.select, flip, H.search, H.back],
  );

  if (!m)
    return (
      <div ref={root} className="h-full">
        {markets.error && <Quiet className="pt-20">This market is unavailable right now.</Quiet>}
      </div>
    );

  const rows = (bars.data ?? []).map((b) => ({ ...b, time: Math.floor(b.time / 1000) }));
  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      <div className="grid grid-cols-[minmax(0,1fr)_24rem] gap-[1.5rem] px-[2.5rem]">
        <div className="min-w-0">
          <div className="flex items-center gap-[1rem]">
            <CoinLogo src={logo(m.marketId, m.symbol)} symbol={m.symbol} size="3.5rem" />
            <div className="min-w-0">
              <h1 className="flex items-baseline gap-[0.5rem] text-title font-semibold">
                {m.symbol}
                <span className="text-body font-medium text-secondary">{m.pair.replace(/USD$/, "/USD")} perp</span>
              </h1>
              <p className="flex items-baseline gap-[0.75rem]">
                <span className="text-display font-semibold tabular-nums">{perpPx(m.price)}</span>
                <Num kind="change" value={m.changePct} className="text-body" />
                <span className="text-small text-secondary">24h</span>
              </p>
            </div>
            <div className="ml-auto">
              <TfChips items={PERP_TIMEFRAMES.map((x) => ({ value: x.tf, label: x.tf }))} value={tf} onChange={setTf} group={`tr-tf-${frame}`} />
            </div>
          </div>

          <ChartArea className="mt-[1rem] h-[19rem]" rows={rows} format={(n) => perpPx(n) || "$0"} fitKey={`${marketId}:${tf}`} tfs={PERP_TIMEFRAMES.map((x) => x.tf)} tf={tf} onTf={(v) => setTf(v as Tf)} />

          <Panel className="mt-[1rem] px-[1.25rem] py-[0.75rem]">
            <StatLine
              items={[
                { label: "24h volume", value: <Num kind="market" value={m.volumeUsd} /> },
                { label: "Open interest", value: <Num kind="market" value={m.openInterestUsd} /> },
                { label: "Funding 1h", value: m.fundingRate != null ? fundingPct(m.fundingRate) : null },
                { label: "Next funding", value: m.nextFundingMs ? untilText(m.nextFundingMs) : null },
                { label: "Max leverage", value: fmtLeverage(m.maxLeverage ?? 1) },
                { label: "Fees", value: m.takerFee != null ? `${(m.takerFee * 100).toFixed(3)}%` : null },
              ]}
            />
          </Panel>

          <Panel className="mt-[1.25rem] p-[1.25rem]">
            <h2 className="text-row font-semibold">Your position</h2>
            {position ? (
              <div className="mt-[0.75rem] flex items-center gap-[1.5rem]">
                <StatLine
                  className="flex-1"
                  items={[
                    { label: "Side", value: `${position.side === "long" ? "Long" : "Short"} ${fmtLeverage(position.leverage)}` },
                    { label: "Size", value: <Num kind="money" value={position.notionalUsd} compact /> },
                    { label: "Entry", value: perpPx(position.entryPrice) },
                    { label: "Liquidation", value: perpPx(position.liquidationPrice) },
                    { label: "P/L", value: <Num kind="pnl" value={position.pnlUsd} /> },
                  ]}
                />
                <Action tone="secondary" onClick={() => setSheet("close")} hint="Close position">
                  Close
                </Action>
              </div>
            ) : (
              <p className="mt-[0.5rem] text-small text-secondary">{account ? "No open position in this market." : "Add a Rare Wallet account to trade perps."}</p>
            )}
          </Panel>
        </div>

        <Panel className="flex flex-col gap-[1rem] self-start p-[1.25rem]">
          <div data-area="order" className="contents">
          <SideSwitch<Side>
            items={[
              { value: "long", label: "Long", tone: "buy" },
              { value: "short", label: "Short", tone: "sell" },
            ]}
            value={side}
            onChange={setSide}
          />
          <div>
            <p className="text-small text-secondary">Margin</p>
            <p className="mt-[0.25rem] text-center text-display font-semibold tabular-nums">{`$${margin}`}</p>
          </div>
          <div data-nav-group="tr-margin" data-nav-remember className="flex gap-[0.5rem]">
            {MARGIN_PRESETS.map((v) => (
              <Chip key={v} on={margin === v} onClick={() => setMargin(v)}>
                ${v}
              </Chip>
            ))}
          </div>
          <div>
            <p className="text-small text-secondary">Leverage</p>
            <div className="mt-[0.25rem]">
              <Stepper group="tr-lev" label="leverage" onStep={(d) => setLev((v) => step(stops, v, d, maxLev, 1))}>
                <div className="flex items-center gap-[0.75rem]">
                  <div className="flex flex-1 items-center gap-[0.25rem]">
                    {stops.map((s) => (
                      <span key={s} className={cn("h-[0.375rem] flex-1 rounded-full", s <= lev ? "bg-fg" : "bg-l4")} />
                    ))}
                  </div>
                  <span className="w-[3.5rem] text-right text-row font-semibold tabular-nums">{fmtLeverage(lev)}</span>
                </div>
              </Stepper>
            </div>
          </div>
          <div>
            <Line label="Size">{preview.data ? `${numSize(preview.data.sizeUnits)} ${m.symbol}, ${numText("money", preview.data.notionalUsd)}` : ""}</Line>
            <Line label="Entry">{preview.data ? perpPx(preview.data.entryPrice) : ""}</Line>
            <Line label="Liquidation">{preview.data ? liqText(preview.data.liquidationPrice, preview.data.entryPrice) : ""}</Line>
            <Line label="Fees">{preview.data ? <Num kind="money" value={preview.data.feeUsd} /> : ""}</Line>
            {account && (
              <Line label={plan.needsAccount ? "Perps account" : "From your wallet"}>{plan.needsAccount ? "Set up first" : plan.shortBy > 0 ? `${perpPx(plan.shortBy)} USDC` : "Nothing"}</Line>
            )}
            <Line label="Wallet USDC">{usdc.data != null ? <Num kind="money" value={usdc.data} /> : ""}</Line>
          </div>
          <Action def tone={side === "long" ? "buy" : "sell"} disabled={blocked && !plan.needsAccount} onClick={() => setSheet("open")} hint={why ?? "Review"} className="w-full">
            {plan.needsAccount && account ? "Set up perps" : `Review ${side} ${fmtLeverage(lev)}`}
          </Action>
          {why && <p className="-mt-[0.5rem] text-center text-label text-secondary">{why}</p>}
          </div>
        </Panel>
      </div>

      {sheet === "open" && plan.needsAccount && (
        <ReviewSheet title="Set up perps" sub="One time, before your first order" account={account} action="Create account" build={(sender) => buildPerpTx(sender, { kind: "create-account" })} onClose={() => setSheet(null)} onSent={() => invalidate("/ext/perps/account")}>
          <Line label="What it does">Creates your perps account for USDC margin</Line>
          <Line label="After that">Review your order again to open it</Line>
        </ReviewSheet>
      )}
      {sheet === "open" && !plan.needsAccount && preview.data && (
        <ReviewSheet
          title={`${side === "long" ? "Long" : "Short"} ${m.symbol} ${fmtLeverage(lev)}`}
          sub="Market order"
          account={account}
          action={`Open ${side}`}
          tone={side === "long" ? "buy" : "sell"}
          build={(sender) => buildPerpTx(sender, openRequest({ marketId, side, marginUsd: margin, leverage: lev, shortBy: plan.shortBy }))}
          onClose={() => setSheet(null)}
          onSent={() => invalidate("/ext/perps/account")}
        >
          <Line label="Margin">{`$${margin}`}</Line>
          <Line label="Size" strong>{`${numSize(preview.data.sizeUnits)} ${m.symbol}, ${numText("money", preview.data.notionalUsd)}`}</Line>
          <Line label="Entry about">{perpPx(preview.data.entryPrice)}</Line>
          <Line label="Liquidation">{liqText(preview.data.liquidationPrice, preview.data.entryPrice)}</Line>
          <Line label="Fees">
            <Num kind="money" value={preview.data.feeUsd} />
          </Line>
          {plan.shortBy > 0 && <Line label="Adds from your wallet">{`${perpPx(plan.shortBy)} USDC`}</Line>}
          <Line label="Slippage">1%</Line>
        </ReviewSheet>
      )}
      {sheet === "close" && position && (
        <ReviewSheet
          title={`Close ${m.symbol} ${position.side}`}
          sub="Market order for the whole position"
          account={account}
          action="Close position"
          build={(sender) => buildPerpTx(sender, { kind: "close", marketId })}
          onClose={() => setSheet(null)}
          onSent={() => invalidate("/ext/perps/account")}
        >
          <Line label="Position">{`${position.side === "long" ? "Long" : "Short"} ${fmtLeverage(position.leverage)}`}</Line>
          <Line label="Entry">{perpPx(position.entryPrice)}</Line>
          <Line label="Mark">{perpPx(position.markPrice)}</Line>
          <Line label="P/L if closed now" strong>
            <Num kind="pnl" value={position.pnlUsd} />
          </Line>
        </ReviewSheet>
      )}
    </div>
  );
}

const numSize = (v: number) => (v >= 1000 ? v.toLocaleString("en-US", { maximumFractionDigits: 0 }) : v.toLocaleString("en-US", { maximumFractionDigits: v >= 1 ? 3 : 6 }));

function untilText(ms: number) {
  const s = Math.max(0, Math.round((ms - Date.now()) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}h ${m}m` : `${m}m`;
}
