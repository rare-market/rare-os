// Perps markets: the account line and every market as a table, biggest volume first
// (a perps market picker's columns).

import { useRef } from "react";
import { HeadRow, Panel, Row } from "../ui";
import { fmtLeverage } from "./amount";
import { useApi, usePerpLogos } from "./api";
import { useLandingFocus, useTradeNav } from "./nav";
import { CoinLogo, Num, Quiet, StatLine } from "./parts";
import { fundingPct, perpPaths, perpPx, type PerpMarket, type PerpsAccount } from "./perps";
import { useTradeAccount } from "./wallet";

const COLS = "grid-cols-[minmax(0,1fr)_9rem_6rem_7rem_8rem_7rem_4rem] text-right";

export function PerpsSection({ frame, active }: { frame: number; active: boolean }) {
  const push = useTradeNav((s) => s.push);
  const { account } = useTradeAccount();
  const root = useRef<HTMLDivElement>(null);
  const markets = useApi<{ items: PerpMarket[] }>(perpPaths.markets(), 10_000, 10_000);
  const acct = useApi<{ account: PerpsAccount | null }>(account ? perpPaths.account(account.address) : null, 15_000, 15_000);
  const logo = usePerpLogos();
  const rows = [...(markets.data?.items ?? [])].sort((a, b) => (b.volumeUsd ?? 0) - (a.volumeUsd ?? 0));
  useLandingFocus(root, active, rows.length > 0);
  const a = acct.data?.account;

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-[2.5rem]">
      {a && (
        <Panel className="mx-[2.5rem] mb-[1.25rem] px-[1.5rem] py-[1rem]">
          <StatLine
            items={[
              { label: "Perps balance", value: <Num kind="money" value={a.equityUsd} /> },
              { label: "Available", value: <Num kind="money" value={a.availableUsd} /> },
              { label: "Unrealized P/L", value: <Num kind="pnl" value={a.unrealizedPnlUsd} /> },
              { label: "Open positions", value: String(a.positions.length) },
            ]}
          />
        </Panel>
      )}
      <Panel className="mx-[2.5rem] p-[1rem]">
        <HeadRow cols={COLS}>
          <span className="text-left">Market</span>
          <span>Price</span>
          <span>24h</span>
          <span>Volume</span>
          <span>Open interest</span>
          <span>Funding 1h</span>
          <span>Max</span>
        </HeadRow>
        <div data-nav-group={`tr-perp-table-${frame}`}>
          {rows.map((m, i) => (
            <Row key={m.marketId} cols={COLS} idx={i} hint="Trade" onClick={() => push({ kind: "perp", marketId: m.marketId })} className="cv-row">
              <span className="flex min-w-0 items-center gap-[0.75rem] text-left">
                <CoinLogo src={logo(m.marketId, m.symbol)} symbol={m.symbol} size="2rem" />
                <span className="min-w-0">
                  <span className="block truncate text-body font-semibold">{m.symbol}</span>
                  <span className="block truncate text-label text-secondary">{m.pair.replace(/USD$/, "/USD")}</span>
                </span>
              </span>
              <span className="font-semibold tabular-nums">{perpPx(m.price)}</span>
              <Num kind="change" value={m.changePct} />
              <Num kind="market" value={m.volumeUsd} />
              <Num kind="market" value={m.openInterestUsd} />
              <span className="tabular-nums text-secondary">{m.fundingRate != null ? fundingPct(m.fundingRate) : ""}</span>
              <span className="tabular-nums">{fmtLeverage(m.maxLeverage ?? 1)}</span>
            </Row>
          ))}
        </div>
        {markets.error && !rows.length && <Quiet>Perps markets are unavailable right now. This page refreshes on its own.</Quiet>}
      </Panel>
    </div>
  );
}
