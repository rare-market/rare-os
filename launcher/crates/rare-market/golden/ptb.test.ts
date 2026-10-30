// Golden PTBs for the Rust port (launcher/crates/rare-market): the web launcher's builders run on
// fixed fixtures and their `tx.getData()` is kept in ptb.json. The Rust tests (src/golden.rs) build
// the same cases and must produce the same JSON, input for input and command for command. This test
// fails when the TypeScript builders change without the golden file (and so the Rust port) being
// updated: `UPDATE_GOLDEN=1 npx vitest run crates/rare-market/golden` rewrites it.

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { ItemCard, Offer } from "../../../src/marketplace/api";
import { buildCartBuy, configureResale, type ApiListing, type OwnedKioskRef, type TransferPolicy } from "../../../src/marketplace/venues";
import { KIOSK_EXT } from "../../../src/marketplace/venues/ids";
import { buildAcceptOfferTransaction, buildListTransaction } from "../../../src/overlay/trade";
import { buildCancelTransaction } from "../../../src/inventory/trade";
import type { InvItem } from "../../../src/inventory/types";

const FILE = fileURLToPath(new URL("./ptb.json", import.meta.url));

const BUYER = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const SELLER = "0x7682c8538863a72d81f09c1673d50ffda1220d5ec680f7f7f0c527eed0324fac";
const BIDDER = "0x3c2f7de4c6e2892a80d44b77fae4e14dcd95cf3b8f016c6b1f1da456df28587d";
const RETRO = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
const ITEM = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";
const SELLER_KIOSK = "0x10785ddeb5258be06802aed6d57ec411415d0a86902bf334cea00fd8573ee6f3";
const EXT = KIOSK_EXT.origin;
const rep = (b: string) => "0x" + b.repeat(32);

const royalty = { type: "royalty_rule", ruleType: `${EXT}::royalty_rule::Rule`, config: { amount_bp: 1000, min_amount: "0" } };
const lock = { type: "kiosk_lock_rule", ruleType: `${EXT}::kiosk_lock_rule::Rule` };
const personalRule = { type: "personal_kiosk_rule", ruleType: `${KIOSK_EXT.personalOrigin}::personal_kiosk_rule::Rule` };
const floorRule = { type: "floor_price_rule", ruleType: `${KIOSK_EXT.floorOrigin}::floor_price_rule::Rule`, config: { floor_price: "5" } };
const POL: Record<string, TransferPolicy> = {
  royaltyOnly: { id: rep("ab"), isShared: true, rules: [royalty] },
  lockRoyalty: { id: "0x7309bb80aee534f2b4334c9b97df0227412ed51e0a8b103fc3c8c751a925cf94", isShared: true, rules: [lock, royalty] },
  personal: { id: rep("cd"), isShared: true, rules: [personalRule, lock, royalty] },
  floor: { id: rep("ce"), isShared: true, rules: [royalty, floorRule] },
};
const plainKiosk: OwnedKioskRef = { kioskId: rep("11"), ownerCapId: rep("22"), personalCapId: null, isPersonal: false };
const personalKiosk: OwnedKioskRef = { kioskId: rep("33"), ownerCapId: rep("44"), personalCapId: rep("55"), isPersonal: true };
const sellerKiosk: OwnedKioskRef = { kioskId: SELLER_KIOSK, ownerCapId: rep("22"), personalCapId: null, isPersonal: false };

const listing = (over: Partial<ApiListing>): ApiListing => ({
  venue: "kiosk",
  source: "kiosk/native",
  orderKey: `${SELLER_KIOSK}:${ITEM}`,
  objectId: null,
  priceMist: "12000000",
  allInMist: "13200000",
  feeMist: "0",
  commissionMist: "0",
  seller: "",
  kioskId: SELLER_KIOSK,
  nftType: RETRO,
  extra: { royaltyMist: "1200000" },
  ...over,
});
const buy = (lines: Array<{ l: ApiListing; game?: string }>, policies: Record<string, TransferPolicy[]>, kiosks: OwnedKioskRef[]) =>
  buildCartBuy(lines.map(({ l, game }) => ({ itemId: ITEM, listing: l, resaleGame: game })), { sender: BUYER, buyerKiosks: kiosks, policies });

