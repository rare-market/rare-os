// Ported from rare.market: market/packages/venues/src/build.ts (buildBuy,
// buildAcceptBid and what they call; simulation left out) and packages/venues/src/ours.ts (buildBuy
// and buildAcceptBid for rare's own contract, off while its ids are empty, as on rare.market today).
//
// The builder only adds commands to a Transaction; it never signs or executes. Rules it keeps:
// - Calls target each venue's latest package (versioned stores reject older code).
// - Royalties are computed on chain with `royalty_rule::fee_amount` on what the request records.
// - A bought kiosk NFT always ends in the buyer's kiosk (a new one is created and shared when the
//   buyer has none). Escrow listings deliver to the buyer's wallet.
// - Selling into a bid always delivers the NFT to the bidder.

import type { Transaction, TransactionArgument, TransactionObjectArgument } from "@mysten/sui/transactions";
import { CLOCK, DRIP_V2, KIOSK_EXT, TRADEPORT } from "./ids";
import type { ActiveBid, ActiveListing } from "./model";
import { addResaleProve, resaleRule, RULES, type Policy } from "./policy";

const KIOSK = "0x2::kiosk::Kiosk";

export class UnsupportedError extends Error {}

/** A kiosk the buyer controls. `personalCapId` set means the KioskOwnerCap is wrapped in a
 * PersonalKioskCap (borrowed for the PTB and returned). */
export type UserKiosk = { kioskId: string; capId: string; personalCapId?: string | null; personal?: boolean };

export type BuyContext = {
  sender: string;
  /** The buyer's kiosk, or null to create one in the PTB. */
  kiosk: UserKiosk | null;
  /** Transfer policy of the NFT type (required for kiosk NFTs). */
  policy: Policy | null;
  /** Drip v2: buy the native kiosk listing directly (no fee) and clear Drip's record. Default true. */
  dripDirect?: boolean;
  /** Rare Store ResaleLicence: the Game object the licence belongs to (resale::prove needs it). */
  resaleGame?: string;
};

export type SellContext = {
  sender: string;
  policy: Policy | null;
  /** Where the seller's NFT is. */
  nft:
    | { at: "kiosk"; kiosk: UserKiosk; nftId: string; nftType: string; listedNative?: boolean }
    | { at: "wallet"; objectId: string; nftType: string; kiosk?: UserKiosk | null };
};

type KioskArgs = { kiosk: TransactionObjectArgument; cap: TransactionArgument; finish: () => void };

/** Borrows (or creates) the buyer's kiosk and cap for the PTB. */
function openKiosk(tx: Transaction, k: UserKiosk | null, sender: string): KioskArgs {
  if (!k) {
    const [kiosk, cap] = tx.moveCall({ target: "0x2::kiosk::new" });
    return {
      kiosk,
      cap,
      finish: () => {
        tx.moveCall({ target: "0x2::transfer::public_share_object", typeArguments: [KIOSK], arguments: [kiosk] });
        tx.transferObjects([cap], sender);
      },
    };
  }
  const kiosk = tx.object(k.kioskId);
  if (k.personalCapId) {
    const pcap = tx.object(k.personalCapId);
    const [cap, borrow] = tx.moveCall({ target: `${KIOSK_EXT.latest}::personal_kiosk::borrow_val`, arguments: [pcap] });
    return { kiosk, cap, finish: () => tx.moveCall({ target: `${KIOSK_EXT.latest}::personal_kiosk::return_val`, arguments: [pcap, cap, borrow] }) };
  }
  return { kiosk, cap: tx.object(k.capId), finish: () => undefined };
}

function needPolicy(policy: Policy | null, nftType: string | null): Policy {
  if (!policy) throw new UnsupportedError(`no usable transfer policy for ${nftType}`);
  if (policy.unknownRules.length) throw new UnsupportedError(`policy ${policy.policyId} has rules we cannot prove: ${policy.unknownRules.join(", ")}`);
  return policy;
}

