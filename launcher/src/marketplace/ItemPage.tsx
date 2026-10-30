// Item page in rare.market's layout (item/item-view.tsx): media on the left; name, collection,
// rarity and owner, the buy box, traits and recent activity on the right.

import { StatLine } from "../ui/StatLine";
import { useEffect, useRef, useState } from "react";
import { native } from "../lib/bridge";
import { keystore, type RareAccount } from "../wallet";
import { paths, useRare, type CollectionDetail, type ItemCard, type ItemDetail, type MarketEvent, type Page, type Status } from "./api";
import { BuySheet } from "./BuySheet";
import { DASH, allIn, fmtPct, fmtSui, fmtUsd, itemUrl, mistToUsd, shortAddr, venueName } from "./format";
import { MImg } from "./MImg";
import { useMarketNav } from "./nav";
import { H, useLandingFocus, usePageButtons, usePageHints } from "./page";
import { Btn, Empty, EventHead, EventRow, RarityBadge, Sui, VenueMark, Verified } from "./parts";

export function ItemPage({ id, card, thumb, frame, active }: { id: string; card?: ItemCard; thumb?: number; frame: number; active: boolean }) {
  const push = useMarketNav((s) => s.push);
  const root = useRef<HTMLDivElement>(null);
  const [buying, setBuying] = useState(false);
  const [account, setAccount] = useState<RareAccount | null>(null);
  useEffect(() => {
    void keystore.current().then(setAccount).catch(() => setAccount(null));
  }, []);

  const q = useRare<ItemDetail>(paths.item(id), 20_000);
  // Opened from a card: it paints at once (name, price, the card's art from the cache); the full item follows.
  const item: ItemDetail | undefined = q.data ?? card;
  const pending = !q.data;
  const [bigArt, setBigArt] = useState(false);
  const coll = useRare<CollectionDetail>(item?.collection ? paths.collection(item.collection.slug) : null, 60_000);
  const ev = useRare<Page<MarketEvent>>(paths.events({ item: id }, 12), 30_000);
  const status = useRare<Status>(paths.status(), 60_000);
  const listing = item?.listing ?? null;
  const total = allIn(listing);
  const floor = coll.data?.stats.floorMist ?? null;
  const diff = total && floor && Number(floor) > 0 ? ((Number(total) - Number(floor)) / Number(floor)) * 100 : null;

  useLandingFocus(root, active && !buying, !!item);

  usePageButtons(frame, (b) => {
    if (buying) {
      if (b === "b") {
        setBuying(false);
        return true;
      }
      return b === "lb" || b === "rb" || b === "y" || b === "x";
    }
    return false;
  });
  usePageHints(active, buying ? [H.select, H.close] : [H.select, H.search, H.back]);

  if (!item) {
    return (
      <div ref={root} className="h-full">
        {q.error && <Empty title="Could not load this item" action={<Btn onClick={() => void native.openWeb(itemUrl(id))}>Open on rare.market</Btn>} />}
      </div>
    );
  }

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto">
      <div className="grid grid-cols-[minmax(0,1fr)_1.5rem_minmax(0,1.15fr)] px-10 pb-10 pt-2">
        <div className="self-start">
          <div className="relative aspect-square overflow-hidden rounded-xl bg-layer-01">
            {thumb && !bigArt && <MImg src={item.imageUrl} label={item.collection?.name ?? item.name} width={thumb} className="absolute inset-0 size-full" />}
            <MImg src={item.imageUrl} label={item.collection?.name ?? item.name} width={1024} opts={{ fit: "inside" }} contain onLoaded={() => setTimeout(() => setBigArt(true), 350)} className={`absolute inset-0 size-full ${thumb ? "" : "bg-layer-02"}`} />
          </div>
        </div>
        <div />
        <div className="min-w-0">
          <h1 className="text-2xl font-bold">{item.name}</h1>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-2 text-sm">
            {item.collection && (
              <button data-nav onClick={() => push({ kind: "collection", slug: item.collection!.slug })} className="flex h-8 items-center gap-1.5 rounded px-1 transition-opacity">
                {item.collection.name} {item.collection.verified && <Verified size="0.95rem" />}
              </button>
            )}
            <RarityBadge rarity={item.rarity} />
            <span className="h-5 w-px bg-border-light" aria-hidden />
            <span className="ml-auto text-secondary">
              Owned by <span className="font-mono text-primary">{shortAddr(item.owner)}</span>
            </span>
          </div>

          <div className="mt-4 rounded-xl border border-border-primary bg-layer-02 p-4">
            <p className="text-base">{listing ? "Total price" : "Not listed"}</p>
            {listing ? (
              <>
                <p className="mt-1 flex items-baseline gap-1.5 text-4xl font-bold tabular-nums">
                  {fmtSui(total, { dp: 4 })}
                  <span className="text-xl font-semibold">SUI</span>
                </p>
                <span className="text-base text-secondary tabular-nums">({fmtUsd(mistToUsd(total, status.data?.suiUsd))})</span>
                <p className="mt-1 flex items-center gap-1.5 text-sm text-secondary">
                  Listed on <VenueMark venue={listing.venue} /> {venueName(listing.venue)}
                </p>
              </>
            ) : (
              <p className="mt-1 text-sm text-secondary">The owner has not listed this item.</p>
            )}
            <div className="mt-3 flex gap-3">
              {listing ? (
                <Btn variant="primary" size="lg" def className="flex-1" onClick={() => setBuying(true)}>
                  Buy now
                </Btn>
              ) : null}
              <Btn size="lg" def={!listing} className={listing ? "" : "flex-1"} onClick={() => void native.openWeb(itemUrl(item.id))}>
                Open on rare.market
              </Btn>
            </div>
            {/* One stat line (trade pro), not four boxes inside the card. Unknown values are left out. */}
            <StatLine
              className="mt-4 border-t border-border-primary pt-4"
              items={[
                { label: "All-in price", value: listing ? <Sui mist={total} dp={3} /> : null },
                { label: "Floor price", value: floor ? <Sui mist={floor} dp={3} /> : null },
                { label: "Floor diff", value: diff === null ? null : `${diff > 0 ? "+" : diff < 0 ? "-" : ""}${fmtPct(diff, 2)}` },
                { label: "Top offer", value: item.topOfferMist ? <Sui mist={item.topOfferMist} dp={3} /> : null },
              ]}
            />
          </div>

          <h2 className="mb-3 mt-6 text-base font-medium">Traits</h2>
          <div className="rounded-lg border border-border-primary bg-layer-02 p-4">
            {item.attributes && item.attributes.length > 0 ? (
              <div data-nav-group={`mk-traits-${frame}`} className="grid grid-cols-3 gap-2">
                {item.attributes.map((a) => (
                  <button key={a.type} data-nav onClick={() => item.collection && push({ kind: "collection", slug: item.collection.slug })} className="min-h-[5rem] rounded-md bg-layer-04 p-2 text-left transition-colors focus-visible:bg-layer-03">
                    <p className="truncate text-xs text-secondary">{a.type}</p>
                    <p className="truncate text-sm font-semibold">{a.value}</p>
                    <div className="mt-2 flex items-center justify-between text-sm">
                      <span className="rounded-sm bg-labels-neutral px-1 text-xs text-label-neutral-text tabular-nums">{a.pct == null ? DASH : fmtPct(a.pct, a.pct < 1 ? 1 : 0)}</span>
                      <Sui mist={a.floorMist ?? null} dp={3} />
                    </div>
                  </button>
                ))}
              </div>
            ) : pending ? (
              <div className="h-20" aria-busy />
            ) : (
              <Empty title="No traits" compact />
            )}
          </div>

          <h2 className="mb-3 mt-6 text-base font-medium">Activity</h2>
          <div className="rounded-lg border border-border-primary bg-layer-02 p-2">
            {ev.data && ev.data.data.length === 0 ? (
              <Empty title="No activity yet" compact />
            ) : (
              <>
                <EventHead item={false} />
                {(ev.data?.data ?? []).map((e) => (
                  <EventRow key={e.id} e={e} item={false} />
                ))}
              </>
            )}
          </div>
          {item.description && <p className="pt-4 text-sm text-secondary">{item.description}</p>}
        </div>
      </div>
      {buying && listing && <BuySheet item={item} listing={listing} account={account} suiUsd={status.data?.suiUsd ?? null} onClose={() => setBuying(false)} />}
    </div>
  );
}

