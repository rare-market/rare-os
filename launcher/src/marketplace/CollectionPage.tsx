// Collection page in rare.market's layout (collection/header.tsx, items grid, holders-tab):
// banner when the collection has one, header with the stat strip, then Items, Activity and Holders.

import { ChevronDown, X } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { create } from "zustand";
import { feedback } from "../nav/feedback";
import { createPortal } from "react-dom";
import { native } from "../lib/bridge";
import { paths, useRare, usePaged, type CollectionDetail, type HoldersPage, type ItemCard, type MarketEvent, type SortName, type TraitValue } from "./api";
import { DASH, collectionUrl, fmtInt, fmtPct, fmtSui, shortName } from "./format";
import { MImg } from "./MImg";
import { useMarketNav } from "./nav";
import { H, useLandingFocus, usePageButtons, usePageHints } from "./page";
import { Avatar, Btn, Sui, Delta, Empty, EventHead, EventRow, HOLDER_COLS, HolderRow, ItemCardView, Stat, Tabs, Verified, nearEnd } from "./parts";
import { focusEl } from "../nav/focus";

type Tab = "items" | "activity" | "holders";
const TABS: Array<{ value: Tab; label: string }> = [
  { value: "items", label: "Items" },
  { value: "activity", label: "Activity" },
  { value: "holders", label: "Holders" },
];
const SORTS: Array<{ value: SortName; label: string }> = [
  { value: "price_asc", label: "Price: low to high" },
  { value: "price_desc", label: "Price: high to low" },
  { value: "rarity_asc", label: "Rarity: rarest first" },
  { value: "listed_desc", label: "Recently listed" },
];
const COLS = 6;

/** View (Select) flips the items between a grid of cards and a list of rows; the choice is kept. */
const useItemsView = create<{ mode: "grid" | "list" }>(() => ({ mode: "grid" }));

function ItemRow({ card, idx, def, onOpen }: { card: ItemCard; idx: number; def: boolean; onOpen: () => void }) {
  return (
    <button
      data-nav
      data-idx={idx}
      data-nav-default={def ? "" : undefined}
      onClick={onOpen}
      className="tile flex h-[3.5rem] w-full items-center gap-3 rounded-lg border border-transparent bg-layer-01 px-3 text-left focus-visible:border-border-interactive-hover focus-visible:bg-layer-02"
    >
      <span className="relative block size-[2.5rem] shrink-0 overflow-hidden rounded bg-layer-03">
        <MImg src={card.imageUrl} label={card.collection?.name ?? card.name} width={96} className="absolute inset-0 size-full" />
      </span>
      <span className="min-w-0 flex-1 truncate text-sm font-semibold">{shortName(card)}</span>
      <span className="w-[9rem] text-right text-sm">{card.listing ? <Sui mist={card.listing.allInMist ?? card.listing.priceMist} dp={3} className="justify-end font-semibold" /> : <span className="text-secondary">Not listed</span>}</span>
      <span className="w-[7rem] text-right text-sm tabular-nums text-secondary">{card.lastSaleMist ? fmtSui(card.lastSaleMist, { dp: 3, trim: true }) : DASH}</span>
    </button>
  );
}