/** Satisfies a TransferRequest: places or locks the item into `into`, pays royalty from `payFrom`,
 * proves the remaining rules, confirms. */
function resolve(
  tx: Transaction,
  o: {
    policy: Policy;
    nftType: string;
    req: TransactionArgument;
    item: TransactionArgument | null;
    into: { kiosk: TransactionObjectArgument; cap: TransactionArgument } | null;
    payFrom: TransactionObjectArgument;
    /** Rules the venue already proved inside its call. */
    proved?: string[];
    personalKiosk: boolean;
    resaleGame?: string;
  },
) {
  const p = o.policy;
  const policy = tx.object(p.policyId);
  const has = (r: string) => p.rules.includes(r) && !(o.proved ?? []).includes(r);
  if (o.item) {
    if (!o.into) throw new Error("item without a destination kiosk");
    if (p.rules.includes(RULES.kioskLock)) {
      tx.moveCall({ target: "0x2::kiosk::lock", typeArguments: [o.nftType], arguments: [o.into.kiosk, o.into.cap, policy, o.item] });
    } else {
      tx.moveCall({ target: "0x2::kiosk::place", typeArguments: [o.nftType], arguments: [o.into.kiosk, o.into.cap, o.item] });
    }
  }
  if (has(RULES.royalty)) {
    const [paid] = tx.moveCall({ target: "0x2::transfer_policy::paid", typeArguments: [o.nftType], arguments: [o.req] });
    const [amount] = tx.moveCall({ target: `${KIOSK_EXT.latest}::royalty_rule::fee_amount`, typeArguments: [o.nftType], arguments: [policy, paid] });
    const [coin] = tx.splitCoins(o.payFrom, [amount]);
    tx.moveCall({ target: `${KIOSK_EXT.latest}::royalty_rule::pay`, typeArguments: [o.nftType], arguments: [policy, o.req, coin] });
  }
  if (has(RULES.kioskLock)) {
    if (!o.into) throw new UnsupportedError("kiosk lock rule needs the destination kiosk");
    tx.moveCall({ target: `${KIOSK_EXT.latest}::kiosk_lock_rule::prove`, typeArguments: [o.nftType], arguments: [o.req, o.into.kiosk] });
  }
  const resale = resaleRule();
  if (resale && has(resale)) {
    if (!o.into) throw new UnsupportedError("resale rule needs the destination kiosk");
    if (!o.resaleGame) throw new UnsupportedError("licence resale: the game is unknown, so the resale rule cannot be proved");
    addResaleProve(tx, { game: o.resaleGame, req: o.req, kiosk: o.into.kiosk, cap: o.into.cap, payFrom: o.payFrom });
  }
  if (has(RULES.personalKiosk)) {
    if (!o.into || !o.personalKiosk) throw new UnsupportedError("personal kiosk rule: the item must go to a personal kiosk");
    tx.moveCall({ target: `${KIOSK_EXT.latest}::personal_kiosk_rule::prove`, typeArguments: [o.nftType], arguments: [o.into.kiosk, o.req] });
  }
  if (has(RULES.floorPrice)) {
    tx.moveCall({ target: `${KIOSK_EXT.latest}::floor_price_rule::prove`, typeArguments: [o.nftType], arguments: [policy, o.req] });
  }
  tx.moveCall({ target: "0x2::transfer_policy::confirm_request", typeArguments: [o.nftType], arguments: [policy, o.req] });
}

const big = (s: string) => BigInt(s);

/** The marketplace fee the buyer pays on top of the price on the path we use: Drip v2 bought
 * directly through the kiosk pays none. */
export function venueFee(l: ActiveListing, opts: { dripDirect?: boolean } = {}): bigint {
  if (l.source === "drip/v2" && opts.dripDirect !== false) return 0n;
  return big(l.fee);
}

/** Royalty the buyer pays: the policy's rate on what the seller's kiosk receives (price minus
 * legacy commission; Tradeport E charges it on the price). */
