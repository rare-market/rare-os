// Unit tests for the ported rare.market buy builder. The market repo has no unit tests for
// packages/venues/src/build.ts (its indexer/scripts/simulate.ts checks builds against mainnet);
// these check the PTB commands per listing source on fixtures shaped like live /v1 listings.
// The mainnet simulation is in live.test.ts (LIVE=1).

import type { Transaction } from "@mysten/sui/transactions";
import { describe, expect, it } from "vitest";
import { KIOSK_EXT, TRADEPORT } from "./ids";
import { allInPrice, buildCartBuy, choosePolicy, configureResale, loadTradeContext, toActiveListing, toVenuePolicy, type ApiListing, type OwnedKioskRef, type TransferPolicy } from "./index";

const BUYER = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const RETRO = "0x44448dbb2b9e685ef070125725ab5263c3fc6a6acc11820550187e7759101d6d::retrosui::Nft";
const ITEM = "0x025231045a67bb6b53acdc6b178742e20853cf37bb7cc6b7114fb8df262b02f2";
const SELLER_KIOSK = "0x10785ddeb5258be06802aed6d57ec411415d0a86902bf334cea00fd8573ee6f3";
const EXT = "0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879";

const lockRoyalty: TransferPolicy = {
  id: "0x7309bb80aee534f2b4334c9b97df0227412ed51e0a8b103fc3c8c751a925cf94",
  isShared: true,
  rules: [
    { type: "kiosk_lock_rule", ruleType: `${EXT}::kiosk_lock_rule::Rule` },
    { type: "royalty_rule", ruleType: `${EXT}::royalty_rule::Rule`, config: { amount_bp: 1000, min_amount: "0" } },
  ],
};
const royaltyOnly: TransferPolicy = { id: "0x" + "ab".repeat(32), isShared: true, rules: [lockRoyalty.rules[1]] };
const personal: TransferPolicy = {
  id: "0x" + "cd".repeat(32),
  isShared: true,
  rules: [{ type: "personal_kiosk_rule", ruleType: `${KIOSK_EXT.personalOrigin}::personal_kiosk_rule::Rule` }, ...lockRoyalty.rules],
};
const custom: TransferPolicy = { id: "0x" + "ee".repeat(32), isShared: true, rules: [{ type: "witness_rule", ruleType: "0x" + "12".repeat(32) + "::witness_rule::Rule" }] };

const plainKiosk: OwnedKioskRef = { kioskId: "0x" + "11".repeat(32), ownerCapId: "0x" + "22".repeat(32), personalCapId: null, isPersonal: false };
const personalKiosk: OwnedKioskRef = { kioskId: "0x" + "33".repeat(32), ownerCapId: "0x" + "44".repeat(32), personalCapId: "0x" + "55".repeat(32), isPersonal: true };

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

/** Move calls as "module::function", short package aliases for the venues. */
function calls(tx: Transaction): string[] {
  const alias = (p: string) => {
    const n = p.replace(/^0x0*/, "0x");
    if (n === "0x2") return "0x2";
    if (p === KIOSK_EXT.latest) return "ext";
    for (const [name, fam] of Object.entries(TRADEPORT)) if ("latest" in fam && fam.latest === p) return `tp.${name}`;
    return p.slice(0, 6);
  };
  return tx.getData().commands.flatMap((c) => (c.MoveCall ? [`${alias(c.MoveCall.package)}::${c.MoveCall.module}::${c.MoveCall.function}`] : []));
}
/** Every u64 the PTB splits off the gas coin as a literal amount. */
function splits(tx: Transaction): bigint[] {
  const d = tx.getData();
  const out: bigint[] = [];
  for (const c of d.commands) {
    if (!c.SplitCoins) continue;
    for (const a of c.SplitCoins.amounts) {
      if (a.$kind !== "Input") continue;
      const inp = d.inputs[a.Input];
      if (inp.Pure) out.push(new DataView(Uint8Array.from(atob(inp.Pure.bytes), (ch) => ch.charCodeAt(0)).buffer).getBigUint64(0, true));
    }
  }
  return out;
}

const ctx = (policies: TransferPolicy[], kiosks: OwnedKioskRef[] = [plainKiosk]) => ({ sender: BUYER, buyerKiosks: kiosks, policies: { [RETRO]: policies } });

