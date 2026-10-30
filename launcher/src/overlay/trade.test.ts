// Offline checks of overlay selling: the PTB commands buildListTransaction and
// buildAcceptOfferTransaction produce for each case, on fixtures shaped like live rare.market /v1
// answers (mocked `get`, no network). The mainnet simulation is in trade.live.test.ts (LIVE=1).

import type { Transaction } from "@mysten/sui/transactions";
import { describe, expect, it } from "vitest";
import type { ItemCard, Offer } from "../marketplace/api";
import { KIOSK_EXT, TRADEPORT } from "../marketplace/venues/ids";
import { sellerNetMist, type OwnedKioskRef, type RareGet, type TransferPolicy } from "../marketplace/venues";
import { bestOffer, buildAcceptOfferTransaction, buildListTransaction, offerNetMist } from "./trade";

const SELLER = "0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac";
const BIDDER = "0x3c2f7de4c6e2892a80d44b77fae4e14dcd95cf3b8f016c6b1f1da456df28587d";
const RETRO = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
const ITEM = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";
const EXT = KIOSK_EXT.origin;

const royalty = { type: "royalty_rule", ruleType: `${EXT}::royalty_rule::Rule`, config: { amount_bp: 1000, min_amount: "0" } };
const lock = { type: "kiosk_lock_rule", ruleType: `${EXT}::kiosk_lock_rule::Rule` };
const personalRule = { type: "personal_kiosk_rule", ruleType: `${KIOSK_EXT.personalOrigin}::personal_kiosk_rule::Rule` };
const POL = {
  royaltyOnly: { id: "0x" + "ab".repeat(32), isShared: true, rules: [royalty] } as TransferPolicy,
  lockRoyalty: { id: "0x7309bb80aee534f2b4334c9b97df0227412ed51e0a8b103fc3c8c751a925cf94", isShared: true, rules: [lock, royalty] } as TransferPolicy,
  personal: { id: "0x" + "cd".repeat(32), isShared: true, rules: [personalRule, lock, royalty] } as TransferPolicy,
};

const plainKiosk: OwnedKioskRef = { kioskId: "0x10785ddeb5258be06802aed6d57ec411415d0a86902bf334cea00fd8573ee6f3", ownerCapId: "0x" + "22".repeat(32), personalCapId: null, isPersonal: false };
const personalKiosk: OwnedKioskRef = { kioskId: "0x" + "33".repeat(32), ownerCapId: "0x" + "44".repeat(32), personalCapId: "0x" + "55".repeat(32), isPersonal: true };

/** A rare.market API stand-in: the seller's kiosks, one policy for the type, offers and the item. */
function api(o: { kiosks?: OwnedKioskRef[]; policy?: TransferPolicy | null; offers?: Offer[]; item?: Partial<ItemCard> } = {}): RareGet & { paths: string[] } {
  const paths: string[] = [];
  const get = (async (p: string) => {
    paths.push(p);
    if (p.endsWith("/kiosks")) return { data: o.kiosks ?? [] };
    if (p.startsWith("/v1/policies/")) return { data: o.policy === null ? [] : [o.policy ?? POL.royaltyOnly] };
    if (p.startsWith("/v1/offers")) return { data: o.offers ?? [] };
    if (p.startsWith("/v1/items/")) return { ...card(), ...(o.item ?? {}) };
    throw new Error(`unexpected ${p}`);
  }) as RareGet & { paths: string[] };
  get.paths = paths;
  return get;
}

const card = (over: Partial<ItemCard> = {}): ItemCard => ({
  id: ITEM,
  name: "RetroSui #1550",
  type: RETRO,
  imageUrl: null,
  collection: { slug: "retrosui", name: "RetroSui", imageUrl: null, verified: false },
  listing: null,
  owner: SELLER,
  kioskId: null,
  ...over,
});

const nativeListing = { venue: "kiosk", source: "kiosk/native", orderKey: `${plainKiosk.kioskId}:${ITEM}`, priceMist: "12000000", kioskId: plainKiosk.kioskId, nftType: RETRO };

const offer = (over: Partial<Offer> = {}): Offer => ({
  id: "0x" + "0f".repeat(32),
  venue: "tradeport",
  source: "tradeport/bids",
  nftType: RETRO,
  feeMist: "3330000",
  royaltyMist: "11100000",
  commissionMist: "0",
  poolId: "0x14af0f71ded139ff5b41cacd37b8b924e8298c9de37c6b2f8fde0aea3a66605e",
  fillable: true,
  extra: { nftBcs: null, balance: "0" },
  kind: "collection",
  collection: { slug: "retrosui", name: "RetroSui", imageUrl: null, verified: false },
  item: null,
  trait: null,
  priceMist: "111000000",
  quantity: 1,
  filled: 0,
  bidder: BIDDER,
  destKioskId: null,
  createdAt: "2026-10-25T22:55:39.131Z",
  expiresAt: null,
  status: "open",
  ...over,
});

