// The item sheet: price and floor, best offer, last sale, and the four things you can do with
// an item you hold: sell into the best offer, list or change the price, cancel the listing, send.
// Every action shows on screen at once (store.runOptimistic) and rolls back if it fails.

import { useEffect, useState } from "react";
import { useApp } from "../lib/store";
import { askPrice } from "../overlay/price";
import { fmtSui, venueName } from "../marketplace/format";
import { MImg } from "../marketplace/MImg";
import { paths } from "../marketplace/api";
import { StatLine } from "../ui/StatLine";
import { Sui } from "../ui";
import { SignRejected } from "../wallet/sign";
import { canSignAccount } from "../wallet/ui/dev";
import { LoadBar } from "../wallet/ui/home/lib";
import { Btn, Sheet } from "../wallet/ui/kit";
import { useWallet, useWalletNav } from "../wallet/ui/store";
import { isStaked, unstakeLine } from "./logic";
import { currentView, openItemSheet, runOptimistic, source } from "./store";
import { tradeApi, type Quote } from "./trade";
import type { InvItem } from "./types";
import { AddPasskey } from "../wallet/ui/AddPasskey";

type Phase = { at: "idle" } | { at: "working"; text: string } | { at: "error"; message: string } | { at: "done"; text: string };

export function plain(e: unknown): string {
  const m = (e as Error)?.message ?? String(e);
  if (/^rare\.market \d+|failed to fetch|networkerror|load failed/i.test(m)) return "Couldn't reach rare.market. Check the connection and try again.";
  return m || "That didn't go through. Try again.";
}

const find = (id: string): InvItem | undefined => currentView().groups.flatMap((g) => g.items).find((i) => i.id === id);