export function CollectionPage({ slug, frame, active }: { slug: string; frame: number; active: boolean }) {
  const push = useMarketNav((s) => s.push);
  const root = useRef<HTMLDivElement>(null);
  const [tab, setTab] = useState<Tab>("items");
  const [sort, setSort] = useState<SortName>("price_asc");
  const [traits, setTraits] = useState<string[]>([]);
  const [filters, setFilters] = useState(false);
  const view = useItemsView((v) => v.mode);

  const col = useRare<CollectionDetail>(paths.collection(slug), 60_000);
  const items = usePaged<ItemCard>(paths.items(slug, sort, traits.join(",")));
  const c = col.data;
  const s = c?.stats;

  useLandingFocus(root, active, items.ready);

  // The focus ring follows the active tab; down enters its content once it has loaded.
  const switchTab = (t: Tab) => {
    setTab(t);
    requestAnimationFrame(() => {
      const el = root.current?.querySelector<HTMLElement>(`[data-tab="${t}"]`);
      if (el) focusEl(el);
    });
  };

  usePageButtons(frame, (b) => {
    if (filters) {
      if (b === "b" || b === "x") {
        setFilters(false);
        return true;
      }
      return b === "lt" || b === "rt" || b === "y";
    }
    // LT/RT switch Items, Activity and Holders (LB/RB always stay the launcher's tabs).
    if (b === "lt" || b === "rt") {
      const i = TABS.findIndex((t) => t.value === tab);
      switchTab(TABS[(i + (b === "rt" ? 1 : TABS.length - 1)) % TABS.length].value);
      return true;
    }
    if (b === "select" && tab === "items") {
      feedback("toggle");
      useItemsView.setState((v) => ({ mode: v.mode === "grid" ? "list" : "grid" }));
      return true;
    }
    if (b === "x" && tab === "items") {
      setFilters(true);
      return true;
    }
    return false;
  });
  usePageHints(
    active,
    filters
      ? [H.select, { b: "b", label: "Done" }]
      : [H.open, ...(tab === "items" ? [{ b: "x" as const, label: "Filters" }, { b: "select" as const, label: view === "grid" ? "List view" : "Grid view" }] : []), { b: "ltrt", label: "Sections" }, H.search, H.back],
  );

  return (
    <div ref={root} data-scroll className="h-full overflow-y-auto pb-10">
      {c?.bannerUrl && (
        <div className="relative h-[11rem] w-full overflow-hidden bg-layer-02">
          <MImg src={c.bannerUrl} label={c.name} width={1600} opts={{ fit: "inside" }} className="absolute inset-0 size-full" />
        </div>
      )}
      <div className="flex items-center gap-3 px-10 pt-4">
        <Avatar src={c?.imageUrl} label={c?.name ?? slug} size="3.5rem" />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="truncate text-xl font-semibold">{c?.name ?? ""}</h1>
            {c?.verified && <Verified />}
          </div>
          {c && (
            <p className="mt-0.5 text-xs text-secondary">
              Royalty {c.royaltyBps != null ? `${(c.royaltyBps / 100).toFixed(1)}%` : DASH}
              {c.venues.length > 0 && `, listed on ${c.venues.map((v) => (v === "market" ? "Rare" : v.charAt(0).toUpperCase() + v.slice(1))).join(", ")}`}
            </p>
          )}
        </div>
        {s && (
          <div className="ml-8 flex min-w-0 flex-1 items-end gap-x-6 overflow-hidden">
            <Stat label="Floor price">
              {fmtSui(s.floorMist, { dp: 3 })} <span className="font-normal text-secondary">SUI</span>
              <Delta value={s.floorChange24hPct} dp={2} className="text-base font-normal" />
            </Stat>
            <Stat label="Top offer">
              {fmtSui(s.topOfferMist, { dp: 3 })} <span className="font-normal text-secondary">SUI</span>
            </Stat>
            <Stat label="Volume 7d">{fmtSui(s.volume7dMist, { dp: 1, compact: true })}</Stat>
            <Stat label="Sales 7d">{fmtInt(s.sales7d)}</Stat>
            <Stat label="Listed / supply">
              {fmtInt(s.listedCount)} / {fmtInt(s.supply)} <span className="font-normal text-secondary">{s.supply ? fmtPct((s.listedCount / s.supply) * 100, 0) : ""}</span>
            </Stat>
            <Stat label="Owners">
              {s.owners ? (
                <>
                  {fmtInt(s.owners, { compact: true })} <span className="font-normal text-secondary">{fmtPct(s.uniqueOwnerPct, 1)}</span>
                </>
              ) : (
                DASH
              )}
            </Stat>
          </div>
        )}
      </div>
      {col.error && !c && <Empty title="Could not load this collection" action={<Btn onClick={() => void native.openWeb(collectionUrl(slug))}>Open on rare.market</Btn>} />}

      <div className="mt-6 flex items-center gap-3 border-b border-border-primary px-10">
        <Tabs items={TABS} value={tab} onChange={switchTab} />
        <div className="flex-1" />
        {tab === "items" && (
          <div className="flex items-center gap-2 pb-2">
            <span className="text-sm text-secondary">{items.total != null ? `${fmtInt(items.total)} listed` : ""}</span>
            <Btn size="sm" onClick={() => setFilters(true)}>
              {SORTS.find((x) => x.value === sort)?.label}
              {traits.length > 0 && `, ${traits.length} trait${traits.length > 1 ? "s" : ""}`}
              <ChevronDown size="0.875rem" />
            </Btn>
          </div>
        )}
      </div>

      <div className="px-10 pt-4">
        {tab === "items" && (
          <>
            <div data-nav-group={`mk-items-${frame}`} className={view === "grid" ? "grid grid-cols-6 gap-3" : "flex flex-col gap-1"} onFocus={nearEnd(items.items.length, view === "grid" ? COLS : 1, items.more)}>
              {items.items.map((it, i) =>
                view === "grid" ? (
                  <ItemCardView key={it.id} card={it} idx={i} def={i === 0} className="cv-cell" onOpen={() => push({ kind: "item", id: it.id, card: it, thumb: 400 })} />
                ) : (
                  <ItemRow key={it.id} card={it} idx={i} def={i === 0} onOpen={() => push({ kind: "item", id: it.id, card: it, thumb: 96 })} />
                ),
              )}
            </div>
            {items.ready && items.items.length === 0 && (
              <Empty title={traits.length ? "Nothing listed with these traits" : "Nothing listed right now"} action={traits.length ? <Btn size="md" onClick={() => setTraits([])}>Clear traits</Btn> : undefined} />
            )}
          </>
        )}
        {tab === "activity" && <Activity slug={slug} onOpen={(id) => push({ kind: "item", id })} />}
        {tab === "holders" && <Holders slug={slug} onOpen={(address, label) => push({ kind: "account", address, label })} />}
      </div>

      {filters && <FilterSheet slug={slug} sort={sort} setSort={setSort} traits={traits} setTraits={setTraits} onClose={() => setFilters(false)} />}
    </div>
  );
}

