// Marketplace front page in rare.market's home layout: search, trending (movers cards), your NFTs
// (item cards) and the top collections table with its time range.

import { Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { keystore, type RareAccount } from "../wallet";
import { paths, useRare, usePaged, type CollectionRow, type ItemCard, type Page, type Status } from "./api";
import { fmtInt, shortAddr } from "./format";
import { useMarketNav } from "./nav";
import { stepSection } from "../market/area";
import { MarketSections } from "../market/Sections";
import { H, useLandingFocus, usePageButtons, usePageHints } from "./page";
import { CollectionHead, CollectionTableRow, Empty, ItemCardView, MoverCard, SectionTitle, Segmented } from "./parts";

const WINDOWS = ["1h", "6h", "24h", "7d", "30d"] as const;
type Win = (typeof WINDOWS)[number];

export function HomePage({ frame, active }: { frame: number; active: boolean }) {
  const push = useMarketNav((s) => s.push);
  const [win, setWin] = useState<Win>("24h");
  const [account, setAccount] = useState<RareAccount | null>(null);
  useEffect(() => {
    void keystore.current().then(setAccount).catch(() => setAccount(null));
  }, []);

  const trending = useRare<Page<CollectionRow>>(paths.collections("24h", "volume", 12));
  const status = useRare<Status>(paths.status(), 60_000);
  const top = usePaged<CollectionRow>(paths.collections(win, "volume", 25));
  const mine = useRare<Page<ItemCard>>(account ? paths.accountItems(account.address) : null);
  const rows = trending.data?.data ?? [];
  const root = useRef<HTMLDivElement>(null);
  useLandingFocus(root, active, rows.length > 0);

  usePageButtons(frame, (b) => {
    if (b === "x") {
      setWin((w) => WINDOWS[(WINDOWS.indexOf(w) + 1) % WINDOWS.length]);
      return true;
    }
    if (b === "lt" || b === "rt") {
      stepSection(b === "rt" ? 1 : -1);
      return true;
    }
    return false;
  });
  usePageHints(active, [H.open, { b: "x", label: `Range ${win}` }, { b: "ltrt", label: "Sections" }, H.search, H.back, H.qs]);

  return (
    <div ref={root} data-scroll className="relative h-full overflow-y-auto pb-10">
      <div className="relative flex items-center gap-4 px-10 pb-6 pt-2">
        <MarketSections />
        <button data-nav onClick={() => push({ kind: "search", q: "" })} className="ml-auto flex h-11 w-[24rem] items-center gap-2 rounded border border-border-light bg-transparent px-3 text-left text-sm text-secondary transition-colors focus-visible:border-border-interactive-focus">
          <Search size="1rem" className="text-icon-secondary" />
          Search collections
        </button>
        {status.data && status.data.suiUsd > 0 && <span className="text-sm text-secondary tabular-nums">SUI ${status.data.suiUsd.toFixed(3)}</span>}
      </div>

      <section className="relative">
        <h2 className="px-10 text-xl font-bold">Trending</h2>
        {rows.length > 0 ? (
          <div data-rail data-nav-group={`mk-trending-${frame}`} className="-my-3 mt-1 flex gap-4 overflow-x-auto px-10 py-3">
            {rows.map((c, i) => (
              <MoverCard key={c.slug} c={c} def={i === 0} onOpen={() => push({ kind: "collection", slug: c.slug })} />
            ))}
          </div>
        ) : (
          trending.error && <Empty title="Could not load Rare Marketplace" compact>Check the connection. This page refreshes on its own.</Empty>
        )}
      </section>

      {account && (
        <section className="mt-10">
          <div className="px-10">
            <SectionTitle right={`${account.label || shortAddr(account.address)}${mine.data?.total != null ? `, ${fmtInt(mine.data.total)} items` : ""}`}>Your NFTs</SectionTitle>
          </div>
          {mine.data && mine.data.data.length === 0 ? (
            <Empty title="No NFTs in this wallet yet" compact />
          ) : (
            <div data-rail data-nav-group={`mk-mine-${frame}`} className="-my-3 flex gap-4 overflow-x-auto px-10 py-3">
              {(mine.data?.data ?? []).slice(0, 14).map((it) => (
                <ItemCardView key={it.id} card={it} showCollection className="w-[11rem] shrink-0" onOpen={() => push({ kind: "item", id: it.id, card: it, thumb: 400 })} />
              ))}
              {(mine.data?.data.length ?? 0) > 0 && (
                <button data-nav onClick={() => push({ kind: "account", address: account.address, label: "Your NFTs" })} className="w-[11rem] shrink-0 rounded-lg bg-button-secondary text-sm font-medium transition-colors focus-visible:bg-button-secondary-active">
                  See all
                </button>
              )}
            </div>
          )}
        </section>
      )}

      <section className="mx-10 mt-10 rounded-2xl bg-layer-01 p-4">
        <div className="flex items-center gap-2 pb-3">
          <h2 className="pl-3 text-xl font-bold">Top collections</h2>
          <div className="flex-1" />
          <Segmented items={WINDOWS.map((w) => ({ value: w, label: w }))} value={win} onChange={setWin} />
        </div>
        <CollectionHead win={win} />
        <div
          data-nav-group={`mk-top-${frame}`}
          onFocus={(e) => {
            const i = Number((e.target as HTMLElement).dataset.idx);
            if (i >= top.items.length - 6 && top.items.length < 100) top.more();
          }}
        >
          {top.items.slice(0, 100).map((c, i) => (
            <CollectionTableRow key={c.slug} r={c} idx={i} onOpen={() => push({ kind: "collection", slug: c.slug })} />
          ))}
        </div>
        {top.ready && top.items.length === 0 && <Empty title="No collections yet" compact />}
        {top.error && !top.items.length && <Empty title="Could not load collections" compact />}
      </section>
    </div>
  );
}
