// What the inventory can do with an item: quote the best offer, sell into it, list, cancel a
// listing. Real builders come from the overlay and marketplace modules; every transaction goes
// through the wallet's review sheet. Mock mode swaps in ./mock so the flows run without a chain.

import type { Transaction } from "@mysten/sui/transactions";
import { native } from "../lib/bridge";
import type { Offer } from "../marketplace/api";
import { KioskScope, delistFromOwnKiosk, loadTradeContext, type OwnedKioskRef } from "../marketplace/venues";
import { bestOffer, buildAcceptOfferTransaction, buildListTransaction, offerNetMist } from "../overlay/trade";
import { grpcClient } from "../wallet/grpc";
import { signAndExecuteWithCover } from "../wallet/sign";
import { sponsorOffer } from "../wallet/sponsor";
import { ownedKiosks, type NftChainReader } from "../wallet/tx/nft/context";
import { mock, useMock } from "./mock";
import type { InvItem } from "./types";

export interface Quote {
  offer: Offer;
  /** What the seller receives, MIST. */
  netMist: bigint;
  /** Plain reason this offer can't be filled here, or null. */
  blocked: string | null;
  /** Rare's gas sponsor will pay the network fee for this sale. */
  sponsored: boolean;
}

export interface TradeApi {
  quote(item: InvItem, sender: string): Promise<Quote | null>;
  sell(item: InvItem, offer: Offer, sender: string): Promise<void>;
  list(item: InvItem, priceMist: bigint, sender: string): Promise<void>;
  cancel(item: InvItem, sender: string): Promise<void>;
}

const origin = { name: "Rare Marketplace", kind: "market" } as const;
const norm = (id: string) => id.replace(/^0x/i, "").toLowerCase().padStart(64, "0");

async function run(tx: Transaction, sender: string, title: string) {
  // Rare pays the fee when the wallet is short of SUI and the sponsor offers it; otherwise the wallet does.
  const res = await signAndExecuteWithCover(tx, "trade", { account: sender, origin, title });
  if (res.status === "failure") throw new Error("The network rejected this transaction.");
}

/** Takes a native kiosk listing off sale. Listings on Tradeport or Drip come off there. */
export async function buildCancelTransaction(item: InvItem, sender: string, get = native.rareGet, kiosksOf?: (s: string) => Promise<OwnedKioskRef[]>): Promise<Transaction> {
  const l = item.listing;
  if (!l) throw new Error("This item isn't listed.");
  if (l.venue !== "kiosk") throw new Error("Cancel it where it is listed.");
  if (!item.type) throw new Error("This item is still being indexed. Try again in a minute.");
  const chain = kiosksOf ?? ((s: string) => ownedKiosks((grpcClient as unknown as { core: NftChainReader }).core, s).then((r) => r.kiosks));
  const ctx = await loadTradeContext(get, sender, [], chain);
  const kioskId = l.kioskId ?? item.kioskId;
  const ref = ctx.buyerKiosks.find((k) => !!kioskId && norm(k.kioskId) === norm(kioskId)) ?? (kioskId ? (await chain(sender).catch(() => [])).find((k) => norm(k.kioskId) === norm(kioskId)) : undefined);
  if (!ref) throw new Error("Your wallet doesn't hold the kiosk this item is in.");
  const { Transaction } = await import("@mysten/sui/transactions");
  const tx = new Transaction();
  tx.setSender(sender);
  const scope = new KioskScope(tx, sender);
  delistFromOwnKiosk(tx, scope.open(ref), { nftId: item.id, nftType: item.type });
  scope.close();
  return tx;
}

export const realTrade: TradeApi = {
  async quote(item, sender) {
    const offer = await bestOffer(item.id).catch(() => null);
    if (!offer) return null;
    const blocked = await buildAcceptOfferTransaction(item, offer, sender).then(
      () => null,
      (e: unknown) => (e as Error).message || "This offer can't be filled here.",
    );
    const balance = async () => BigInt((await grpcClient.core.getBalance({ owner: sender, coinType: "0x2::sui::SUI" })).balance.balance);
    const sponsored = !!(await sponsorOffer(sender, "trade", { suiBalanceMist: balance }));
    return { offer, netMist: offerNetMist(offer), blocked, sponsored };
  },
  async sell(item, offer, sender) {
    await run(await buildAcceptOfferTransaction(item, offer, sender), sender, `Sell ${item.name}`);
  },
  async list(item, price, sender) {
    await run(await buildListTransaction(item, price, sender), sender, `List ${item.name}`);
  },
  async cancel(item, sender) {
    await run(await buildCancelTransaction(item, sender), sender, `Cancel listing of ${item.name}`);
  },
};

export const tradeApi = (): TradeApi => (useMock ? mock.trade : realTrade);