/** Move calls as "module::function", with the package aliased where it matters. */
function calls(tx: Transaction): string[] {
  return tx.getData().commands.flatMap((c) => (c.MoveCall ? [`${c.MoveCall.module}::${c.MoveCall.function}`] : []));
}
const kinds = (tx: Transaction) => tx.getData().commands.map((c) => c.$kind);
const pkgOf = (tx: Transaction, fn: string) => tx.getData().commands.find((c) => c.MoveCall && `${c.MoveCall.module}::${c.MoveCall.function}` === fn)?.MoveCall?.package;
/** The u64 pure inputs in input order (prices). */
function u64s(tx: Transaction): bigint[] {
  return tx
    .getData()
    .inputs.filter((i) => i.Pure)
    .map((i) => Uint8Array.from(atob(i.Pure!.bytes), (ch) => ch.charCodeAt(0)))
    .filter((b) => b.length === 8)
    .map((b) => new DataView(b.buffer).getBigUint64(0, true));
}
const norm = (id: string) => "0x" + id.replace(/^0x/, "").padStart(64, "0");

describe("buildListTransaction (native kiosk listing)", () => {
  it("wallet item, seller has no kiosk: new kiosk, place, list, share, cap to the seller", async () => {
    const tx = await buildListTransaction(card(), 5_000_000_000n, SELLER, api({ kiosks: [] }));
    expect(calls(tx)).toEqual(["kiosk::new", "kiosk::place", "kiosk::list", "transfer::public_share_object"]);
    expect(kinds(tx).at(-1)).toBe("TransferObjects");
    expect(u64s(tx)).toContain(5_000_000_000n);
    expect(tx.getData().sender).toBe(SELLER);
  });

  it("item already in the seller's kiosk: list only", async () => {
    const tx = await buildListTransaction(card({ kioskId: plainKiosk.kioskId }), 1_000_000_000n, SELLER, api({ kiosks: [plainKiosk] }));
    expect(calls(tx)).toEqual(["kiosk::list"]);
    expect(kinds(tx)).toEqual(["MoveCall"]);
  });

  it("lock-rule policy: a wallet item is locked (with the policy) into the existing kiosk", async () => {
    const tx = await buildListTransaction(card(), 1_000_000_000n, SELLER, api({ kiosks: [plainKiosk], policy: POL.lockRoyalty }));
    expect(calls(tx)).toEqual(["kiosk::lock", "kiosk::list"]);
    const d = tx.getData();
    const objs = d.inputs.flatMap((i) => (i.UnresolvedObject ? [norm(i.UnresolvedObject.objectId)] : []));
    expect(objs).toContain(norm(POL.lockRoyalty.id));
    expect(objs).toContain(norm(plainKiosk.kioskId));
  });

  it("personal-kiosk policy, seller has only a plain kiosk: a personal kiosk is made in the PTB", async () => {
    const tx = await buildListTransaction(card(), 1_000_000_000n, SELLER, api({ kiosks: [plainKiosk], policy: POL.personal }));
    expect(calls(tx)).toEqual([
      "kiosk::new",
      "personal_kiosk::new",
      "personal_kiosk::borrow_val",
      "kiosk::lock",
      "kiosk::list",
      "personal_kiosk::return_val",
      "personal_kiosk::transfer_to_sender",
      "transfer::public_share_object",
    ]);
    expect(pkgOf(tx, "personal_kiosk::new")).toBe(KIOSK_EXT.latest);
  });

  it("personal-kiosk policy, seller has a personal kiosk: borrow the cap, lock, list, return it", async () => {
    const tx = await buildListTransaction(card(), 1_000_000_000n, SELLER, api({ kiosks: [plainKiosk, personalKiosk], policy: POL.personal }));
    expect(calls(tx)).toEqual(["personal_kiosk::borrow_val", "kiosk::lock", "kiosk::list", "personal_kiosk::return_val"]);
  });

  it("relisting a native listing at a new price: delist then list in the same kiosk", async () => {
    const tx = await buildListTransaction(card({ kioskId: plainKiosk.kioskId, listing: nativeListing }), 20_000_000n, SELLER, api({ kiosks: [plainKiosk] }));
    expect(calls(tx)).toEqual(["kiosk::delist", "kiosk::list"]);
    expect(u64s(tx)).toEqual([20_000_000n]);
  });

  it("items listed on Tradeport or Drip must be delisted there first", async () => {
    const get = api({ kiosks: [plainKiosk] });
    await expect(buildListTransaction(card({ kioskId: plainKiosk.kioskId, listing: { venue: "tradeport", priceMist: "1" } }), 1n, SELLER, get)).rejects.toThrow("Delist it on Tradeport first.");
    await expect(buildListTransaction(card({ kioskId: plainKiosk.kioskId, listing: { venue: "drip", priceMist: "1" } }), 1n, SELLER, get)).rejects.toThrow("Delist it on Drip first.");
    expect(get.paths).toEqual([]);
  });

  it("a zero price is refused", async () => {
    await expect(buildListTransaction(card(), 0n, SELLER, api())).rejects.toThrow("Price must be above zero.");
  });
});

