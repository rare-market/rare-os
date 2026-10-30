// Ported from rare.market: market/web/lib/venues/index.ts (buildCartBuy,
// buildAcceptOffer, toVenuePolicy, toActiveListing, toActiveBid, the buyer kiosk choice) and
// web/lib/wallet/execute.ts (loadTradeContext). Builders are in ./build.ts
// (packages/venues/src/build.ts); native listing is in ./list.ts (web/lib/chain/listing.ts).
//
// Inputs come from rare.market's API: the listing (on the item), the buyer's kiosks
// (GET /v1/accounts/:address/kiosks) and the transfer policies of the NFT type
// (GET /v1/policies/:type). The web app's buyer-kiosk plumbing in web/lib/chain/purchase.ts
// (planBuyerKiosk) is not on this path: buildCartBuy picks the kiosk itself, as ported below.

import { Transaction } from "@mysten/sui/transactions";
import { buildAcceptBid, buildBuy, UnsupportedError, type UserKiosk } from "./build";
import type { ActiveBid, ActiveListing, ApiListing, ApiOffer, OwnedKioskRef, PoliciesByType, Source, TransferPolicy, Venue } from "./model";
import { choosePolicy, isKnownRule, normType, readPolicy, type Policy } from "./policy";

export { allInPrice, buildAcceptBid, buildBuy, configureOurs, royaltyOf, UnsupportedError, venueFee, type SellContext as BidSellContext } from "./build";
export { buildList, delistFromOwnKiosk, KioskScope, listInOwnKiosk, type KioskArg, type ListingItem } from "./list";
export type * from "./model";
export { choosePolicy, configureResale, normType, readPolicy, resaleLicenceType, RULES, type Policy } from "./policy";

export const VENUE_LABEL: Record<string, string> = { market: "Rare", tradeport: "Tradeport", drip: "Drip", kiosk: "Kiosk" };
const LABEL = VENUE_LABEL;

/** Builder errors in plain words. */
function friendly(e: unknown, venue: string, action: "buy" | "sell" = "buy"): Error {
  if (e instanceof UnsupportedError || /not deployed|not supported/i.test((e as Error)?.message ?? "")) {
    const name = LABEL[venue] ?? venue;
    return new Error(action === "buy" ? `Buying from ${name} is not available for this item yet.` : `Selling into offers on ${name} is not available for this item yet.`);
  }
  return e instanceof Error ? e : new Error(String(e));
}

/** A /v1/policies policy to the builder's shape. */
export function toVenuePolicy(p: TransferPolicy | null, nftType: string): Policy | null {
  if (!p) return null;
  const shape = readPolicy(p);
  const rules = p.rules.map((r) => normType(r.ruleType ?? r.type)).map((t) => (t.endsWith("::Rule") ? t : `${t.split("<")[0]}`));
  return {
    policyId: p.id,
    nftType: normType(nftType),
    rules,
    unknownRules: rules.filter((r) => !isKnownRule(r)),
    royalty: shape.royalty ? { bps: Number(shape.royalty.bps), min: shape.royalty.min } : null,
    floor: shape.floor,
    sharedVersion: null,
  };
}

function toUserKiosk(k: OwnedKioskRef | null): UserKiosk | null {
  return k ? { kioskId: k.kioskId, capId: k.ownerCapId, personalCapId: k.personalCapId ?? null, personal: k.isPersonal } : null;
}

export function toActiveListing(itemId: string, l: ApiListing): ActiveListing {
  return {
    venue: l.venue as Venue,
    source: (l.source ?? "") as Source,
    listingId: l.orderKey ?? itemId,
    objectId: l.objectId ?? null,
    nftId: itemId,
    nftType: l.nftType ?? null,
    seller: l.seller ?? null,
    kioskId: l.kioskId ?? null,
    price: l.priceMist,
    fee: l.feeMist ?? "0",
    commission: l.commissionMist ?? "0",
    coinType: "0x2::sui::SUI",
    extra: l.extra ?? {},
  };
}

export function toActiveBid(o: ApiOffer): ActiveBid {
  return {
    venue: o.venue as Venue,
    source: o.source as Source,
    bidId: o.id,
    kind: o.kind === "item" ? "item" : "collection",
    nftType: o.nftType,
    nftId: o.item?.id ?? null,
    bidder: o.bidder,
    price: o.priceMist,
    fee: o.feeMist,
    royalty: o.royaltyMist,
    commission: o.commissionMist,
    coinType: "0x2::sui::SUI",
    poolId: o.poolId,
    expireAt: o.expiresAt ? String(Date.parse(o.expiresAt)) : null,
    fillable: o.fillable && o.status === "open",
    extra: { ...(o.extra ?? {}), destKiosk: o.destKioskId ?? (o.extra as { destKiosk?: string } | undefined)?.destKiosk ?? null },
  };
}

export interface BuyLine {
  itemId: string;
  listing: ApiListing;
  /** Rare Store ResaleLicence: the Game object (resale::prove needs it). */
  resaleGame?: string;
}

export interface BuyContext {
  sender: string;
  buyerKiosks: OwnedKioskRef[];
  policies: PoliciesByType;
}

/** The buyer kiosk the whole cart uses: personal when any policy needs one. */
function buyerKiosk(lines: BuyLine[], ctx: BuyContext): OwnedKioskRef | null {
  const needsPersonal = lines.some((l) => readPolicy(choosePolicy(ctx.policies[l.listing.nftType ?? ""])).personal);
  const personal = ctx.buyerKiosks.find((k) => k.isPersonal && k.personalCapId);
  const plain = ctx.buyerKiosks.find((k) => !k.isPersonal);
  return needsPersonal ? personal ?? null : plain ?? personal ?? null;
}