export function royaltyOf(l: ActiveListing, policy: Policy | null): bigint {
  // Escrow listings hand over the NFT without a transfer request: no royalty.
  if (!policy?.royalty || l.source.endsWith("escrow") || (l.source === "tradeport/simple" && Number(l.extra.listingType) === 0)) return 0n;
  const base = l.source === "tradeport/simple" ? big(l.price) : big(l.price) - big(l.commission);
  const r = (base * BigInt(policy.royalty.bps)) / 10000n;
  return r > policy.royalty.min ? r : policy.royalty.min;
}

/** All-in price (price + venue fee + royalty, before gas). */
export function allInPrice(l: ActiveListing, policy: Policy | null, opts: { dripDirect?: boolean } = {}): bigint {
  return big(l.price) + venueFee(l, opts) + royaltyOf(l, policy);
}

// ---------- ours.ts: rare's own contract (venue `market`), off until published ----------

export type OursConfig = { package: string; config: string };
let ours: OursConfig = { package: "", config: "" };
export function configureOurs(c: Partial<OursConfig>) {
  ours = { ...ours, ...c };
}

function buildOursBuy(tx: Transaction, l: ActiveListing, ctx: BuyContext) {
  if (!ours.package || !ours.config) throw new UnsupportedError("our contract is not deployed yet");
  const T = l.nftType!;
  const cfg = tx.object(ours.config);
  const k = openKiosk(tx, ctx.kiosk, ctx.sender);
  if (l.kioskId) {
    const policy = needPolicy(ctx.policy, T);
    const [pay] = tx.splitCoins(tx.gas, [BigInt(l.price)]);
    if (policy.royalty) {
      // An upper bound: the change comes back.
      const [due] = tx.moveCall({ target: `${ours.package}::clearing::royalty_due`, typeArguments: [T], arguments: [tx.object(policy.policyId), tx.pure.u64(BigInt(l.price))] });
      const [r] = tx.splitCoins(tx.gas, [due]);
      tx.mergeCoins(pay, [r]);
    }
    const [req] = tx.moveCall({
      target: `${ours.package}::sale_kiosk::buy`,
      typeArguments: [T],
      arguments: [tx.object(l.listingId), cfg, tx.object(l.kioskId), k.kiosk, k.cap, tx.object(policy.policyId), pay],
    });
    // Rare Store licences: the hold check and royalty go in before the request is confirmed.
    const resale = resaleRule();
    if (resale && policy.rules.includes(resale)) addResaleProve(tx, { game: ctx.resaleGame, req, kiosk: k.kiosk, cap: k.cap, payFrom: tx.gas });
    tx.moveCall({ target: "0x2::transfer_policy::confirm_request", typeArguments: [T], arguments: [tx.object(policy.policyId), req] });
    tx.mergeCoins(tx.gas, [pay]);
  } else {
    const [pay] = tx.splitCoins(tx.gas, [BigInt(l.price)]);
    const [item] = tx.moveCall({ target: `${ours.package}::sale_escrow::take`, typeArguments: [T], arguments: [tx.object(l.listingId), cfg, pay] });
    tx.moveCall({ target: "0x2::kiosk::place", typeArguments: [T], arguments: [k.kiosk, k.cap, item] });
    tx.mergeCoins(tx.gas, [pay]);
  }
  k.finish();
}

/** Offers (`offer::fill`) and pool members (`offer_pool::fill`) on rare's own contract: the item is
 * bought out of the seller's kiosk into the maker's kiosk with royalty paid from the offer's
 * reserve; the seller receives the proceeds. */
