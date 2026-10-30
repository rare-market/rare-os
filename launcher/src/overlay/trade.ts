// Selling from the in-game overlay: list an item at a price, or sell it to the best offer.
// Builders are rare.market's own, ported in ../marketplace/venues (list.ts for native kiosk
// listings, build.ts buildAcceptBid for offers on every venue). The seller's kiosks and the type's
// transfer policies come from rare.market's API, with the kiosk list read over gRPC when the API
// can't answer, as in ../marketplace/buy.ts. Nothing here signs or executes.

import type { Transaction } from "@mysten/sui/transactions";
import { native } from "../lib/bridge";
import type { ItemCard, Offer } from "../marketplace/api";
import { buildAcceptOffer, buildList, choosePolicy, loadTradeContext, normType, readPolicy, sellerNetMist, VENUE_LABEL, type ApiListing, type OwnedKioskRef, type RareGet, type TransferPolicy } from "../marketplace/venues";
import { grpcClient } from "../wallet/grpc";
import { ownedKiosks, type NftChainReader } from "../wallet/tx/nft/context";

/** Bid sources the ported buildAcceptBid can fill (rare.market web/lib/sell/best-bid.ts SELLABLE).
 * market/v1 is left out: rare's own contract is not deployed. */
const SELLABLE = new Set(["tradeport/bids", "tradeport/escrow-bids", "tradeport/kiosk-bids", "drip/v2-bids"]);
/** These take wallet-held items only (no kiosk). */
const WALLET_ONLY = new Set(["tradeport/escrow-bids"]);

const normId = (id: string) => "0x" + id.replace(/^0x/i, "").toLowerCase().padStart(64, "0");
const sameId = (a: string | null | undefined, b: string | null | undefined) => !!a && !!b && normId(a) === normId(b);
const sameType = (a: string, b: string) => normType(a) === normType(b);

function chainKiosks(sender: string): Promise<OwnedKioskRef[]> {
  const core = (grpcClient as unknown as { core: NftChainReader }).core;
  return ownedKiosks(core, sender).then((r) => r.kiosks);
}

/** Sources whose seller net depends on the type's royalty (see sellerNetMist in ../marketplace/venues),
 * and which can't take items whose policy needs a personal kiosk. */
const ROYALTY_SENSITIVE = new Set(["tradeport/kiosk-bids", "drip/v2-bids"]);

/**
 * What the seller receives for the offer, before gas. Venue fee, royalty and commission are paid by
 * the bidder on top of the price on most sources, so this is the price. Offers from bestOffer carry
 * the exact figure for the two sources where the royalty can come out of the seller's side
 * (Tradeport kiosk bids with a stale royalty reserve, Drip v2 bids).
 */
export function offerNetMist(offer: Offer): bigint {
  const exact = offer.extra?.sellerNetMist;
  if (typeof exact === "string" && /^-?\d+$/.test(exact)) return BigInt(exact);
  return sellerNetMist(offer);
}

/** Whether this offer can take the item (rare.market web/lib/sell/best-bid.ts canFill). */
function canFill(o: Offer, item: { id: string; type?: string; kioskId?: string | null; owner?: string | null }, now = Date.now()): boolean {
  if (o.status !== "open" || !o.fillable) return false;
  if ((o.filled ?? 0) >= (o.quantity ?? 1)) return false;
  if (o.expiresAt && Date.parse(o.expiresAt) <= now) return false;
  if (!SELLABLE.has(o.source)) return false;
  if (WALLET_ONLY.has(o.source) && item.kioskId) return false;
  if (o.nftType && item.type && !sameType(o.nftType, item.type)) return false;
  if (item.owner && sameId(o.bidder, item.owner)) return false;
  if (o.kind === "item") return sameId(o.item?.id, item.id);
  // Trait bids need the item's attributes; the API sends none for ?item= today.
  return o.kind === "collection";
}

