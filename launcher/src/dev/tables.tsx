// Dev-only fixture page (http://localhost:5180/tables.html) with every table row type holding
// worst-case values, in a half-width column. scripts/table-check.mjs loads it at 1920x1200 and
// fails when any cell overflows or two cells overlap. Not part of the production build.

import { createRoot } from "react-dom/client";
import "../styles/app.css";
import { CollectionHead, CollectionTableRow, EventHead, EventRow, HOLDER_COLS, HolderRow } from "../marketplace/parts";
import { CoinHead, CoinRow } from "../wallet/ui/home/CoinRows";
import { HoldersTable, TradesTable } from "../trade/TokenPage";
import { HeadRow, Row } from "../ui";
import type { CollectionRow, Holder, MarketEvent } from "../marketplace/api";

const LONG = "The Extremely Long Collection Name Number";
const ADDR = "0x" + "0912ab34cd56ef78".repeat(4);
const HUGE = "123456789012000000000000";
const noop = () => {};

const coll = { slug: "x", name: LONG, imageUrl: null, verified: true } as unknown as CollectionRow;
const collRow: CollectionRow = { ...coll, rank: 1234, supply: 123456789, floorMist: HUGE, floorChangePct: -123.45, topOfferMist: HUGE, volumeMist: HUGE, volumeChangePct: 5, sales: 123456789, listedCount: 12345678, owners: 1 };
const ev = (o: Partial<MarketEvent>): MarketEvent => ({
  id: String(Math.random()),
  kind: "sale",
  venue: "tradeport",
  item: { id: "i", name: `${LONG} #123456`, imageUrl: null },
  collection: coll,
  priceMist: HUGE,
  from: ADDR,
  to: ADDR,
  fromName: null,
  toName: null,
  at: new Date(Date.now() - 86400e3 * 400).toISOString(),
  ...o,
});
const events = [ev({}), ev({ fromName: LONG + " owner", toName: LONG + " buyer" }), ev({ venue: "unknown-venue-with-a-very-long-id", kind: "offer_cancel" }), ev({ priceMist: "2050000000", venue: "tradeport" })];
const holder: Holder = { rank: 12345, address: ADDR, name: null, count: 123456789, pctOfSupply: 12.3456, listed: 123456789, staked: 0 };
const named: Holder = { ...holder, name: LONG + " name.sui" };

function Frame({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section data-table={id} className="mb-6">
      <h2 className="mb-1 text-label text-secondary">{title}</h2>
      <div className="rounded-lg border border-border-primary bg-layer-02 p-2">{children}</div>
    </section>
  );
}

function Page() {
  return (
    // The right half of a 1920 wide screen, as on the item page.
    <div className="bg-bg p-4 text-fg" style={{ width: "50vw", marginLeft: "50vw" }}>
      <Frame id="collections" title="Marketplace collections">
        <CollectionHead win="24h" />
        <CollectionTableRow r={collRow} onOpen={noop} />
      </Frame>
      <Frame id="activity-item" title="Item activity (venue column)">
        <EventHead item={false} />
        {events.map((e, i) => (
          <EventRow key={i} e={e} item={false} />
        ))}
      </Frame>
      <Frame id="activity-collection" title="Collection activity (item column)">
        <EventHead />
        {events.map((e, i) => (
          <EventRow key={i} e={e} />
        ))}
      </Frame>
      <Frame id="holders" title="Marketplace holders">
        <div className={`trow grid ${HOLDER_COLS} h-10 items-center px-4 text-xs text-secondary`}>
          <span>#</span>
          <span>Holder</span>
          <span className="text-right">Items</span>
          <span>Share of supply</span>
          <span className="text-right">Listed</span>
        </div>
        <HolderRow r={holder} onOpen={noop} />
        <HolderRow r={named} onOpen={noop} />
      </Frame>
      <Frame id="wallet-coins" title="Wallet coins">
        <CoinHead />
        <CoinRow t={{ coinType: "0x2::sui::SUI", symbol: "A_VERY_LONG_SYMBOL_TOKEN", name: LONG, decimals: 9, amount: 123456789012.123456, iconUrl: null, priceUsd: 1, valueUsd: 123456789012.12, change24h: -123.4, verified: true } as never} onOpen={noop} />
      </Frame>
      <Frame id="trade-trades" title="Trade: trades">
        <TradesTable
          symbol="LONGSYMBOL"
          error={null}
          frame={0}
          rows={[{ id: "1", timeMs: Date.now() - 86400e3 * 900, buy: true, usd: 123456789012, amount: 123456789012345, priceUsd: 0.00000012345678, trader: ADDR }]}
        />
      </Frame>
      <Frame id="trade-holders" title="Trade: holders">
        <HoldersTable
          error={null}
          frame={0}
          page={{ count: 123456789, top10Pct: 1, holders: [{ address: ADDR, amount: 123456789012345, valueUsd: 123456789012, pctSupply: 12.34, kind: "pool", venue: "a-venue-with-a-long-name" }] }}
        />
      </Frame>
      <Frame id="generic-row" title="Shared Row">
        <HeadRow cols="grid-cols-[minmax(0,1fr)_6rem_6rem] text-right">
          <span className="text-left">Name</span>
          <span>Amount</span>
          <span>Value</span>
        </HeadRow>
        <Row cols="grid-cols-[minmax(0,1fr)_6rem_6rem] text-right">
          <span className="text-left">{LONG + LONG}</span>
          <span>{HUGE}</span>
          <span>{HUGE}</span>
        </Row>
      </Frame>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<Page />);