function buildOursAcceptBid(tx: Transaction, b: ActiveBid, ctx: SellContext) {
  if (!ours.package || !ours.config) throw new UnsupportedError("our contract is not deployed yet");
  const T = ctx.nft.nftType;
  const cfg = tx.object(ours.config);
  const proof = tx.pure.vector("vector<u8>", (b.extra.proof as number[][] | undefined) ?? []);
  if (ctx.nft.at === "wallet") {
    const [proceeds] = b.poolId
      ? tx.moveCall({ target: `${ours.package}::offer_pool::fill_owned`, typeArguments: [T], arguments: [tx.object(b.poolId), tx.pure.id(b.bidId), cfg, tx.object(ctx.nft.objectId), proof, tx.object(CLOCK)] })
      : tx.moveCall({ target: `${ours.package}::offer::fill_owned`, typeArguments: [T], arguments: [tx.object(b.bidId), cfg, tx.object(ctx.nft.objectId), proof, tx.object(CLOCK)] });
    tx.transferObjects([proceeds], ctx.sender);
    return;
  }
  const policy = needPolicy(ctx.policy, T);
  const dest = b.extra.destKiosk as string | undefined;
  if (!dest) throw new Error("offer has no destination kiosk");
  const k = openKiosk(tx, ctx.nft.kiosk, ctx.sender);
  if (ctx.nft.listedNative) tx.moveCall({ target: "0x2::kiosk::delist", typeArguments: [T], arguments: [k.kiosk, k.cap, tx.pure.id(ctx.nft.nftId)] });
  const args = [cfg, k.kiosk, k.cap, tx.pure.id(ctx.nft.nftId), tx.object(dest), tx.object(policy.policyId), proof, tx.object(CLOCK)];
  const [proceeds, req] = b.poolId
    ? tx.moveCall({ target: `${ours.package}::offer_pool::fill`, typeArguments: [T], arguments: [tx.object(b.poolId), tx.pure.id(b.bidId), ...args] })
    : tx.moveCall({ target: `${ours.package}::offer::fill`, typeArguments: [T], arguments: [tx.object(b.bidId), ...args] });
  tx.moveCall({ target: "0x2::transfer_policy::confirm_request", typeArguments: [T], arguments: [tx.object(policy.policyId), req] });
  tx.transferObjects([proceeds], ctx.sender);
  k.finish();
}

// ---------- buildBuy ----------