function Activity({ slug, onOpen }: { slug: string; onOpen: (id: string) => void }) {
  const ev = usePaged<MarketEvent>(paths.events({ collection: slug }, 30));
  if (ev.ready && ev.items.length === 0) return <Empty title="No activity yet" />;
  return (
    <div className="rounded-lg border border-border-primary bg-layer-02 p-2">
      <EventHead />
      <div onFocus={nearEnd(ev.items.length, 3, ev.more)}>
        {ev.items.map((e, i) => (
          <EventRow key={e.id} e={e} idx={i} onOpen={e.item ? () => onOpen(e.item!.id) : undefined} />
        ))}
      </div>
    </div>
  );
}

function Holders({ slug, onOpen }: { slug: string; onOpen: (address: string, label: string) => void }) {
  const h = useRare<HoldersPage>(paths.holders(slug), 120_000);
  if (h.error && !h.data) return <Empty title="Could not load holders" />;
  const hd = h.data?.header;
  return (
    <div className="rounded-lg border border-border-primary bg-layer-02">
      {hd && (
        <div className="grid grid-cols-5 gap-x-4 gap-y-3 border-b border-border-primary px-4 py-3">
          {[
            ["Owners", fmtInt(hd.owners)],
            ["Unique owners", fmtPct(hd.uniqueOwnersPct, 1)],
            ["Top 10 hold", fmtPct(hd.top10Pct, 1)],
            ["Top 100 hold", fmtPct(hd.top100Pct, 1)],
            ["Items held", `${fmtInt(hd.itemsHeld)} / ${fmtInt(hd.supply)}`],
          ].map(([k, v]) => (
            <div key={k} className="min-w-0">
              <p className="text-xs text-secondary">{k}</p>
              <p className="mt-0.5 truncate text-sm font-semibold tabular-nums">{v}</p>
            </div>
          ))}
        </div>
      )}
      {hd && hd.stakedOrLocked.length > 0 && (
        <div className="border-b border-border-primary px-4 py-3">
          <p className="text-xs text-secondary">Staked or locked</p>
          <ul className="mt-1 space-y-0.5 text-[0.8125rem]">
            {hd.stakedOrLocked.map((r) => (
              <li key={r.name} className="tabular-nums">
                {fmtInt(r.count)} in {r.name}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className={`trow grid ${HOLDER_COLS} h-10 items-center px-4 text-xs text-secondary`}>
        <span>#</span>
        <span>Holder</span>
        <span className="text-right">Items</span>
        <span>Share of supply</span>
        <span className="text-right">Listed</span>
      </div>
      {(h.data?.data ?? []).map((r) => (
        <HolderRow key={r.address} r={r} onOpen={() => onOpen(r.address, r.name ?? "Holder")} />
      ))}
    </div>
  );
}

/** Filters panel (collection/filters.tsx): sort, then trait types that open to their values. */
function FilterSheet({ slug, sort, setSort, traits, setTraits, onClose }: { slug: string; sort: SortName; setSort: (s: SortName) => void; traits: string[]; setTraits: (t: string[]) => void; onClose: () => void }) {
  const tv = useRare<{ data: TraitValue[] }>(paths.traits(slug), 300_000);
  const [open, setOpen] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useLandingFocus(root, true, 0);
  const groups = useMemo(() => {
    const m = new Map<string, TraitValue[]>();
    for (const v of tv.data?.data ?? []) m.set(v.type, [...(m.get(v.type) ?? []), v]);
    return [...m.entries()].map(([type, vals]) => ({ type, vals: vals.sort((a, b) => b.count - a.count).slice(0, 60) }));
  }, [tv.data]);
  const toggle = (key: string) => setTraits(traits.includes(key) ? traits.filter((t) => t !== key) : [...traits, key]);

  return createPortal(
    <div className="fade-in fixed inset-x-0 bottom-[3rem] top-0 z-40 flex justify-end bg-background/50">
      <div ref={root} data-nav-scope data-scroll className="slide-in-right flex h-full w-[26rem] flex-col overflow-y-auto border-l border-border-light bg-background">
        <div className="flex h-16 shrink-0 items-center justify-between px-4">
          <h2 className="text-lg font-semibold">Filters</h2>
          <button data-nav onClick={onClose} aria-label="Close" className="flex size-10 items-center justify-center rounded text-icon-primary">
            <X size="1.25rem" />
          </button>
        </div>
        <div className="px-4 pb-6">
          <div className="border-b border-border-primary pb-3">
            <p className="flex h-12 items-center text-base font-medium">Sort</p>
            {SORTS.map((o) => (
              <button key={o.value} data-nav data-nav-default={o.value === sort ? "" : undefined} onClick={() => setSort(o.value)} className="flex h-11 w-full items-center gap-3 rounded px-1 text-left text-sm text-primary">
                <span className={`size-5 shrink-0 rounded-full border-2 ${o.value === sort ? "border-[6px] border-primary" : "border-transparent bg-layer-04"}`} />
                {o.label}
              </button>
            ))}
          </div>
          <div className="pt-1">
            <div className="flex h-12 items-center justify-between">
              <p className="text-base font-medium">Traits</p>
              {traits.length > 0 && (
                <button data-nav onClick={() => setTraits([])} className="h-8 rounded bg-button-secondary px-3 text-sm">
                  Clear
                </button>
              )}
            </div>
            {groups.length === 0 && tv.data && <p className="py-3 text-sm text-secondary">No traits for this collection</p>}
            {groups.map((g) => {
              const sel = traits.filter((t) => t.startsWith(`${g.type}:`)).length;
              const shown = open === g.type;
              return (
                <div key={g.type} className="border-b border-border-primary last:border-b-0">
                  <button data-nav onClick={() => setOpen(shown ? null : g.type)} className="flex h-11 w-full items-center gap-2 rounded px-1 text-left text-sm">
                    <span className="min-w-0 flex-1 truncate">{g.type}</span>
                    {sel > 0 && <span className="flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-layer-04 px-1 text-xs tabular-nums">{sel}</span>}
                    <span className="w-8 shrink-0 text-right text-secondary tabular-nums">{g.vals.length}</span>
                    <ChevronDown size="1.125rem" className={`shrink-0 text-icon-secondary transition-transform ${shown ? "rotate-180" : ""}`} />
                  </button>
                  {shown && (
                    <ul className="pb-2">
                      {g.vals.map((v) => {
                        const key = `${v.type}:${v.value}`;
                        const on = traits.includes(key);
                        return (
                          <li key={key}>
                            <button data-nav onClick={() => toggle(key)} className="flex h-12 w-full min-w-0 items-center gap-3 rounded px-1 text-left">
                              <span className={`flex size-4 shrink-0 items-center justify-center rounded-sm border ${on ? "border-primary bg-primary" : "border-border-interactive-hover bg-transparent"}`}>
                                {on && (
                                  <svg width="0.625rem" height="0.625rem" viewBox="0 0 10 10" aria-hidden>
                                    <path d="M2 5.2 4 7l4-4.2" fill="none" stroke="rgb(var(--background))" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
                                  </svg>
                                )}
                              </span>
                              <span className="min-w-0 flex-1">
                                <span className="block truncate text-sm leading-[1.125rem]">{v.value}</span>
                                <span className="block truncate text-xs leading-4 text-secondary">{v.floorMist ? `${fmtSui(v.floorMist, { dp: 3 })} SUI floor` : "No listings"}</span>
                              </span>
                              <span className="w-[3.25rem] shrink-0 whitespace-nowrap text-right text-xs leading-4 text-secondary tabular-nums">
                                <span className="block text-sm leading-[1.125rem] text-primary">{fmtInt(v.count)}</span>
                                {fmtPct(v.pct, v.pct < 1 ? 1 : 0)}
                              </span>
                            </button>
                          </li>
                        );
                      })}
                    </ul>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
