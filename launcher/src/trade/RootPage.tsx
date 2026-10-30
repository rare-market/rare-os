// The Trade tab's front page: the section tabs (LB/RB) over Home, Perps, Traders and Positions.
// Home follows the Trade API's home: portfolio and SUI up top, trending, top movers, new launches and the
// perps markets.

import { useRef } from "react";
import { useApp } from "../lib/store";
import { Panel, SectionHead } from "../ui";
import { IconSearch } from "../components/icons";
import { fmtLeverage } from "./amount";
import { tokenPaths, useApi, usePerpLogos, type Portfolio, type TokenRow } from "./api";
import { H, sectionButtons, useLandingFocus, usePageButtons, usePageHints, useTradeNav } from "./nav";
import { Action, CoinLogo, Num, Quiet, TokenCard, TokenListRow, tokenMeta } from "./parts";
import { PerpsSection } from "./PerpsSection";
import { perpPaths, perpPx, type PerpMarket } from "./perps";
import { PositionsSection } from "./PositionsSection";
import { TradersSection, useBoardPrefs } from "./traders/TradersSection";
import { useTradeAccount } from "./wallet";
import { stepSection } from "../market/area";
import { MarketSections } from "../market/Sections";

export function RootPage({ frame, active }: { frame: number; active: boolean }) {
  const section = useTradeNav((s) => s.section);

  usePageButtons(frame, (b, repeat) => {
    if (sectionButtons.current?.(b, repeat)) return true;
    // LT/RT walk the sections (LB/RB always stay the launcher's tabs).
    if (b === "lt" || b === "rt") {
      stepSection(b === "rt" ? 1 : -1);
      return true;
    }
    return false;
  });
  const boardWindow = useBoardPrefs((s) => s.window);
  const traderHint = { b: "x" as const, label: boardWindow === "7d" ? "30 days" : "7 days" };
  usePageHints(active, [H.open, ...(section === "traders" ? [traderHint] : []), { b: "ltrt" as const, label: "Sections" }, H.search, H.qs]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex shrink-0 items-center gap-[2rem] px-[2.5rem] pb-[1rem]">
        <MarketSections />
        <div data-nav-group={`tr-search-${frame}`} className="ml-auto">
          <button data-nav data-hint-a="Search coins" onClick={() => useApp.getState().open({ kind: "search" })} className="flex h-[2.5rem] w-[19rem] items-center gap-[0.6rem] rounded-chip border border-border-light bg-l1 px-[0.9rem] text-left text-small text-muted">
            <IconSearch size="1.1rem" className="text-icon-2" />
            Search any coin
          </button>
        </div>
      </div>
      <div className="relative min-h-0 flex-1">
        {section === "home" && <HomeSection key="home" frame={frame} active={active} />}
        {section === "perps" && <PerpsSection key="perps" frame={frame} active={active} />}
        {section === "traders" && <TradersSection key="traders" frame={frame} active={active} />}
        {section === "positions" && <PositionsSection key="positions" frame={frame} active={active} />}
      </div>
    </div>
  );
}