/** Buy one listing. Adds commands to `tx`. */
export function buildBuy(tx: Transaction, l: ActiveListing, ctx: BuyContext): void {
  const T = l.nftType;
  if (!T) throw new UnsupportedError("listing has no NFT type yet");
  const tp = TRADEPORT;
  const sender = ctx.sender;
  switch (l.source) {
    case "tradeport/simple": {
      const ob = tx.object(tp.orderbook.store);
      const store = tx.object(tp.simple.store);
      if (!l.extra.inOrderbook) {
        // Listings made by pre-orderbook code must join the orderbook before buy_ob_* can remove them.
        tx.moveCall({ target: `${tp.simple.latest}::tradeport_listings::report_ob_listing`, typeArguments: [T], arguments: [store, ob, tx.pure.id(l.nftId)] });
      }
      const [pay] = tx.splitCoins(tx.gas, [big(l.price) + big(l.fee)]);
      if (Number(l.extra.listingType) === 1) {
        const policy = needPolicy(ctx.policy, T);
        if (policy.royalty) {
          const [roy] = tx.moveCall({ target: `${KIOSK_EXT.latest}::royalty_rule::fee_amount`, typeArguments: [T], arguments: [tx.object(policy.policyId), tx.pure.u64(l.price)] });
          const [r] = tx.splitCoins(tx.gas, [roy]);
          tx.mergeCoins(pay, [r]);
        }
        const k = openKiosk(tx, ctx.kiosk, sender);
        const [req] = tx.moveCall({
          target: `${tp.simple.latest}::tradeport_listings::buy_ob_listing_with_transfer_policy`,
          typeArguments: [T],
          arguments: [store, ob, tx.object(l.kioskId!), k.kiosk, k.cap, tx.pure.address(sender), tx.pure.id(l.nftId), pay, tx.object(policy.policyId)],
        });
        // The call takes its fee, pays royalty, locks or places the item and proves lock, floor and personal rules.
        resolve(tx, { policy: { ...policy, rules: policy.rules.filter((r) => r !== RULES.royalty) }, nftType: T, req, item: null, into: k, payFrom: tx.gas, proved: [RULES.kioskLock, RULES.floorPrice, RULES.personalKiosk], personalKiosk: !!ctx.kiosk?.personal, resaleGame: ctx.resaleGame });
        k.finish();
      } else {
        tx.moveCall({ target: `${tp.simple.latest}::tradeport_listings::buy_ob_listing_without_transfer_policy`, typeArguments: [T], arguments: [store, ob, tx.pure.id(l.nftId), pay] });
      }
      tx.mergeCoins(tx.gas, [pay]);
      return;
    }
    case "tradeport/kiosk": {
      const policy = needPolicy(ctx.policy, T);
      const k = openKiosk(tx, ctx.kiosk, sender);
      const [pay] = tx.splitCoins(tx.gas, [big(l.price) + big(l.fee)]);
      const [item, req] = tx.moveCall({
        target: `${tp.kiosk.latest}::kiosk_listings::buy`,
        typeArguments: [T],
        arguments: [tx.object(tp.kiosk.listingsStore), tx.object(l.kioskId!), k.kiosk, tx.pure.id(l.nftId), pay],
      });
      resolve(tx, { policy, nftType: T, req, item, into: k, payFrom: tx.gas, personalKiosk: !!ctx.kiosk?.personal, resaleGame: ctx.resaleGame });
      k.finish();
      tx.mergeCoins(tx.gas, [pay]);
      return;
    }
    case "tradeport/kiosk-early": {
      const policy = needPolicy(ctx.policy, T);
      const k = openKiosk(tx, ctx.kiosk, sender);
      // The early module wants exactly price + commission (commission on top).
      const [pay] = tx.splitCoins(tx.gas, [big(l.price) + big(l.fee) + big(l.commission)]);
      const [item, req] = tx.moveCall({
        target: `${tp.kioskEarly.latest}::kiosk_listings::buy`,
        typeArguments: [T],
        arguments: [tx.object(tp.kioskEarly.listingsStore), tx.object(l.kioskId!), k.kiosk, k.cap, tx.pure.id(l.nftId), pay],
      });
      resolve(tx, { policy, nftType: T, req, item, into: k, payFrom: tx.gas, personalKiosk: !!ctx.kiosk?.personal, resaleGame: ctx.resaleGame });
      k.finish();
      tx.mergeCoins(tx.gas, [pay]);
      return;
    }
    case "tradeport/escrow": {
      const [pay] = tx.splitCoins(tx.gas, [big(l.price)]);
      tx.moveCall({ target: `${tp.escrow.latest}::listings::buy`, typeArguments: [T], arguments: [tx.object(tp.escrow.listingsStore), tx.pure.id(l.listingId), pay] });
      tx.mergeCoins(tx.gas, [pay]);
      return;
    }
    case "drip/v2": {
      const policy = needPolicy(ctx.policy, T);
      const k = openKiosk(tx, ctx.kiosk, sender);
      if (ctx.dripDirect !== false) {
        // Drip v2 listings are plain public kiosk listings: buy at the listed price with
        // kiosk::purchase (no marketplace fee), settle the policy into the buyer's kiosk...
        const [pay] = tx.splitCoins(tx.gas, [big(l.price)]);
        const [item, req] = tx.moveCall({ target: "0x2::kiosk::purchase", typeArguments: [T], arguments: [tx.object(l.kioskId!), tx.pure.id(l.nftId), pay] });
        resolve(tx, { policy, nftType: T, req, item, into: k, payFrom: tx.gas, personalKiosk: !!ctx.kiosk?.personal, resaleGame: ctx.resaleGame });
        // ...then clear Drip's now stale record so the buyer can list it on Drip later: its delist
        // needs the item listed in the kiosk passed, so list it for a moment and delist through Drip.
        tx.moveCall({ target: "0x2::kiosk::list", typeArguments: [T], arguments: [k.kiosk, k.cap, tx.pure.id(l.nftId), tx.pure.u64(1)] });
        tx.moveCall({ target: `${DRIP_V2.market}::marketplace::delist`, typeArguments: [T], arguments: [tx.object(DRIP_V2.registry), k.kiosk, k.cap, tx.pure.id(l.nftId)] });
        k.finish();
        return;
      }
      const [pay] = tx.splitCoins(tx.gas, [big(l.price) + big(l.fee)]);
      const [item, req] = tx.moveCall({
        target: `${DRIP_V2.market}::marketplace::buy`,
        typeArguments: [T],
        arguments: [tx.object(DRIP_V2.registry), tx.object(l.kioskId!), tx.pure.id(l.nftId), pay],
      });
      resolve(tx, { policy, nftType: T, req, item, into: k, payFrom: tx.gas, personalKiosk: !!ctx.kiosk?.personal, resaleGame: ctx.resaleGame });
      k.finish();
      return;
    }
    case "kiosk/native": {
      const policy = needPolicy(ctx.policy, T);
      const k = openKiosk(tx, ctx.kiosk, sender);
      const [pay] = tx.splitCoins(tx.gas, [big(l.price)]);
      const [item, req] = tx.moveCall({ target: "0x2::kiosk::purchase", typeArguments: [T], arguments: [tx.object(l.kioskId!), tx.pure.id(l.nftId), pay] });
      resolve(tx, { policy, nftType: T, req, item, into: k, payFrom: tx.gas, personalKiosk: !!ctx.kiosk?.personal, resaleGame: ctx.resaleGame });
      k.finish();
      return;
    }
    case "market/v1":
      return buildOursBuy(tx, l, ctx);
    default:
      throw new UnsupportedError(`buying ${l.source} is not supported`);
  }
}