describe("rare buy builder (ported buildCartBuy)", () => {
  it("kiosk/native: purchase, lock into the buyer's kiosk, pay royalty, prove lock, confirm", () => {
    const tx = buildCartBuy([{ itemId: ITEM, listing: listing({}) }], ctx([lockRoyalty]));
    expect(calls(tx)).toEqual([
      "0x2::kiosk::purchase",
      "0x2::kiosk::lock",
      "0x2::transfer_policy::paid",
      "ext::royalty_rule::fee_amount",
      "ext::royalty_rule::pay",
      "ext::kiosk_lock_rule::prove",
      "0x2::transfer_policy::confirm_request",
    ]);
    expect(splits(tx)).toEqual([12_000_000n]);
    expect(tx.getData().sender).toBe(BUYER);
  });

  it("kiosk/native with no buyer kiosk: creates one, shares it, sends the cap to the buyer", () => {
    const tx = buildCartBuy([{ itemId: ITEM, listing: listing({}) }], ctx([royaltyOnly], []));
    const c = calls(tx);
    expect(c[0]).toBe("0x2::kiosk::new");
    expect(c).toContain("0x2::kiosk::place");
    expect(c).not.toContain("0x2::kiosk::lock");
    expect(c.at(-1)).toBe("0x2::transfer::public_share_object");
    expect(tx.getData().commands.at(-1)?.TransferObjects).toBeTruthy();
  });

  it("personal kiosk rule: borrows the personal cap, proves, returns it", () => {
    const tx = buildCartBuy([{ itemId: ITEM, listing: listing({}) }], ctx([personal], [plainKiosk, personalKiosk]));
    const c = calls(tx);
    expect(c[0]).toBe("ext::personal_kiosk::borrow_val");
    expect(c).toContain("ext::personal_kiosk_rule::prove");
    expect(c.at(-1)).toBe("ext::personal_kiosk::return_val");
  });

  it("tradeport/simple, kiosk listing: fee and royalty in the payment, venue proves lock", () => {
    const l = listing({
      venue: "tradeport",
      source: "tradeport/simple",
      orderKey: ITEM,
      objectId: "0x" + "77".repeat(32),
      priceMist: "1000000000",
      feeMist: "30000000",
      allInMist: "1130000000",
      kioskId: "0x" + "88".repeat(32),
      extra: { feeBps: 300, inOrderbook: true, listingType: 1, royaltyMist: "100000000" },
    });
    const tx = buildCartBuy([{ itemId: ITEM, listing: l }], ctx([lockRoyalty]));
    expect(calls(tx)).toEqual(["ext::royalty_rule::fee_amount", "tp.simple::tradeport_listings::buy_ob_listing_with_transfer_policy", "0x2::transfer_policy::confirm_request"]);
    expect(splits(tx)).toEqual([1_030_000_000n]);
  });

  it("tradeport/simple, wallet listing not yet in the orderbook: reports it first, no policy needed", () => {
    const l = listing({ venue: "tradeport", source: "tradeport/simple", orderKey: ITEM, priceMist: "500000000", feeMist: "15000000", kioskId: null, extra: { inOrderbook: false, listingType: 0 } });
    const tx = buildCartBuy([{ itemId: ITEM, listing: l }], ctx([]));
    expect(calls(tx)).toEqual(["tp.simple::tradeport_listings::report_ob_listing", "tp.simple::tradeport_listings::buy_ob_listing_without_transfer_policy"]);
    expect(splits(tx)).toEqual([515_000_000n]);
  });

  it("tradeport/kiosk: legacy kiosk_listings::buy, then the policy is settled into the buyer's kiosk", () => {
    const l = listing({ venue: "tradeport", source: "tradeport/kiosk", orderKey: ITEM, priceMist: "226600000", commissionMist: "6600000", kioskId: "0x" + "99".repeat(32), extra: {} });
    const tx = buildCartBuy([{ itemId: ITEM, listing: l }], ctx([lockRoyalty]));
    const c = calls(tx);
    expect(c[0]).toBe("tp.kiosk::kiosk_listings::buy");
    expect(c.slice(1)).toEqual(["0x2::kiosk::lock", "0x2::transfer_policy::paid", "ext::royalty_rule::fee_amount", "ext::royalty_rule::pay", "ext::kiosk_lock_rule::prove", "0x2::transfer_policy::confirm_request"]);
    expect(splits(tx)).toEqual([226_600_000n]);
    // royalty on price minus commission, as the API prices it (RetroSui #76: 220 * 10% = 22)
    expect(allInPrice(toActiveListing(ITEM, l), toVenuePolicy(lockRoyalty, RETRO))).toBe(248_600_000n);
  });

  it("tradeport/kiosk-early: commission on top, buyer cap passed", () => {
    const l = listing({ venue: "tradeport", source: "tradeport/kiosk-early", priceMist: "100", commissionMist: "3", kioskId: "0x" + "99".repeat(32) });
    const tx = buildCartBuy([{ itemId: ITEM, listing: l }], ctx([royaltyOnly]));
    expect(calls(tx)[0]).toBe("tp.kioskEarly::kiosk_listings::buy");
    expect(splits(tx)).toEqual([103n]);
  });

  it("tradeport/escrow: listings::buy by listing id, no policy, item goes to the wallet", () => {
    const l = listing({ venue: "tradeport", source: "tradeport/escrow", orderKey: "0x" + "66".repeat(32), kioskId: null, priceMist: "5000" });
    const tx = buildCartBuy([{ itemId: ITEM, listing: l }], ctx([]));
    expect(calls(tx)).toEqual(["tp.escrow::listings::buy"]);
    expect(allInPrice(toActiveListing(ITEM, l), toVenuePolicy(lockRoyalty, RETRO))).toBe(5000n);
  });

  it("drip/v2: bought direct through the kiosk (no fee), then Drip's record is cleared", () => {
    const l = listing({ venue: "drip", source: "drip/v2", feeMist: "240000" });
    const tx = buildCartBuy([{ itemId: ITEM, listing: l }], ctx([lockRoyalty]));
    const c = calls(tx);
    expect(c[0]).toBe("0x2::kiosk::purchase");
    expect(c.slice(-2)).toEqual(["0x2::kiosk::list", "0xe503::marketplace::delist"]);
    expect(splits(tx)).toEqual([12_000_000n]);
  });

  it("market/v1 (rare's own contract, not published): plain unsupported message", () => {
    expect(() => buildCartBuy([{ itemId: ITEM, listing: listing({ venue: "market", source: "market/v1" }) }], ctx([lockRoyalty]))).toThrow("Buying from Rare is not available for this item yet.");
  });

  it("a policy with a rule nobody can prove is never chosen; the buy says so plainly", () => {
    expect(choosePolicy([custom])).toBeNull();
    expect(choosePolicy([custom, lockRoyalty])?.id).toBe(lockRoyalty.id);
    expect(() => buildCartBuy([{ itemId: ITEM, listing: listing({}) }], ctx([custom]))).toThrow("Buying from Kiosk is not available for this item yet.");
  });

  it("loadTradeContext reads kiosks and policies from the rare API paths, chain fallback for kiosks", async () => {
    const seen: string[] = [];
    const get = async <T,>(p: string): Promise<T> => {
      seen.push(p);
      if (p.includes("/kiosks")) throw new Error("rare.market 502");
      return { data: [lockRoyalty] } as T;
    };
    const ctx2 = await loadTradeContext(get, BUYER, [RETRO, RETRO], async () => [plainKiosk]);
    expect(seen).toEqual([`/v1/accounts/${BUYER}/kiosks`, `/v1/policies/${encodeURIComponent(RETRO)}`]);
    expect(ctx2.buyerKiosks).toEqual([plainKiosk]);
    expect(ctx2.policies[RETRO]).toEqual([lockRoyalty]);
  });

  it("Rare Store ResaleLicence: lock, kiosk_lock_rule::prove, resale::prove, then confirm", () => {
    const origin = "0x" + "9a".repeat(32);
    const pkg = "0x" + "9b".repeat(32);
    const game = "0x" + "9c".repeat(32);
    const LICENCE = `${origin}::licence::ResaleLicence`;
    const policy: TransferPolicy = {
      id: "0x" + "9d".repeat(32),
      isShared: true,
      rules: [lockRoyalty.rules[0], { type: "rule", ruleType: `${origin}::resale::Rule` }],
    };
    const lic = (over: Partial<ApiListing> = {}) => listing({ nftType: LICENCE, ...over });
    const c = (): { sender: string; buyerKiosks: OwnedKioskRef[]; policies: Record<string, TransferPolicy[]> } => ({ sender: BUYER, buyerKiosks: [plainKiosk], policies: { [LICENCE]: [policy] } });
    // Until the store config has a package the rule is unknown: not buyable.
    expect(() => buildCartBuy([{ itemId: ITEM, listing: lic(), resaleGame: game }], c())).toThrow(/not available/);
    configureResale({ origin, pkg });
    const tx = buildCartBuy([{ itemId: ITEM, listing: lic(), resaleGame: game }], c());
    expect(calls(tx)).toEqual(["0x2::kiosk::purchase", "0x2::kiosk::lock", "ext::kiosk_lock_rule::prove", `${pkg.slice(0, 6)}::resale::prove`, "0x2::transfer_policy::confirm_request"]);
    // The game id is required.
    expect(() => buildCartBuy([{ itemId: ITEM, listing: lic() }], c())).toThrow(/not available/);
  });
});