/** The best offer the item can be sold into right now, or null. Ranked by what the seller receives. */
export async function bestOffer(itemId: string, get: RareGet = native.rareGet): Promise<Offer | null> {
  const id = encodeURIComponent(itemId);
  const [offers, item] = await Promise.all([
    get<{ data: Offer[] }>(`/v1/offers?item=${id}&limit=10`),
    // Where the item is and who holds it: wallet-only bids and the owner's own bids are skipped.
    get<ItemCard>(`/v1/items/${id}`).catch(() => null),
  ]);
  let fillable = (offers.data ?? []).filter((o) => canFill(o, { id: itemId, type: item?.type, kioskId: item?.kioskId, owner: item?.owner }));
  const type = item?.type ?? fillable.find((o) => o.nftType)?.nftType;
  if (type && fillable.some((o) => ROYALTY_SENSITIVE.has(o.source))) {
    const pols = await get<{ data: TransferPolicy[] }>(`/v1/policies/${encodeURIComponent(type)}`).then((r) => r.data).catch(() => null);
    if (pols) {
      const policy = choosePolicy(pols);
      // Tradeport kiosk bids and Drip v2 bids can't deliver into the bidder's personal kiosk.
      if (readPolicy(policy).personal) fillable = fillable.filter((o) => !ROYALTY_SENSITIVE.has(o.source));
      fillable = fillable.map((o) => (ROYALTY_SENSITIVE.has(o.source) ? { ...o, extra: { ...o.extra, sellerNetMist: sellerNetMist(o, policy).toString() } } : o));
    }
  }
  // The API sorts by price, highest first; keep that order among equal nets.
  let best: Offer | null = null;
  for (const o of fillable) if (!best || offerNetMist(o) > offerNetMist(best)) best = o;
  return best;
}

/** The kiosk of the sender's that holds the item, or null for a wallet item. */
async function sellerKiosk(kioskId: string | null | undefined, kiosks: OwnedKioskRef[], sender: string): Promise<OwnedKioskRef | null> {
  if (!kioskId) return null;
  const hit = kiosks.find((k) => sameId(k.kioskId, kioskId));
  if (hit) return hit;
  // rare.market's kiosk list can trail the chain by a few seconds: ask the node before giving up.
  const fresh = await chainKiosks(sender).catch(() => [] as OwnedKioskRef[]);
  const k = fresh.find((x) => sameId(x.kioskId, kioskId));
  if (!k) throw new Error("Your wallet doesn't hold the kiosk this item is in.");
  return k;
}

function currentListing(item: ItemCard): ApiListing | null {
  return (item.listing as ApiListing | null | undefined) ?? null;
}

function needOffVenue(l: ApiListing | null): void {
  if (l && l.venue !== "kiosk") throw new Error(`Delist it on ${VENUE_LABEL[l.venue] ?? l.venue} first.`);
}

/**
 * List the item at `priceMist` with 0x2::kiosk::list in the seller's own kiosk (no marketplace fee).
 * A wallet item is placed (or locked) into the seller's kiosk first, and a kiosk is created in the
 * same transaction when they have none. An item already listed natively is relisted at the new price.
 */
export async function buildListTransaction(item: ItemCard, priceMist: bigint, sender: string, get: RareGet = native.rareGet): Promise<Transaction> {
  if (priceMist <= 0n) throw new Error("Price must be above zero.");
  if (!item.type) throw new Error("This item is still being indexed. Try again in a minute.");
  const listing = currentListing(item);
  needOffVenue(listing);
  const ctx = await loadTradeContext(get, sender, [item.type], chainKiosks);
  const kiosk = await sellerKiosk(listing?.kioskId ?? item.kioskId, ctx.buyerKiosks, sender);
  const list = ctx.policies[item.type];
  const policy = choosePolicy(list) ?? list?.[0] ?? null;
  return buildList(
    sender,
    [{ nftId: item.id, nftType: item.type, priceMist, kiosk, policy, listing: listing ? { venue: listing.venue, orderKey: listing.orderKey } : undefined }],
    ctx.buyerKiosks,
  );
}

/**
 * Sell the item into `offer` on whatever venue it lives. The NFT always reaches the bidder and the
 * seller receives offerNetMist(offer). An item listed natively in the seller's kiosk is delisted in
 * the same transaction; one listed on Tradeport or Drip must be delisted there first.
 */
export async function buildAcceptOfferTransaction(item: ItemCard, offer: Offer, sender: string, get: RareGet = native.rareGet): Promise<Transaction> {
  if (!item.type) throw new Error("This item is still being indexed. Try again in a minute.");
  if (offer.status !== "open" || !offer.fillable) throw new Error("This offer can't be filled any more.");
  if (offer.nftType && !sameType(offer.nftType, item.type)) throw new Error("This offer is for another collection.");
  if (offer.kind === "item" && offer.item && !sameId(offer.item.id, item.id)) throw new Error("This offer is for another item.");
  if (sameId(offer.bidder, sender)) throw new Error("You can't accept your own offer.");
  const listing = currentListing(item);
  needOffVenue(listing);
  const ctx = await loadTradeContext(get, sender, [item.type], chainKiosks);
  const kiosk = await sellerKiosk(listing?.kioskId ?? item.kioskId, ctx.buyerKiosks, sender);
  return buildAcceptOffer(
    { ...offer, nftType: offer.nftType ? item.type : null },
    { sender, kiosk, nftId: item.id, nftType: item.type, policies: ctx.policies, listedNative: listing?.venue === "kiosk" },
  );
}