/** One PTB buying every line, whatever venue each listing is on. */
export function buildCartBuy(lines: BuyLine[], ctx: BuyContext, tx = new Transaction()): Transaction {
  if (!lines.length) throw new Error("Nothing to buy.");
  tx.setSender(ctx.sender);
  const kiosk = toUserKiosk(buyerKiosk(lines, ctx));
  for (const line of lines) {
    const type = line.listing.nftType ?? "";
    const policy = toVenuePolicy(choosePolicy(ctx.policies[type]), type);
    try {
      buildBuy(tx, toActiveListing(line.itemId, line.listing), { sender: ctx.sender, kiosk, policy, resaleGame: line.resaleGame });
    } catch (e) {
      throw friendly(e, line.listing.venue);
    }
  }
  return tx;
}

const bigOr0 = (v: unknown): bigint => {
  try {
    return v == null || v === "" ? 0n : BigInt(String(v));
  } catch {
    return 0n;
  }
};

/** royalty_rule::fee_amount: bps of `paid`, at least the minimum. */
function royaltyDue(paid: bigint, policy: TransferPolicy | null | undefined): bigint {
  const r = readPolicy(policy).royalty;
  if (!r) return 0n;
  const amount = (paid * r.bps) / 10000n;
  return amount < r.min ? r.min : amount;
}

/**
 * What the seller receives for an offer, before gas. rare.market (web/lib/sell/net.ts) takes the
 * price on every source; mainnet simulation of the ported builders refines two sources:
 * - tradeport/kiosk-bids: the seller pays the royalty (on the price) from gas and gets back the
 *   bid's royalty reserve (escrow minus price minus commission). A bid placed under an older,
 *   lower royalty nets less than its price.
 * - drip/v2-bids: the royalty is paid out of the sale proceeds (not simulated live: no open bids).
 * Tradeport pooled bids and escrow bids pay everything from the bid: the price.
 * `policy` is the type's transfer policy; without it the reserve is assumed to cover the royalty.
 */
export function sellerNetMist(o: ApiOffer, policy?: TransferPolicy | null): bigint {
  const price = bigOr0(o.priceMist);
  if (policy === undefined) return price;
  if (o.source === "tradeport/kiosk-bids") {
    const escrow = o.extra?.escrow;
    const reserve = escrow == null ? royaltyDue(price, policy) : bigOr0(escrow) - price - bigOr0(o.commissionMist);
    return price + (reserve > 0n ? reserve : 0n) - royaltyDue(price, policy);
  }
  if (o.source === "drip/v2-bids") return price - royaltyDue(price, policy);
  return price;
}

export interface SellContext {
  sender: string;
  /** The seller's kiosk holding the item, or null for a wallet item. */
  kiosk: OwnedKioskRef | null;
  nftId: string;
  nftType: string;
  policies: PoliciesByType;
  /** The item is listed with plain kiosk::list and must be delisted first. */
  listedNative?: boolean;
}

/** Sell an item into an offer or bid on any venue. The item always reaches the bidder. */
export function buildAcceptOffer(offer: ApiOffer, ctx: SellContext, tx = new Transaction()): Transaction {
  tx.setSender(ctx.sender);
  const policy = toVenuePolicy(choosePolicy(ctx.policies[ctx.nftType]), ctx.nftType);
  const k = toUserKiosk(ctx.kiosk);
  try {
    buildAcceptBid(tx, toActiveBid(offer), {
      sender: ctx.sender,
      policy,
      nft: k ? { at: "kiosk", kiosk: k, nftId: ctx.nftId, nftType: ctx.nftType, listedNative: ctx.listedNative } : { at: "wallet", objectId: ctx.nftId, nftType: ctx.nftType, kiosk: null },
    });
  } catch (e) {
    throw friendly(e, offer.venue, "sell");
  }
  return tx;
}

/** Reads rare.market API paths (/v1/...): native.rareGet in the app, fetch in tests. */
export type RareGet = <T>(path: string) => Promise<T>;

/** What the builder needs for a sender: their kiosks and the policies of the given types.
 * `chainKiosks` (a gRPC read) stands in when rare.market can't list the kiosks. */
export async function loadTradeContext(
  get: RareGet,
  sender: string,
  nftTypes: string[],
  chainKiosks?: (sender: string) => Promise<OwnedKioskRef[]>,
): Promise<{ buyerKiosks: OwnedKioskRef[]; policies: PoliciesByType }> {
  const types = Array.from(new Set(nftTypes.filter(Boolean)));
  const listKiosks = get<{ data: OwnedKioskRef[] }>(`/v1/accounts/${encodeURIComponent(sender)}/kiosks`).then((r) => r.data);
  const [kiosks, ...pols] = await Promise.all([
    chainKiosks ? listKiosks.catch(() => chainKiosks(sender)) : listKiosks,
    ...types.map((t) => get<{ data: TransferPolicy[] }>(`/v1/policies/${encodeURIComponent(t)}`)),
  ]);
  const policies: PoliciesByType = {};
  types.forEach((t, i) => {
    policies[t] = pols[i].data;
  });
  return {
    buyerKiosks: kiosks.map((k) => ({ kioskId: k.kioskId, ownerCapId: k.ownerCapId, personalCapId: k.personalCapId ?? null, isPersonal: k.isPersonal })),
    policies,
  };
}