describe("buildAcceptOfferTransaction", () => {
  it("tradeport/bids collection bid, item listed natively in the seller's kiosk: delist, accept, confirm", async () => {
    const tx = await buildAcceptOfferTransaction(card({ kioskId: plainKiosk.kioskId, listing: nativeListing }), offer(), SELLER, api({ kiosks: [plainKiosk], policy: POL.lockRoyalty }));
    expect(calls(tx)).toEqual(["kiosk::delist", "tradeport_biddings::accept_bid_with_transfer_policy", "transfer_policy::confirm_request"]);
    expect(pkgOf(tx, "tradeport_biddings::accept_bid_with_transfer_policy")).toBe(TRADEPORT.bids.latest);
  });

  it("tradeport/bids from a wallet: placed into a new kiosk first, the kiosk shared", async () => {
    const tx = await buildAcceptOfferTransaction(card(), offer(), SELLER, api({ kiosks: [plainKiosk], policy: POL.lockRoyalty }));
    expect(calls(tx)).toEqual(["kiosk::new", "kiosk::place", "tradeport_biddings::accept_bid_with_transfer_policy", "transfer_policy::confirm_request", "transfer::public_share_object"]);
  });

  it("tradeport/bids from a wallet when the type has no policy: the plain accept", async () => {
    const tx = await buildAcceptOfferTransaction(card(), offer(), SELLER, api({ policy: null }));
    expect(calls(tx)).toEqual(["tradeport_biddings::accept_bid_without_transfer_policy"]);
  });

  it("tradeport/kiosk-bids item offer from a kiosk: escrow kiosk, royalty paid, lock proved, leftover back to the seller", async () => {
    const o = offer({ source: "tradeport/kiosk-bids", kind: "item", item: { id: ITEM, name: "RetroSui #1550", imageUrl: "" }, poolId: null, feeMist: "0", royaltyMist: "0", commissionMist: "24000000", priceMist: "800000000", extra: { escrow: "904000000" } });
    const tx = await buildAcceptOfferTransaction(card({ kioskId: plainKiosk.kioskId }), o, SELLER, api({ kiosks: [plainKiosk], policy: POL.lockRoyalty }));
    expect(calls(tx)).toEqual([
      "kiosk_biddings::accept_bid",
      "transfer_policy::paid",
      "royalty_rule::fee_amount",
      "royalty_rule::pay",
      "kiosk_lock_rule::prove",
      "transfer_policy::confirm_request",
    ]);
    expect(kinds(tx).at(-1)).toBe("TransferObjects");
    const objs = tx.getData().inputs.flatMap((i) => (i.UnresolvedObject ? [norm(i.UnresolvedObject.objectId)] : []));
    expect(objs).toContain(norm(TRADEPORT.kiosk.escrowKiosk));
  });

  it("tradeport/escrow-bids take a wallet item directly", async () => {
    const tx = await buildAcceptOfferTransaction(card(), offer({ source: "tradeport/escrow-bids", poolId: null }), SELLER, api());
    expect(calls(tx)).toEqual(["biddings::accept_bid"]);
    expect(pkgOf(tx, "biddings::accept_bid")).toBe(TRADEPORT.escrow.latest);
  });

  it("drip/v2-bids item offer: accept, withdraw proceeds, settle into a new kiosk for the bidder", async () => {
    const o = offer({ venue: "drip", source: "drip/v2-bids", kind: "item", item: { id: ITEM, name: "", imageUrl: "" }, poolId: null });
    const tx = await buildAcceptOfferTransaction(card({ kioskId: plainKiosk.kioskId }), o, SELLER, api({ kiosks: [plainKiosk], policy: POL.lockRoyalty }));
    expect(calls(tx)).toEqual([
      "bids::accept_offer",
      "kiosk::withdraw",
      "kiosk::new",
      "kiosk::lock",
      "transfer_policy::paid",
      "royalty_rule::fee_amount",
      "royalty_rule::pay",
      "kiosk_lock_rule::prove",
      "transfer_policy::confirm_request",
      "transfer::public_share_object",
    ]);
  });

  it("unsupported venues and shapes get one plain sentence", async () => {
    const get = api({ kiosks: [plainKiosk], policy: POL.personal });
    const inKiosk = card({ kioskId: plainKiosk.kioskId });
    await expect(buildAcceptOfferTransaction(inKiosk, offer({ venue: "market", source: "market/v1" }), SELLER, get)).rejects.toThrow("Selling into offers on Rare is not available for this item yet.");
    await expect(buildAcceptOfferTransaction(inKiosk, offer({ source: "tradeport/multi-bids" }), SELLER, get)).rejects.toThrow("Selling into offers on Tradeport is not available for this item yet.");
    // Escrow bids take wallet items only; kiosk bids can't satisfy a personal kiosk rule.
    await expect(buildAcceptOfferTransaction(inKiosk, offer({ source: "tradeport/escrow-bids" }), SELLER, get)).rejects.toThrow("Selling into offers on Tradeport is not available for this item yet.");
    await expect(buildAcceptOfferTransaction(inKiosk, offer({ source: "tradeport/kiosk-bids" }), SELLER, get)).rejects.toThrow("Selling into offers on Tradeport is not available for this item yet.");
  });

  it("refuses own offers, other items, closed offers and items listed elsewhere", async () => {
    const get = api({ kiosks: [plainKiosk] });
    await expect(buildAcceptOfferTransaction(card(), offer({ bidder: SELLER }), SELLER, get)).rejects.toThrow("You can't accept your own offer.");
    await expect(buildAcceptOfferTransaction(card(), offer({ kind: "item", item: { id: "0x" + "99".repeat(32), name: "", imageUrl: "" } }), SELLER, get)).rejects.toThrow("This offer is for another item.");
    await expect(buildAcceptOfferTransaction(card(), offer({ status: "cancelled" }), SELLER, get)).rejects.toThrow("This offer can't be filled any more.");
    await expect(buildAcceptOfferTransaction(card(), offer({ nftType: "0x2::other::Nft" }), SELLER, get)).rejects.toThrow("This offer is for another collection.");
    await expect(buildAcceptOfferTransaction(card({ kioskId: plainKiosk.kioskId, listing: { venue: "tradeport", priceMist: "1" } }), offer(), SELLER, get)).rejects.toThrow("Delist it on Tradeport first.");
  });
});