function api(o: { kiosks?: OwnedKioskRef[]; policy?: TransferPolicy | null } = {}) {
  return (async (p: string) => {
    if (p.endsWith("/kiosks")) return { data: o.kiosks ?? [] };
    if (p.startsWith("/v1/policies/")) return { data: o.policy === null ? [] : [o.policy ?? POL.royaltyOnly] };
    throw new Error(`unexpected ${p}`);
  }) as <T>(p: string) => Promise<T>;
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
const nativeListing = { venue: "kiosk", source: "kiosk/native", orderKey: `${SELLER_KIOSK}:${ITEM}`, priceMist: "12000000", kioskId: SELLER_KIOSK, nftType: RETRO };
const offer = (over: Partial<Offer> = {}): Offer => ({
  id: rep("0f"),
  venue: "tradeport",
  source: "tradeport/bids",
  nftType: RETRO,
  feeMist: "3330000",
  royaltyMist: "11100000",
  commissionMist: "0",
  poolId: "0x14af0f71ded139ff5b41cacd37b8b924e8298c9de37c6b2f8fde0aea3a66605e",
  fillable: true,
  extra: {},
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
const inKiosk = { kioskId: SELLER_KIOSK };
const itemOffer = (source: string, extra: Record<string, unknown> = {}) => offer({ source, kind: "item", item: { id: ITEM, name: "", imageUrl: "" }, poolId: null, extra });

/** Case name -> builder, in a fixed order (the resale case configures a global, so it runs last). */
const CASES: Array<[string, () => Promise<{ getData(): unknown }> | { getData(): unknown }]> = [
  ["buy/kiosk-native", () => buy([{ l: listing({}) }], { [RETRO]: [POL.lockRoyalty] }, [plainKiosk])],
  ["buy/kiosk-native-new-kiosk", () => buy([{ l: listing({}) }], { [RETRO]: [POL.royaltyOnly] }, [])],
  ["buy/personal", () => buy([{ l: listing({}) }], { [RETRO]: [POL.personal] }, [plainKiosk, personalKiosk])],
  ["buy/floor", () => buy([{ l: listing({}) }], { [RETRO]: [POL.floor] }, [plainKiosk])],
  ["buy/tradeport-simple-kiosk", () => buy([{ l: listing({ venue: "tradeport", source: "tradeport/simple", orderKey: ITEM, objectId: rep("77"), priceMist: "1000000000", feeMist: "30000000", kioskId: rep("88"), extra: { feeBps: 300, inOrderbook: true, listingType: 1 } }) }], { [RETRO]: [POL.lockRoyalty] }, [plainKiosk])],
  ["buy/tradeport-simple-personal", () => buy([{ l: listing({ venue: "tradeport", source: "tradeport/simple", orderKey: ITEM, priceMist: "1000000000", feeMist: "30000000", kioskId: rep("88"), extra: { inOrderbook: false, listingType: "1" } }) }], { [RETRO]: [POL.personal] }, [personalKiosk])],
  ["buy/tradeport-simple-wallet", () => buy([{ l: listing({ venue: "tradeport", source: "tradeport/simple", orderKey: ITEM, priceMist: "500000000", feeMist: "15000000", kioskId: null, extra: { inOrderbook: false, listingType: 0 } }) }], {}, [plainKiosk])],
  ["buy/tradeport-kiosk", () => buy([{ l: listing({ venue: "tradeport", source: "tradeport/kiosk", orderKey: ITEM, priceMist: "226600000", commissionMist: "6600000", kioskId: rep("99"), extra: {} }) }], { [RETRO]: [POL.lockRoyalty] }, [plainKiosk])],
  ["buy/tradeport-kiosk-early", () => buy([{ l: listing({ venue: "tradeport", source: "tradeport/kiosk-early", priceMist: "100", commissionMist: "3", kioskId: rep("99") }) }], { [RETRO]: [POL.royaltyOnly] }, [plainKiosk])],
  ["buy/tradeport-escrow", () => buy([{ l: listing({ venue: "tradeport", source: "tradeport/escrow", orderKey: rep("66"), kioskId: null, priceMist: "5000" }) }], {}, [plainKiosk])],
  ["buy/drip-v2", () => buy([{ l: listing({ venue: "drip", source: "drip/v2", feeMist: "240000" }) }], { [RETRO]: [POL.lockRoyalty] }, [plainKiosk])],
  ["buy/cart", () => buy([{ l: listing({}) }, { l: listing({ venue: "drip", source: "drip/v2", priceMist: "7", feeMist: "1", kioskId: rep("98") }) }, { l: listing({ venue: "tradeport", source: "tradeport/escrow", orderKey: rep("66"), kioskId: null, priceMist: "5000" }) }], { [RETRO]: [POL.lockRoyalty] }, [plainKiosk])],
  ["list/wallet-no-kiosk", () => buildListTransaction(card(), 5_000_000_000n, SELLER, api({ kiosks: [] }))],
  ["list/in-kiosk", () => buildListTransaction(card(inKiosk), 1_000_000_000n, SELLER, api({ kiosks: [sellerKiosk] }))],
  ["list/lock", () => buildListTransaction(card(), 1_000_000_000n, SELLER, api({ kiosks: [sellerKiosk], policy: POL.lockRoyalty }))],
  ["list/personal-new", () => buildListTransaction(card(), 1_000_000_000n, SELLER, api({ kiosks: [sellerKiosk], policy: POL.personal }))],
  ["list/personal-existing", () => buildListTransaction(card(), 1_000_000_000n, SELLER, api({ kiosks: [sellerKiosk, personalKiosk], policy: POL.personal }))],
  ["list/relist", () => buildListTransaction(card({ ...inKiosk, listing: nativeListing }), 20_000_000n, SELLER, api({ kiosks: [sellerKiosk] }))],
  ["accept/tradeport-bids-listed", () => buildAcceptOfferTransaction(card({ ...inKiosk, listing: nativeListing }), offer(), SELLER, api({ kiosks: [sellerKiosk], policy: POL.lockRoyalty }))],
  ["accept/tradeport-bids-wallet", () => buildAcceptOfferTransaction(card(), offer(), SELLER, api({ kiosks: [sellerKiosk], policy: POL.lockRoyalty }))],
  ["accept/tradeport-bids-no-policy", () => buildAcceptOfferTransaction(card(), offer(), SELLER, api({ policy: null }))],
  ["accept/tradeport-kiosk-bids", () => buildAcceptOfferTransaction(card(inKiosk), offer({ ...itemOffer("tradeport/kiosk-bids", { escrow: "904000000" }), commissionMist: "24000000", priceMist: "800000000" }), SELLER, api({ kiosks: [sellerKiosk], policy: POL.lockRoyalty }))],
  ["accept/tradeport-escrow-bids", () => buildAcceptOfferTransaction(card(), offer({ source: "tradeport/escrow-bids", poolId: null }), SELLER, api())],
  ["accept/drip-v2-item", () => buildAcceptOfferTransaction(card(inKiosk), itemOffer("drip/v2-bids"), SELLER, api({ kiosks: [sellerKiosk], policy: POL.lockRoyalty }))],
  ["accept/drip-v2-collection-wallet", () => buildAcceptOfferTransaction(card(), offer({ venue: "drip", source: "drip/v2-bids", poolId: null }), SELLER, api({ kiosks: [personalKiosk], policy: POL.lockRoyalty }))],
  ["cancel/native", () => buildCancelTransaction({ ...card({ ...inKiosk, listing: nativeListing }) } as InvItem, SELLER, api({ kiosks: [sellerKiosk] }), async () => [])],
  [
    "buy/resale",
    () => {
      configureResale({ origin: rep("9a"), pkg: rep("9b") });
      const LICENCE = `${rep("9a")}::licence::ResaleLicence`;
      const policy: TransferPolicy = { id: rep("9d"), isShared: true, rules: [lock, { type: "rule", ruleType: `${rep("9a")}::resale::Rule` }] };
      return buy([{ l: listing({ nftType: LICENCE }), game: rep("9c") }], { [LICENCE]: [policy] }, [plainKiosk]);
    },
  ],
];

describe("golden PTBs for the Rust port", () => {
  it("match ptb.json", async () => {
    const out: Record<string, unknown> = {};
    for (const [name, make] of CASES) out[name] = (await make()).getData();
    const text = JSON.stringify(out, null, 1) + "\n";
    if (process.env.UPDATE_GOLDEN) writeFileSync(FILE, text);
    expect(JSON.parse(readFileSync(FILE, "utf8"))).toEqual(JSON.parse(text));
  });
});