function HomeSection({ frame, active }: { frame: number; active: boolean }) {
  const push = useTradeNav((s) => s.push);
  const setSection = useTradeNav((s) => s.setSection);
  const { account } = useTradeAccount();
  const root = useRef<HTMLDivElement>(null);
  const trending = useApi<{ items: TokenRow[] }>(tokenPaths.list("trending", 16), 30_000, 30_000);
  const gainers = useApi<{ items: TokenRow[] }>(tokenPaths.list("gainers", 6), 30_000, 30_000);
  const fresh = useApi<{ items: TokenRow[] }>(tokenPaths.list("new", 6), 30_000, 30_000);
  const perps = useApi<{ items: PerpMarket[] }>(perpPaths.markets(), 15_000, 15_000);
  const portfolio = useApi<Portfolio>(account ? tokenPaths.portfolio(account.address) : null, 30_000, 30_000);
  const rows = trending.data?.items ?? [];
  useLandingFocus(root, active, rows.length > 0);
  const open = (t: TokenRow) => push({ kind: "token", coinType: t.coinType });
  const markets = [...(perps.data?.items ?? [])].sort((a, b) => (b.volumeUsd ?? 0) - (a.volumeUsd ?? 0)).slice(0, 12);

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-10">
      <Panel className="mx-[2.5rem] flex items-center gap-[2rem] px-[1.5rem] py-[1.25rem]">
        {account ? (
          <div className="min-w-0">
            <p className="text-small text-secondary">Portfolio</p>
            <div className="mt-1 flex items-baseline gap-3">
              {portfolio.data ? <Num kind="money" value={portfolio.data.totalUsd} className="text-display font-semibold" /> : <span className="text-display font-semibold text-secondary">{portfolio.error ? "Unavailable" : ""}</span>}
              {portfolio.data && (
                <span className="text-small">
                  <Num kind="pnl" value={portfolio.data.change24hUsd} /> <Num kind="change" value={portfolio.data.change24hPct} className="text-secondary" tone="none" /> <span className="text-secondary">today</span>
                </span>
              )}
            </div>
            <p className="mt-[0.25rem] truncate text-label text-secondary">
              {account.label || `${account.address.slice(0, 6)}...${account.address.slice(-4)}`}
              {account.kind === "watch" ? ", watch only" : ""}
            </p>
          </div>
        ) : (
          <div className="min-w-0">
            <p className="text-row font-semibold">Trade any coin on Sui</p>
            <p className="mt-[0.25rem] text-small text-secondary">Add a Rare Wallet account to buy, sell and open perps from here.</p>
          </div>
        )}
        <div data-nav-group={`tr-actions-${frame}`} data-nav-remember className="ml-auto flex gap-2">
          <Action tone="primary" onClick={() => push({ kind: "swap" })}>
            Swap
          </Action>
          {account ? (
            <Action tone="secondary" onClick={() => setSection("positions")}>
              Positions
            </Action>
          ) : (
            <Action tone="secondary" onClick={() => useApp.getState().setTab("wallet")}>
              Open Wallet
            </Action>
          )}
        </div>
      </Panel>

      <section className="mt-[2.25rem]">
        <SectionHead title="Trending" className="mb-[0.9rem] px-[2.5rem]" />
        {rows.length > 0 ? (
          <div data-rail data-nav-group={`tr-trending-${frame}`} className="flex gap-[1rem] overflow-x-auto px-[2.5rem]">
            {rows.map((t, i) => (
              <TokenCard key={t.coinType} t={t} def={i === 0} onOpen={() => open(t)} />
            ))}
          </div>
        ) : (
          trending.error && <Quiet>Could not load trending coins. This page refreshes on its own.</Quiet>
        )}
      </section>

      <div className="mx-[2.5rem] mt-[2.25rem] grid grid-cols-2 gap-[1.25rem]">
        <Panel className="p-[1rem]">
          <SectionHead title="Top movers" right="24h" className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
          <div data-nav-group={`tr-gainers-${frame}`}>
            {(gainers.data?.items ?? []).map((t) => (
              <TokenListRow key={t.coinType} t={t} meta={tokenMeta(t)} onOpen={() => open(t)} />
            ))}
          </div>
          {gainers.error && !gainers.data && <Quiet>Could not load movers.</Quiet>}
        </Panel>
        <Panel className="p-[1rem]">
          <SectionHead title="New launches" className="mb-[0.5rem] px-[0.75rem] pt-[0.25rem]" />
          <div data-nav-group={`tr-new-${frame}`}>
            {(fresh.data?.items ?? []).map((t) => (
              <TokenListRow key={t.coinType} t={t} meta={tokenMeta(t, { age: true })} onOpen={() => open(t)} />
            ))}
          </div>
          {fresh.error && !fresh.data && <Quiet>Could not load new launches.</Quiet>}
        </Panel>
      </div>

      <section className="mt-[2.25rem]">
        <SectionHead title="Perps" note="Long or short with leverage" className="mb-[0.9rem] px-[2.5rem]" />
        {markets.length > 0 && (
          <div data-rail data-nav-group={`tr-perps-${frame}`} className="flex gap-[1rem] overflow-x-auto px-[2.5rem]">
            {markets.map((m) => (
              <PerpCard key={m.marketId} m={m} onOpen={() => push({ kind: "perp", marketId: m.marketId })} />
            ))}
            <button data-nav onClick={() => setSection("perps")} className="tile w-[10rem] shrink-0 self-stretch rounded-card bg-button-secondary text-body font-medium">
              All markets
            </button>
          </div>
        )}
        {perps.error && !perps.data && <Quiet>Perps markets are unavailable right now.</Quiet>}
      </section>
    </div>
  );
}

export function PerpCard({ m, onOpen }: { m: PerpMarket; onOpen: () => void }) {
  const logo = usePerpLogos();
  return (
    <button data-nav data-hint-a="Trade" onClick={onOpen} className="tile w-[13rem] shrink-0 rounded-card border border-hair bg-l1 p-[1rem] text-left">
      <span className="flex items-center gap-[0.75rem]">
        <CoinLogo src={logo(m.marketId, m.symbol)} symbol={m.symbol} size="2rem" />
        <span className="min-w-0">
          <span className="block truncate text-body font-semibold">{m.symbol}</span>
          <span className="block text-label text-secondary">Up to {fmtLeverage(m.maxLeverage ?? 1)}</span>
        </span>
      </span>
      <span className="mt-[1rem] flex items-baseline justify-between gap-[0.5rem]">
        <span className="text-row font-semibold tabular-nums">{perpPx(m.price)}</span>
        <Num kind="change" value={m.changePct} className="text-small" />
      </span>
    </button>
  );
}