export function ItemSheet({ id }: { id: string }) {
  const account = useWallet((s) => s.current);
  const push = useWalletNav((s) => s.push);
  const toast = useApp((s) => s.toast);
  const [item, setItem] = useState<InvItem | null | undefined>(() => find(id));
  const [quote, setQuote] = useState<Quote | null | undefined>(undefined);
  const [price, setPrice] = useState<bigint | null>(null);
  const [phase, setPhase] = useState<Phase>({ at: "idle" });
  const canSign = canSignAccount(account);
  const close = () => openItemSheet(null);

  useEffect(() => {
    let live = true;
    void (async () => {
      const it = find(id) ?? (await source()<InvItem>(paths.item(id)).catch(() => null));
      if (!live) return;
      setItem(it);
      if (it && account && canSign && !isStaked(it)) {
        const q = await tradeApi().quote(it, account.address).catch(() => null);
        if (live) setQuote(q);
      } else if (live) setQuote(null);
    })();
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, account?.address]);

  const floor = item?.floorMist ? BigInt(item.floorMist) : null;
  useEffect(() => {
    if (price === null && floor) setPrice(floor);
  }, [floor, price]);

  if (item === null) return <Sheet title="Item" onClose={close}><p className="text-sm text-secondary">Couldn't load this item.</p></Sheet>;
  if (!item) return <Sheet title="Item" onClose={close}><LoadBar on /></Sheet>;

  const busy = phase.at === "working";
  const listing = item.listing;
  const staked = isStaked(item);
  const native = listing?.venue === "kiosk";
  const elsewhere = listing && !native ? venueName(listing.venue) : null;

  const act = async (text: string, op: Parameters<typeof runOptimistic>[0], run: (sender: string) => Promise<void>, done: string) => {
    if (!account || busy) return;
    setPhase({ at: "working", text });
    try {
      await runOptimistic(op, () => run(account.address));
      setPhase({ at: "done", text: done });
      toast(done);
      setTimeout(close, 900);
    } catch (e) {
      setPhase(e instanceof SignRejected ? { at: "idle" } : { at: "error", message: plain(e) });
    }
  };

  const sell = () =>
    quote &&
    act("Review and approve in Rare Wallet.", { kind: "remove", ids: [item.id], why: "sold" }, (s) => tradeApi().sell(item, quote.offer, s), `Sold ${item.name} for ${fmtSui(quote.netMist, { dp: 3, trim: true })} SUI`);
  const list = () =>
    price &&
    act("Review and approve in Rare Wallet.", { kind: "list", id: item.id, priceMist: price.toString() }, (s) => tradeApi().list(item, price, s), `Listed ${item.name} for ${fmtSui(price, { dp: 3, trim: true })} SUI`);
  const cancel = () => act("Review and approve in Rare Wallet.", { kind: "delist", id: item.id }, (s) => tradeApi().cancel(item, s), `Cancelled the listing of ${item.name}`);
  const send = () => {
    close();
    push({ kind: "send-nfts", ids: [item.id] });
  };

  const choices: Array<{ label: string; mist: bigint }> = [];
  if (floor) choices.push({ label: "Floor", mist: floor }, { label: "Floor +10%", mist: (floor * 110n) / 100n });
  const other = async () => {
    const t = await askPrice(price ?? floor ?? 0n);
    if (t !== null) setPrice(t);
  };

  return (
    <Sheet title={item.name} subtitle={item.collection?.name} onClose={close} width="40rem" ready={quote === undefined ? 0 : 1}>
      <LoadBar on={quote === undefined && canSign} />
      <div className="flex gap-5">
        <div className="relative size-[8rem] shrink-0 overflow-hidden rounded-lg bg-layer-03">
          <MImg src={item.imageUrl} label={item.collection?.name ?? item.name} width={400} className="absolute inset-0 size-full" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-sm text-secondary">{listing ? `Listed on ${venueName(listing.venue)}` : staked ? "Staked" : "Not listed"}</p>
          {listing && <Sui mist={listing.allInMist ?? listing.priceMist} dp={3} className="text-xl font-semibold" />}
          <StatLine
            className="mt-3"
            items={[
              { label: "Floor", value: item.floorMist ? <Sui mist={item.floorMist} dp={3} /> : null },
              { label: "Best offer", value: quote ? <Sui mist={quote.offer.priceMist} dp={3} /> : item.topOfferMist ? <Sui mist={item.topOfferMist} dp={3} /> : null },
              { label: "Last sale", value: item.lastSaleMist ? <Sui mist={item.lastSaleMist} dp={3} /> : null },
            ]}
          />
        </div>
      </div>

      {staked && <p data-testid="staked-hint" className="mt-5 text-sm text-secondary">{unstakeLine(item)}</p>}
      {!staked && !canSign && <AddPasskey need="sell" account={account} className="mt-5" />}

      {!staked && canSign && phase.at !== "done" && (
        <div className="mt-6 flex flex-col gap-5">
          <section className="flex flex-col gap-2">
            <h3 className="text-sm font-medium text-secondary">Sell now</h3>
            {quote === undefined ? (
              <p className="text-sm text-secondary">Looking for offers...</p>
            ) : quote && quote.blocked ? (
              <p className="text-sm text-secondary">Best offer {fmtSui(quote.offer.priceMist, { dp: 3, trim: true })} SUI. {quote.blocked}</p>
            ) : quote ? (
              <>
                <Btn variant="primary" size="md" def onClick={() => void sell()}>
                  Sell for {fmtSui(quote.netMist, { dp: 3, trim: true })} SUI
                </Btn>
                <p className="text-label text-secondary">
                  {quote.sponsored ? "Network fee paid by Rare. " : ""}
                  {quote.netMist < BigInt(quote.offer.priceMist) ? `Best offer ${fmtSui(quote.offer.priceMist, { dp: 3, trim: true })} SUI on ${venueName(quote.offer.venue)}. You get this after fees and royalty.` : `Best offer on ${venueName(quote.offer.venue)}. You get the full price.`}
                </p>
              </>
            ) : (
              <p className="text-sm text-secondary">No offers on this item right now.</p>
            )}
          </section>

          {elsewhere ? (
            <p className="text-sm text-secondary">Listed on {elsewhere}. Cancel it there to list or send it here.</p>
          ) : (
            <section className="flex flex-col gap-2">
              <h3 className="text-sm font-medium text-secondary">{listing ? "Change the price" : "List for sale"}</h3>
              <div data-nav-group="inv-price" className="flex flex-wrap gap-2">
                {choices.map((c) => (
                  <Btn key={c.label} size="md" variant={price === c.mist ? "primary" : "secondary"} onClick={() => setPrice(c.mist)}>
                    {c.label} {fmtSui(c.mist, { dp: 2, trim: true })}
                  </Btn>
                ))}
                <Btn size="md" onClick={() => void other()}>Other price</Btn>
              </div>
              <Btn size="md" onClick={() => void list()}>
                {price ? `${listing ? "Relist" : "List"} for ${fmtSui(price, { dp: 3, trim: true })} SUI` : "Pick a price"}
              </Btn>
            </section>
          )}

          <div className="flex flex-wrap gap-2 border-t border-border-primary pt-4">
            {native && <Btn size="md" onClick={() => void cancel()}>Cancel listing</Btn>}
            {!listing && <Btn size="md" onClick={send}>Send</Btn>}
          </div>
        </div>
      )}

      {phase.at !== "idle" && (
        <p role="status" className={`mt-4 text-sm ${phase.at === "error" ? "text-negative" : phase.at === "done" ? "text-primary" : "text-secondary"}`}>
          {phase.at === "working" ? phase.text : phase.at === "error" ? phase.message : phase.text}
        </p>
      )}
    </Sheet>
  );
}

export function InventorySheetHost() {
  const id = useInventoryId();
  return id ? <ItemSheet key={id} id={id} /> : null;
}

import { useInventory } from "./store";
const useInventoryId = () => useInventory((s) => s.sheetId);
