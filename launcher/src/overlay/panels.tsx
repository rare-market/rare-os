// The Rare menu's market panels: your items from the game (floor, best offer, last sale; sell into
// the best offer or list), the cheapest listings, trophies, and the hooks that load them. Moved out
// of Overlay.tsx unchanged when the panel became the Rare menu's Items, Market and Trophies tabs.
//
// Signing goes through Rare Wallet's review sheet (signAndExecute), like Buy in the Marketplace.

import { useEffect, useRef, useState } from "react";
import { native } from "../lib/bridge";
import { loadGames, type SuiGame } from "../lib/rare";
import { useApp } from "../lib/store";
import type { ItemCard } from "../marketplace/api";
import { fmtSui, shortName, venueName } from "../marketplace/format";
import { focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { askPrice } from "./price";
import { sortRows, type TrophyRow } from "../trophies/model";
import type { CaseGame } from "../trophies/ui/useCase";
import { Button, Dot, HeadRow, MImg, Row, StatLine, Sui } from "../ui";
import { keystore, type RareAccount } from "../wallet";
import { AddPasskey } from "../wallet/ui/AddPasskey";
import { useWallet } from "../wallet/ui/store";
import { canSignWithWallet, signAndExecute, SignRejected } from "../wallet/market";
import { signAndExecuteForGame } from "../wallet/sign";
import { loadItems, loadMarket, loadTrophies, type Floors, type OverlayItem, type Trophy } from "./data";
import type { OverlayGame } from "./game";
import { closeOverlay } from "./session";
import { bestOffer, buildAcceptOfferTransaction, buildListTransaction, offerNetMist } from "./trade";
import type { Offer } from "../marketplace/api";

/**
 * Quitting the game under the overlay: once it has gone from the running list (or after 10 s),
 * the overlay closes and Rare has the screen. Also closes once nothing is running any more.
 */
export function useQuitThenClose() {
  const running = useApp((s) => s.running);
  const [quitting, setQuitting] = useState<string | null>(null);
  const seen = useRef(false);
  if (running.length) seen.current = true;
  useEffect(() => {
    if (seen.current && !running.length) {
      void closeOverlay();
      return;
    }
    if (!quitting) return;
    if (!running.includes(quitting)) {
      void closeOverlay();
      return;
    }
    const t = setTimeout(() => void closeOverlay(), 10000);
    return () => clearTimeout(t);
  }, [quitting, running]);
  return { quitting, setQuitting };
}

/** The game, frozen and dimmed, behind the panel. */
export function Frozen({ src }: { src: string | null }) {
  return (
    <div className="absolute inset-0 overflow-hidden bg-bg" aria-hidden>
      {src && <img src={src} alt="" className="fade-in absolute inset-0 size-full object-cover" />}
      <div className="absolute inset-0 bg-black/65" />
      <div className="absolute inset-y-0 right-0 w-[45rem] bg-gradient-to-l from-black/55 to-transparent" />
    </div>
  );
}

const ITEM_COLS = "grid-cols-[2.6rem_minmax(0,1fr)_3.9rem_3.9rem_3.9rem]";

export function useGames(): SuiGame[] {
  const [games, setGames] = useState<SuiGame[]>([]);
  useEffect(() => {
    void loadGames().then((r) => setGames(r.games));
  }, []);
  return games;
}

export function useAccount(): RareAccount | null | undefined {
  const [a, setA] = useState<RareAccount | null | undefined>(undefined);
  useEffect(() => {
    void keystore.current().then(setA, () => setA(null));
    // An account added from the prompt in this panel lands here without reopening it.
    return useWallet.subscribe((s, p) => s.current?.address !== p.current?.address && setA(s.current));
  }, []);
  return a;
}

export interface Data {
  items: OverlayItem[];
  floors: Floors;
  market: ItemCard[];
  trophies: Trophy[];
}

export function useOverlayData(game: OverlayGame, address: string | null, ready: boolean) {
  const [data, setData] = useState<Data | null>(null);
  const [loading, setLoading] = useState(false);
  const [tick, setTick] = useState(0);
  const key = `${game.slug}|${game.collections.map((c) => c.nftType).join(",")}|${address}`;
  useEffect(() => {
    if (!ready) return;
    let live = true;
    setLoading(true);
    void (async () => {
      const get = native.rareGet;
      const empty = { items: [] as OverlayItem[], floors: {} as Floors };
      const mine = address ? await loadItems(get, address, game).catch(() => empty) : empty;
      // Floors also give the collection slugs the market tab needs.
      let floors = mine.floors;
      if (!address) floors = (await loadItems(get, "0x0", game).catch(() => empty)).floors;
      if (live) setData((d) => ({ items: mine.items, floors, market: d?.market ?? [], trophies: d?.trophies ?? [] }));
      const [market, trophies] = await Promise.all([
        loadMarket(get, game, floors, address).catch(() => [] as ItemCard[]),
        address ? loadTrophies(address, game).catch(() => [] as Trophy[]) : Promise.resolve([] as Trophy[]),
      ]);
      if (live) setData({ items: mine.items, floors, market, trophies });
    })().finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, ready, tick]);
  return { data, loading, refresh: () => setTick((t) => t + 1) };
}

/** A SUI amount without trailing zeros, unit muted (rare.market's Sui, trimmed). */
export function Amount({ mist }: { mist: string | bigint }) {
  return (
    <span className="inline-flex items-baseline gap-1 whitespace-nowrap tabular-nums">
      {fmtSui(mist, { dp: 3, trim: true })}
      <span className="font-normal text-secondary">SUI</span>
    </span>
  );
}

export function Calm({ text }: { text: string }) {
  return <p className="px-[0.75rem] py-[1.5rem] text-small text-secondary">{text}</p>;
}

const num = (mist: string | null | undefined) => (mist ? fmtSui(mist, { dp: 2, compact: true, trim: true }) : null);

function Price({ mist }: { mist: string | null | undefined }) {
  const t = num(mist);
  return <span className={`text-right tabular-nums ${t ? "text-primary" : "text-secondary"}`}>{t ?? ""}</span>;
}

export function ItemList({ items, ready, onOpen }: { items: OverlayItem[]; ready: boolean; onOpen: (i: OverlayItem) => void }) {
  if (ready && !items.length) return <Calm text="You don't hold any items from this game yet. The Market tab has some for sale." />;
  if (!items.length) return null;
  return (
    <>
      <HeadRow cols={ITEM_COLS}>
        <span />
        <span>Item</span>
        <span className="text-right">Floor</span>
        <span className="text-right">Offer</span>
        <span className="text-right">Last</span>
      </HeadRow>
      <div data-nav-group="overlay-items" className="flex flex-col">
        {items.map((it, i) => (
          <Row key={it.id} idx={i} cols={ITEM_COLS} onClick={() => onOpen(it)} hint="Sell or list" className="h-[3.5rem]">
            <Thumb src={it.imageUrl} label={it.name} />
            <span className="min-w-0">
              <span className="block truncate font-medium">{shortName(it)}</span>
              <span className="block truncate text-label text-secondary">{it.listing ? `Listed for ${fmtSui(it.listing.priceMist, { dp: 2, trim: true })} SUI` : it.collection?.name}</span>
            </span>
            <Price mist={it.floorMist} />
            <Price mist={it.topOfferMist} />
            <Price mist={it.lastSaleMist} />
          </Row>
        ))}
      </div>
      <p className="px-[0.75rem] pt-[0.6rem] text-label text-secondary">Prices in SUI.</p>
    </>
  );
}

export function Thumb({ src, label }: { src: string | null | undefined; label: string }) {
  return (
    <span className="relative size-[2.6rem] overflow-hidden rounded-card bg-l3">
      <MImg src={src} label={label} width={120} className="absolute inset-0 size-full" />
    </span>
  );
}

const MARKET_COLS = "grid-cols-[2.6rem_minmax(0,1fr)_auto]";

export function MarketList({ items, ready, onOpen }: { items: ItemCard[]; ready: boolean; onOpen: (i: ItemCard) => void }) {
  if (ready && !items.length) return <Calm text="Nothing from this game is listed right now." />;
  return (
    <div data-nav-group="overlay-market" className="flex flex-col">
      {items.map((it, i) => (
        <Row key={it.id} idx={i} cols={MARKET_COLS} onClick={() => onOpen(it)} hint="Buy" className="h-[3.5rem]">
          <Thumb src={it.imageUrl} label={it.name} />
          <span className="min-w-0">
            <span className="block truncate font-medium">{shortName(it)}</span>
            <span className="block truncate text-label text-secondary">
              {it.collection?.name}
              {it.listing?.venue ? ` on ${venueName(it.listing.venue)}` : ""}
            </span>
          </span>
          <Sui mist={it.listing?.allInMist ?? it.listing?.priceMist} dp={3} className="font-medium" />
        </Row>
      ))}
    </div>
  );
}

const TROPHY_COLS = "grid-cols-[2.6rem_minmax(0,1fr)_auto]";
const STATE: Record<TrophyRow["state"], string> = { minted: "Minted", unlocked: "Unlocked", locked: "Locked" };

export function Trophies({ list, rare, ready, game }: { list: Trophy[]; rare: CaseGame | null; ready: boolean; game: string }) {
  const rows = rare ? sortRows(rare.rows) : [];
  if (ready && !list.length && !rows.length) return <Calm text={`No trophies from ${game} yet. Achievements and rewards the game gives you show up here.`} />;
  return (
    <div className="flex flex-col gap-[1rem]">
      {rows.length > 0 && rare && (
        <section>
          <div className="px-[0.75rem] pb-[0.3rem] text-label text-secondary">
            {rare.sum.unlocked} of {rare.sum.total} unlocked{rare.sum.minted ? `, ${rare.sum.minted} minted` : ""}
          </div>
          <div data-nav-group="overlay-rare-trophies" className="flex flex-col">
            {rows.map((r, i) => (
              <Row key={r.key} idx={i} cols={TROPHY_COLS} className="h-[3.5rem]">
                <Thumb src={r.state === "locked" ? r.iconGrayUrl ?? r.iconUrl : r.iconUrl} label={r.name} />
                <span className="min-w-0">
                  <span className={`block truncate font-medium ${r.state === "locked" ? "text-secondary" : ""}`}>{r.masked ? "Hidden trophy" : r.name}</span>
                  {!r.masked && r.description && <span className="block truncate text-label text-secondary">{r.description}</span>}
                </span>
                <span className="text-label text-secondary">{STATE[r.state]}</span>
              </Row>
            ))}
          </div>
        </section>
      )}
      {list.length > 0 && (
        <section>
          {rows.length > 0 && <div className="px-[0.75rem] pb-[0.4rem] text-label text-secondary">From the game</div>}
          <div data-nav-group="overlay-trophies" className="grid grid-cols-3 gap-[0.75rem] px-[0.4rem]">
      {list.map((t) => (
        <button key={t.id} data-nav className="tile flex flex-col overflow-hidden rounded-card bg-l2 text-left">
          <span className="relative block aspect-square w-full bg-l3">
            <MImg src={t.imageUrl} label={t.name} width={240} className="absolute inset-0 size-full" />
          </span>
          <span className="truncate px-[0.6rem] py-[0.5rem] text-meta font-medium">{t.name}</span>
        </button>
      ))}
          </div>
        </section>
      )}
    </div>
  );
}

// ---------- one item: sell, list ----------

type Phase = { at: "idle" } | { at: "building"; what: string } | { at: "signing" } | { at: "error"; message: string } | { at: "done"; text: string };

function plain(e: unknown): string {
  const m = (e as Error)?.message ?? String(e);
  if (/^rare\.market \d+|failed to fetch|networkerror|load failed/i.test(m)) return "Couldn't reach rare.market. Check the connection and try again.";
  return m || "Something went wrong. Try again.";
}

/** What a sale or listing from ItemDetail came to (the Local API's sell answer). */
export type ItemDetailResult = { status: "sold"; digest: string; receivedMist: string } | { status: "listed"; digest: string };

export function ItemDetail({
  item,
  account,
  floors,
  onDone,
  onResult,
  initialPrice,
  payer,
}: {
  item: OverlayItem;
  account: RareAccount | null;
  floors: Floors;
  onDone: () => void;
  onResult?: (r: ItemDetailResult) => void;
  initialPrice?: bigint | null;
  /** A game asked (Local API): its gas budget pays the fee when it covers this. */
  payer?: { slug: string | null; name: string };
}) {
  const [offer, setOffer] = useState<Offer | null | undefined>(undefined);
  const [price, setPrice] = useState<bigint | null>(initialPrice ?? null);
  const [phase, setPhase] = useState<Phase>({ at: "idle" });
  const canSign = canSignWithWallet(account);
  const floor = item.floorMist ? BigInt(item.floorMist) : floors[item.type]?.floorMist ? BigInt(floors[item.type]!.floorMist!) : null;
  const listed = item.listing ? BigInt(item.listing.priceMist) : null;
  // Listed on Tradeport or Drip: their listing holds the item, so it has to come off there first.
  const elsewhere = item.listing && item.listing.venue !== "kiosk" && item.listing.venue !== "market" ? venueName(item.listing.venue) : null;
  const box = useRef<HTMLDivElement>(null);

  // Once the best offer is known, focus lands on Sell (or on the first price when there is none).
  useEffect(() => {
    if (offer !== undefined && box.current) focusFirst(box.current);
  }, [offer]);

  // The best offer, and whether Rare can sell into it: a dry build of the sale (kiosks, policy
  // rules) before the button shows, so an offer we can't fill never gets a Sell button.
  const [blocked, setBlocked] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      const o = await bestOffer(item.id).catch(() => null);
      if (o && account && canSign) {
        const why = await buildAcceptOfferTransaction(item, o, account.address).then(
          () => null,
          (e: unknown) => plain(e),
        );
        if (live) setBlocked(why);
      }
      if (live) setOffer(o);
    })();
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item.id, account?.address]);
  useEffect(() => {
    if (price === null && floor) setPrice(floor);
  }, [floor, price]);

  const busy = phase.at === "building" || phase.at === "signing";
  const run = async (what: "sell" | "list") => {
    if (!account || busy) return;
    setPhase({ at: "building", what: what === "sell" ? "Getting the sale ready..." : "Getting the listing ready..." });
    try {
      const tx = what === "sell" ? await buildAcceptOfferTransaction(item, offer!, account.address) : await buildListTransaction(item, price!, account.address);
      setPhase({ at: "signing" });
      const opts = { account: account.address, origin: { name: "Rare Marketplace", kind: "market" as const }, title: what === "sell" ? `Sell ${item.name}` : `List ${item.name}` };
      const res = payer ? await signAndExecuteForGame(tx, payer, opts) : await signAndExecute(tx, opts);
      if (res.status === "failure") throw new Error("The network rejected this transaction.");
      onResult?.(what === "sell" ? { status: "sold", digest: res.digest ?? "", receivedMist: offerNetMist(offer!).toString() } : { status: "listed", digest: res.digest ?? "" });
      setPhase({ at: "done", text: what === "sell" ? `Sold for ${fmtSui(offerNetMist(offer!), { dp: 3, trim: true })} SUI. It can take a minute to show.` : `Listed for ${fmtSui(price!, { dp: 3, trim: true })} SUI.` });
      feedback("notify");
      setTimeout(onDone, 4000);
    } catch (e) {
      if (e instanceof SignRejected) setPhase({ at: "idle" });
      else setPhase({ at: "error", message: plain(e) });
    }
  };

  const otherPrice = async () => {
    const t = await askPrice(price ?? floor ?? 0n);
    if (t !== null) setPrice(t);
  };

  const net = offer ? offerNetMist(offer) : null;
  const choices: Array<{ label: string; mist: bigint }> = [];
  if (floor) choices.push({ label: "Floor", mist: floor });
  if (floor) choices.push({ label: "Floor +10%", mist: (floor * 110n) / 100n });
  if (listed && listed !== floor) choices.push({ label: "Current", mist: listed });

  return (
    <div ref={box} className="fade-in flex flex-col gap-[1.1rem] px-[0.75rem] pt-[0.4rem]">
      <div className="flex gap-[1rem]">
        <span className="relative size-[7.5rem] shrink-0 overflow-hidden rounded-card bg-l3">
          <MImg src={item.imageUrl} label={item.name} width={400} className="absolute inset-0 size-full" />
        </span>
        <div className="flex min-w-0 flex-col justify-center gap-[0.25rem]">
          <div className="text-row font-semibold">{item.name}</div>
          <div className="truncate text-small text-secondary">{item.collection?.name}</div>
          {item.listing && (
            <div className="text-small text-secondary">
              Listed for <Amount mist={item.listing.priceMist} /> on {venueName(item.listing.venue)}
            </div>
          )}
        </div>
      </div>
      <StatLine
        items={[
          { label: "Floor", value: floor ? <Amount mist={floor} /> : null },
          { label: "Best offer", value: offer ? <Amount mist={offer.priceMist} /> : item.topOfferMist ? <Amount mist={item.topOfferMist} /> : null },
          { label: "Last sale", value: item.lastSaleMist ? <Amount mist={item.lastSaleMist} /> : null },
        ]}
      />

      {!canSign && <AddPasskey need="sell" account={account} />}

      {canSign && elsewhere && <p className="text-small text-secondary">Listed on {elsewhere}. Delist it there to sell or relist it here.</p>}

      {canSign && elsewhere ? null : canSign && phase.at === "done" ? (
        <div className="flex items-center gap-[0.6rem] text-small">
          <Dot tone="positive" />
          {phase.text}
        </div>
      ) : (
        canSign && (
          <>
            <section className="flex flex-col gap-[0.5rem]">
              <div className="text-label font-medium text-secondary">Sell now</div>
              {offer === undefined ? (
                <p className="text-small text-secondary">Looking for offers...</p>
              ) : offer && blocked ? (
                <p className="text-small text-secondary">
                  Best offer {fmtSui(offer.priceMist, { dp: 3, trim: true })} SUI on {venueName(offer.venue)}. {blocked}
                </p>
              ) : offer && net !== null ? (
                <>
                  <Button variant="primary" size="md" def className="w-full" disabled={busy} hint="Sell" onClick={() => void run("sell")}>
                    Sell for {fmtSui(net, { dp: 3, trim: true })} SUI
                  </Button>
                  <p className="text-label text-secondary">
                    {net < BigInt(offer.priceMist)
                      ? `Best offer ${fmtSui(offer.priceMist, { dp: 3, trim: true })} SUI on ${venueName(offer.venue)}. You get ${fmtSui(net, { dp: 3, trim: true })} SUI after fees and royalty.`
                      : `Best offer on ${venueName(offer.venue)}. You get the full price.`}
                  </p>
                </>
              ) : (
                <p className="text-small text-secondary">No offers on this item right now.</p>
              )}
            </section>

            <section className="flex flex-col gap-[0.5rem]">
              <div className="text-label font-medium text-secondary">{item.listing ? "Change the price" : "List for sale"}</div>
              <div data-nav-group="overlay-price" className="flex flex-wrap gap-[0.5rem]">
                {choices.map((c) => (
                  <button
                    key={c.label}
                    data-nav
                    data-hint-a="Pick"
                    onClick={() => setPrice(c.mist)}
                    className={`btn h-[2.25rem] rounded-chip px-[0.8rem] text-small font-medium ${price === c.mist ? "bg-fg text-bg" : "bg-l3 text-fg"}`}
                  >
                    {c.label} <span className="tabular-nums">{fmtSui(c.mist, { dp: 2, trim: true })}</span>
                  </button>
                ))}
                <button data-nav data-hint-a="Type a price" onClick={() => void otherPrice()} className="btn h-[2.25rem] rounded-chip bg-l3 px-[0.8rem] text-small font-medium">
                  Other price
                </button>
              </div>
              <Button variant="secondary" size="md" className="w-full" disabled={busy || !price} hint="List" onClick={() => void run("list")}>
                {price ? `${item.listing ? "Relist" : "List"} for ${fmtSui(price, { dp: 3, trim: true })} SUI` : "Pick a price"}
              </Button>
              <p className="text-label text-secondary">Listed in your own kiosk. Buyers pay the creator royalty on top.</p>
            </section>
          </>
        )
      )}

      {phase.at !== "idle" && phase.at !== "done" && (
        <p role="status" className={`text-small ${phase.at === "error" ? "text-negative" : "text-secondary"}`}>
          {phase.at === "building" ? phase.what : phase.at === "signing" ? "Review and approve in Rare Wallet." : phase.message}
        </p>
      )}
    </div>
  );
}
