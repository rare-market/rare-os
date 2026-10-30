// Positions: the wallet's coins (priced) and its open perps positions, each with a way out.

import { useRef, useState } from "react";
import { Empty, HeadRow, Panel, Row, SectionHead, numText } from "../ui";
import { SUI_TYPE, fmtLeverage, normalizeType } from "./amount";
import { invalidate, tokenPaths, useApi, usePerpLogos, type Portfolio } from "./api";
import { useLandingFocus, useTradeNav } from "./nav";
import { Action, CoinLogo, Line, Num, Quiet, StatLine } from "./parts";
import { buildPerpTx, liqText, perpPaths, perpPx, type PerpPosition, type PerpsAccount } from "./perps";
import { ReviewSheet } from "./ReviewSheet";
import { useTradeAccount } from "./wallet";

const COIN_COLS = "grid-cols-[minmax(0,1fr)_9rem_9rem_8rem_6rem] text-right";
const PERP_COLS = "grid-cols-[minmax(0,1fr)_7rem_7rem_7rem_7rem_7rem] text-right";

export function PositionsSection({ frame, active }: { frame: number; active: boolean }) {
  const push = useTradeNav((s) => s.push);
  const { account, ready } = useTradeAccount();
  const root = useRef<HTMLDivElement>(null);
  const [closing, setClosing] = useState<PerpPosition | null>(null);
  const portfolio = useApi<Portfolio>(account ? tokenPaths.portfolio(account.address) : null, 20_000, 20_000);
  const perps = useApi<{ account: PerpsAccount | null }>(account ? perpPaths.account(account.address) : null, 15_000, 15_000);
  const coins = (portfolio.data?.holdings ?? []).filter((h) => h.valueUsd >= 0.01 || normalizeType(h.coinType) === SUI_TYPE);
  const logo = usePerpLogos();
  const positions = perps.data?.account?.positions ?? [];
  useLandingFocus(root, active && !closing, coins.length + positions.length);

  if (ready && !account)
    return (
      <div className="px-[2.5rem]">
        <Empty title="No wallet yet" body="Add a Rare Wallet account on the Wallet tab to see your coins and positions here." />
      </div>
    );

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      <Panel className="mx-[2.5rem] px-[1.5rem] py-[1rem]">
        <StatLine
          items={[
            { label: "Coins", value: portfolio.data ? <Num kind="money" value={portfolio.data.totalUsd} /> : null },
            { label: "Today", value: portfolio.data ? <Num kind="pnl" value={portfolio.data.change24hUsd} /> : null },
            { label: "Perps balance", value: perps.data?.account ? <Num kind="money" value={perps.data.account.equityUsd} /> : null },
            { label: "Unrealized P/L", value: perps.data?.account ? <Num kind="pnl" value={perps.data.account.unrealizedPnlUsd} /> : null },
          ]}
        />
      </Panel>

      <Panel className="mx-[2.5rem] mt-[1.25rem] p-[1rem]">
        <SectionHead title="Perps positions" right={account?.kind === "watch" ? "Watch only" : undefined} className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
        {positions.length > 0 ? (
          <>
            <HeadRow cols={PERP_COLS} className="mr-[6.5rem]">
              <span className="text-left">Market</span>
              <span>Size</span>
              <span>Entry</span>
              <span>Mark</span>
              <span>Liquidation</span>
              <span>P/L</span>
            </HeadRow>
            {positions.map((p) => (
              <div key={p.marketId} data-nav-group={`tr-pos-${frame}-${p.marketId}`} data-nav-remember className="flex items-center gap-[0.5rem]">
                <Row cols={PERP_COLS} hint="Open market" onClick={() => push({ kind: "perp", marketId: p.marketId })} className="min-w-0 flex-1">
                  <span className="flex min-w-0 items-center gap-[0.75rem] text-left">
                    <CoinLogo src={logo(p.marketId, p.symbol)} symbol={p.symbol} size="2rem" />
                    <span>
                      <span className="block text-body font-semibold">{p.symbol}</span>
                      <span className="block text-label text-secondary">
                        {p.side === "long" ? "Long" : "Short"} {fmtLeverage(p.leverage)}
                      </span>
                    </span>
                  </span>
                  <Num kind="money" value={p.notionalUsd} compact />
                  <span className="tabular-nums">{perpPx(p.entryPrice)}</span>
                  <span className="tabular-nums">{perpPx(p.markPrice)}</span>
                  <span className="tabular-nums text-secondary">{liqText(p.liquidationPrice, p.entryPrice)}</span>
                  <Num kind="pnl" value={p.pnlUsd} className="font-semibold" />
                </Row>
                <Action tone="secondary" size="md" onClick={() => setClosing(p)} hint="Close position" className="w-[6rem]">
                  Close
                </Action>
              </div>
            ))}
          </>
        ) : (
          <Quiet className="py-[1.5rem]">{perps.data ? "No open perps positions." : perps.error ? "Could not load perps positions." : ""}</Quiet>
        )}
      </Panel>

      <Panel className="mx-[2.5rem] mt-[1.25rem] p-[1rem]">
        <SectionHead title="Coins" className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
        {coins.length > 0 && (
          <HeadRow cols={COIN_COLS}>
            <span className="text-left">Coin</span>
            <span>Balance</span>
            <span>Value</span>
            <span>Price</span>
            <span>24h</span>
          </HeadRow>
        )}
        <div data-nav-group={`tr-coins-${frame}`}>
          {coins.map((h, i) => (
            <Row key={h.coinType} cols={COIN_COLS} idx={i} hint="Trade" onClick={() => push({ kind: "token", coinType: h.coinType, side: normalizeType(h.coinType) === SUI_TYPE ? "buy" : "sell" })} className="cv-row">
              <span className="flex min-w-0 items-center gap-[0.75rem] text-left">
                <CoinLogo src={h.logo} symbol={h.symbol} size="2rem" />
                <span className="min-w-0">
                  <span className="block truncate text-body font-semibold">{h.symbol}</span>
                  <span className="block truncate text-label text-secondary">{h.name}</span>
                </span>
              </span>
              <span className="tabular-nums">{numText("amount", h.amount)}</span>
              <Num kind="money" value={h.valueUsd} className="font-semibold" />
              <Num kind="price" value={h.priceUsd} />
              <Num kind="change" value={h.change24h} />
            </Row>
          ))}
        </div>
        {portfolio.data && coins.length === 0 && <Quiet className="py-[1.5rem]">No coins in this wallet yet.</Quiet>}
        {portfolio.error && !portfolio.data && <Quiet className="py-[1.5rem]">Could not load this wallet's coins.</Quiet>}
      </Panel>

      {closing && (
        <ReviewSheet
          title={`Close ${closing.symbol} ${closing.side}`}
          sub="Market order for the whole position"
          account={account}
          action="Close position"
          build={(sender) => buildPerpTx(sender, { kind: "close", marketId: closing.marketId })}
          onClose={() => setClosing(null)}
          onSent={() => invalidate("/ext/perps/account")}
        >
          <Line label="Position">
            {closing.side === "long" ? "Long" : "Short"} {fmtLeverage(closing.leverage)}, <Num kind="money" value={closing.notionalUsd} />
          </Line>
          <Line label="Entry">{perpPx(closing.entryPrice)}</Line>
          <Line label="Mark">{perpPx(closing.markPrice)}</Line>
          <Line label="P/L if closed now" strong>
            <Num kind="pnl" value={closing.pnlUsd} />
          </Line>
          <Line label="Margin back">
            <Num kind="money" value={Math.max(0, closing.marginUsd + closing.pnlUsd)} />
          </Line>
        </ReviewSheet>
      )}
    </div>
  );
}