// ---------- buildAcceptBid ----------

/** Sell into a collection bid or accept an item offer. The NFT always reaches the bidder. */
export function buildAcceptBid(tx: Transaction, b: ActiveBid, ctx: SellContext): void {
  if (!b.fillable) throw new UnsupportedError("bid cannot be filled (pool short or expired)");
  const T = ctx.nft.nftType;
  if (b.nftType && b.nftType !== T) throw new UnsupportedError(`bid is for ${b.nftType}, NFT is ${T}`);
  if (b.kind === "item" && b.nftId && ctx.nft.at === "kiosk" && ctx.nft.nftId !== b.nftId) throw new UnsupportedError("offer is for another item");
  const tp = TRADEPORT;
  const sender = ctx.sender;
  const nft = ctx.nft;
  const prepKiosk = (): { k: KioskArgs; nftId: string } => {
    if (nft.at === "kiosk") {
      const k = openKiosk(tx, nft.kiosk, sender);
      if (nft.listedNative) tx.moveCall({ target: "0x2::kiosk::delist", typeArguments: [T], arguments: [k.kiosk, k.cap, tx.pure.id(nft.nftId)] });
      return { k, nftId: nft.nftId };
    }
    const k = openKiosk(tx, nft.kiosk ?? null, sender);
    tx.moveCall({ target: "0x2::kiosk::place", typeArguments: [T], arguments: [k.kiosk, k.cap, tx.object(nft.objectId)] });
    return { k, nftId: nft.objectId };
  };
  const pooled = (pkg: string, module: string, store: string) => {
    const pool = tx.pure.option("address", b.poolId);
    if (nft.at === "wallet" && !ctx.policy) {
      tx.moveCall({ target: `${pkg}::${module}::accept_bid_without_transfer_policy`, typeArguments: [T], arguments: [tx.object(CLOCK), tx.object(store), tx.pure.id(b.bidId), pool, tx.object(nft.objectId)] });
      return;
    }
    const policy = needPolicy(ctx.policy, T);
    const { k, nftId } = prepKiosk();
    // The contract pays seller, royalty and fee from the bid, locks the item in a new kiosk for the
    // bidder and proves lock, personal and floor rules.
    const [req] = tx.moveCall({
      target: `${pkg}::${module}::accept_bid_with_transfer_policy`,
      typeArguments: [T],
      arguments: [tx.object(CLOCK), tx.object(store), tx.pure.id(b.bidId), pool, k.kiosk, k.cap, tx.pure.id(nftId), tx.object(policy.policyId)],
    });
    tx.moveCall({ target: "0x2::transfer_policy::confirm_request", typeArguments: [T], arguments: [tx.object(policy.policyId), req] });
    k.finish();
  };
  const walletOnly = (pkg: string, store: string) => {
    if (nft.at !== "wallet") throw new UnsupportedError("this bid takes wallet-held NFTs only");
    tx.moveCall({ target: `${pkg}::biddings::accept_bid`, typeArguments: [T], arguments: [tx.object(store), tx.pure.id(b.bidId), tx.object(nft.objectId)] });
  };
  switch (b.source) {
    case "tradeport/bids":
      return pooled(tp.bids.latest, "tradeport_biddings", tp.bids.store);
    case "tradeport/escrow-bids":
      return walletOnly(tp.escrow.latest, tp.escrow.biddingsStore);
    case "tradeport/kiosk-bids": {
      const policy = needPolicy(ctx.policy, T);
      if (policy.rules.includes(RULES.personalKiosk)) throw new UnsupportedError("personal kiosk rule: Tradeport's escrow kiosk cannot satisfy it");
      const { k, nftId } = prepKiosk();
      const escrow = tx.object(tp.kiosk.escrowKiosk);
      // The item moves to Tradeport's escrow kiosk, locked for the bidder to claim; the price reaches the seller.
      const [leftover, req] = tx.moveCall({
        target: `${tp.kiosk.latest}::kiosk_biddings::accept_bid`,
        typeArguments: [T],
        arguments: [tx.object(tp.kiosk.biddingsStore), tx.pure.id(b.bidId), escrow, k.kiosk, k.cap, tx.pure.id(nftId), tx.object(policy.policyId)],
      });
      resolve(tx, { policy, nftType: T, req, item: null, into: { kiosk: escrow, cap: k.cap }, payFrom: tx.gas, personalKiosk: false });
      tx.transferObjects([leftover], sender);
      k.finish();
      return;
    }
    case "drip/v2-bids": {
      const policy = needPolicy(ctx.policy, T);
      if (policy.rules.includes(RULES.personalKiosk)) throw new UnsupportedError("personal kiosk rule: cannot deliver to the bidder's personal kiosk");
      const { k, nftId } = prepKiosk();
      const [item, req] =
        b.kind === "item"
          ? tx.moveCall({ target: `${DRIP_V2.bids}::bids::accept_offer`, typeArguments: [T], arguments: [tx.object(b.bidId), k.kiosk, k.cap] })
          : tx.moveCall({ target: `${DRIP_V2.bids}::bids::accept_bid`, typeArguments: [T], arguments: [tx.object(b.bidId), k.kiosk, k.cap, tx.pure.id(nftId)] });
      // Sale proceeds land in the seller kiosk's profits: take them now, pay royalty from them.
      const [proceeds] = tx.moveCall({ target: "0x2::kiosk::withdraw", arguments: [k.kiosk, k.cap, tx.pure.option("u64", null)] });
      // Deliver to the bidder: a new kiosk, shared, its cap sent to the bidder.
      const [bk, bcap] = tx.moveCall({ target: "0x2::kiosk::new" });
      resolve(tx, { policy, nftType: T, req, item, into: { kiosk: bk, cap: bcap }, payFrom: proceeds, personalKiosk: false });
      tx.moveCall({ target: "0x2::transfer::public_share_object", typeArguments: [KIOSK], arguments: [bk] });
      tx.transferObjects([bcap], tx.pure.address(b.bidder));
      tx.transferObjects([proceeds], sender);
      k.finish();
      return;
    }
    case "market/v1":
      return buildOursAcceptBid(tx, b, ctx);
    default:
      throw new UnsupportedError(`selling into ${b.source} is not supported`);
  }
}