describe("offerNetMist and bestOffer", () => {
  // A live RetroSui kiosk bid placed at a 2% royalty reserve while the policy now takes 10%.
  const stale = offer({ id: "0x04f5", source: "tradeport/kiosk-bids", poolId: null, feeMist: "0", royaltyMist: "0", commissionMist: "30000", priceMist: "1000000", extra: { escrow: "1050000" } });

  it("the price on most sources; kiosk bids pay the royalty gap from the seller's side", () => {
    expect(offerNetMist(offer())).toBe(111000000n);
    expect(sellerNetMist(stale, POL.lockRoyalty)).toBe(920000n);
    expect(sellerNetMist(offer({ source: "tradeport/kiosk-bids", priceMist: "800000000", commissionMist: "24000000", extra: { escrow: "904000000" } }), POL.lockRoyalty)).toBe(800000000n);
    expect(offerNetMist({ ...stale, extra: { ...stale.extra, sellerNetMist: "920000" } })).toBe(920000n);
  });

  it("picks the best fillable offer by net, skipping own, wallet-only, closed and other-item offers", async () => {
    const offers = [
      offer({ id: "own", priceMist: "9000000", bidder: SELLER }),
      offer({ id: "escrow", priceMist: "8000000", source: "tradeport/escrow-bids" }),
      offer({ id: "other", priceMist: "7000000", kind: "item", item: { id: "0x" + "99".repeat(32), name: "", imageUrl: "" } }),
      offer({ id: "dead", priceMist: "6000000", fillable: false }),
      { ...stale, id: "stale" },
      offer({ id: "good", priceMist: "950000" }),
    ];
    const get = api({ offers, item: { kioskId: plainKiosk.kioskId }, policy: POL.lockRoyalty });
    const best = await bestOffer(ITEM, get);
    expect(best?.id).toBe("good");
    expect(get.paths[0]).toBe(`/v1/offers?item=${ITEM}&limit=10`);
    // Without the policy read the stale kiosk bid would have looked like 1,000,000.
    expect(get.paths.some((p) => p.startsWith("/v1/policies/"))).toBe(true);
  });

  it("drops kiosk bids for personal-kiosk types and returns null when nothing is fillable", async () => {
    expect(await bestOffer(ITEM, api({ offers: [{ ...stale }], item: { kioskId: plainKiosk.kioskId }, policy: POL.personal }))).toBeNull();
    expect(await bestOffer(ITEM, api({ offers: [] }))).toBeNull();
  });
});
