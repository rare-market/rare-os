// Ported from rare.market: market/packages/venues/src/model.ts (the listing and
// bid shapes) and the rare.market API Listing (web/lib/api/types.ts).

/** A marketplace rare.market aggregates. `market` is rare's own contract. */
export type Venue = "tradeport" | "drip" | "kiosk" | "market";

/** Where a listing or bid lives inside a venue; the builders switch on it. */
export type Source =
  | "tradeport/escrow"
  | "tradeport/kiosk"
  | "tradeport/kiosk-early"
  | "tradeport/simple"
  | "drip/v2"
  | "tradeport/escrow-bids"
  | "tradeport/kiosk-bids"
  | "tradeport/bids"
  | "drip/v2-bids"
  | "kiosk/native"
  | "market/v1";

export type BidKind = "collection" | "item";

/** One open listing as the builder sees it. Amounts are MIST as decimal strings. */
export type ActiveListing = {
  venue: Venue;
  source: Source;
  /** The venue's own key for the listing (orderKey in the API). */
  listingId: string;
  objectId: string | null;
  nftId: string;
  nftType: string | null;
  seller: string | null;
  kioskId: string | null;
  /** Price as the venue records it. */
  price: string;
  /** Marketplace fee the buyer pays on top of `price`. */
  fee: string;
  /** Marketplace cut taken out of `price` (legacy commission). */
  commission: string;
  coinType: string;
  /** Venue details the builder needs (listingType, inOrderbook, ...). */
  extra: Record<string, unknown>;
};

/** One open bid or offer as the builder sees it. Amounts are MIST as decimal strings. */
export type ActiveBid = {
  venue: Venue;
  source: Source;
  bidId: string;
  kind: BidKind;
  nftType: string | null;
  /** Item bids and offers only. */
  nftId: string | null;
  bidder: string;
  /** What the seller is offered before venue cuts. */
  price: string;
  fee: string;
  royalty: string;
  commission: string;
  coinType: string;
  poolId: string | null;
  expireAt: string | null;
  /** False when the bid exists but cannot be filled (pool short, frozen store). */
  fillable: boolean;
  extra: Record<string, unknown>;
};

/** A listing as rare.market's API sends it (GET /v1/items/:id, `listing`). */
export interface ApiListing {
  venue: string;
  source?: string;
  orderKey?: string;
  objectId?: string | null;
  priceMist: string;
  allInMist?: string;
  feeMist?: string;
  commissionMist?: string;
  seller?: string;
  kioskId?: string | null;
  nftType?: string;
  extra?: Record<string, unknown>;
}

/** An offer as rare.market's API sends it (GET /v1/offers), the fields the sell builder reads. */
export interface ApiOffer {
  id: string;
  venue: string;
  source: string;
  nftType: string | null;
  feeMist: string;
  royaltyMist: string;
  commissionMist: string;
  poolId: string | null;
  fillable: boolean;
  extra: Record<string, unknown>;
  kind: "collection" | "item" | "trait";
  item: { id: string } | null;
  priceMist: string;
  bidder: string;
  destKioskId: string | null;
  expiresAt: string | null;
  status: "open" | "filled" | "cancelled" | "expired";
}

/** GET /v1/accounts/:address/kiosks */
export interface OwnedKioskRef {
  kioskId: string;
  /** KioskOwnerCap id. For a personal kiosk the cap is wrapped in the PersonalKioskCap. */
  ownerCapId: string;
  personalCapId?: string | null;
  isPersonal: boolean;
}

/** GET /v1/policies/:type */
export interface PolicyRule {
  type: string;
  /** Full Move type of the rule, e.g. 0x434b...::royalty_rule::Rule. */
  ruleType?: string;
  config?: Record<string, unknown>;
}

export interface TransferPolicy {
  id: string;
  isShared?: boolean;
  rules: PolicyRule[];
}

export type PoliciesByType = Record<string, TransferPolicy[] | null | undefined>;
