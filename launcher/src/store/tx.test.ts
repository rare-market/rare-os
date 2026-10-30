import type { Transaction } from "@mysten/sui/transactions";
import { describe, expect, it } from "vitest";
import { PYTH, vaaFromAccumulator } from "./pyth";
import { buildBuySui, buildBuyUsdc, buildClaimFree, buildMakeTradable, buildRefund, suiQuote, usdToMist, type StoreIds } from "./tx";

const A = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;
const USDC = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
const ids: StoreIds = { package: A(0xaa), store: A(0x51), storeVersion: "11", usdcType: USDC, policy: A(0x90), policyVersion: "12" };
const sender = A(0x5e);

/** Move calls in order, as "package::module::function<types>". */
function calls(tx: Transaction): string[] {
  return tx.getData().commands.flatMap((c) => {
    if (c.$kind !== "MoveCall") return [];
    const m = c.MoveCall!;
    const t = m.typeArguments.length ? `<${m.typeArguments.join(",")}>` : "";
    return [`${m.module}::${m.function}${t}`];
  });
}

// A real Hermes accumulator message has a VAA inside; this one is shaped like it.
function fakeUpdate(): string {
  const vaa = [1, 2, 3, 4, 5];
  const msg = [0x50, 0x4e, 0x41, 0x55, 1, 0, 0, 0, vaa.length >> 8, vaa.length & 255, ...vaa, 9, 9];
  return btoa(String.fromCharCode(...msg));
}

describe("prices in SUI", () => {
  it("rounds up like store::usd_to_mist", () => {
    // $14.99 at $0.906: 14.99 / 0.906 SUI = 16.545253... SUI
    expect(usdToMist(14_990_000n, 90_600_000n)).toBe(16_545_253_864n);
    expect(usdToMist(1n, 3n)).toBe(33_333_333_334n);
  });
  it("bounds the pay coin at the quote plus 1%", () => {
    const q = suiQuote(10_000_000n, { price_e8: "100000000" });
    expect(q.mist).toBe(10_000_000_000n);
    expect(q.maxMist).toBe(10_100_000_000n);
  });
});

describe("Pyth", () => {
  it("finds the VAA inside an accumulator message", () => {
    const msg = Uint8Array.from([0x50, 0x4e, 0x41, 0x55, 1, 0, 2, 7, 7, 0, 0, 3, 9, 8, 7, 1]);
    expect(Array.from(vaaFromAccumulator(msg))).toEqual([9, 8, 7]);
  });
});

describe("store transactions", () => {
  it("buy_usdc pays with an exact USDC coin and destroys the empty one", () => {
    const tx = buildBuyUsdc(ids, { sender, gameId: A(0x6a), priceUsd: 14_990_000n });
    expect(calls(tx)).toEqual([`game::buy_usdc<${USDC}>`, `coin::destroy_zero<${USDC}>`]);
    expect(tx.getData().sender).toBe(sender);
    const store = tx.getData().inputs.find((i) => i.UnresolvedObject?.objectId === ids.store || i.Object?.SharedObject?.objectId === ids.store);
    expect(store?.Object?.SharedObject).toMatchObject({ initialSharedVersion: "11", mutable: false });
  });

  it("buy_sui pushes the Pyth price first and returns the change to gas", () => {
    const { tx, mist, maxMist } = buildBuySui(ids, { sender, gameId: A(0x6a), priceUsd: 14_990_000n, price: { price_e8: "90600000", conf_e8: "1", publish_time: 0, update_b64: fakeUpdate() } });
    expect(calls(tx)).toEqual([
      "vaa::parse_and_verify",
      "pyth::create_authenticated_price_infos_using_accumulator",
      "pyth::update_single_price_feed",
      `hot_potato_vector::destroy<${PYTH.original}::price_info::PriceInfo>`,
      "game::buy_sui",
    ]);
    expect(maxMist).toBeGreaterThan(mist);
    const kinds = tx.getData().commands.map((c) => c.$kind);
    expect(kinds[kinds.length - 1]).toBe("MergeCoins");
  });

  it("refuses free games and missing prices", () => {
    expect(() => buildBuyUsdc(ids, { sender, gameId: A(1), priceUsd: 0n })).toThrow();
    // fresh on-chain price: no update calls, buy_sui straight after the split
    const f = buildBuySui(ids, { sender, gameId: A(0x6a), priceUsd: 14_990_000n, price: { price_e8: "90600000", conf_e8: "1", publish_time: 0, update_b64: "", fresh: true, age_s: 4 } });
    const targets = (f.tx.getData().commands as { MoveCall?: { function: string } }[]).map((c) => c.MoveCall?.function).filter(Boolean);
    expect(targets).toEqual(["buy_sui"]);
    expect(() => buildBuySui(ids, { sender, gameId: A(1), priceUsd: 5n, price: { price_e8: "1", conf_e8: "1", publish_time: 0, update_b64: "" } })).toThrow(/price/);
  });

  it("claim_free and refund", () => {
    expect(calls(buildClaimFree(ids, { sender, gameId: A(1) }))).toEqual(["game::claim_free"]);
    expect(calls(buildRefund(ids, { sender, gameId: A(1), licenceId: A(2) }))).toEqual([`game::refund<${USDC}>`]);
  });

  it("make_tradable uses the sender's kiosk, or creates and shares one", () => {
    const own = buildMakeTradable(ids, { sender, gameId: A(1), licenceId: A(2), kiosks: [{ kioskId: A(0x31), ownerCapId: A(0x32), isPersonal: false }] });
    expect(calls(own)).toEqual([`resale::make_tradable<${USDC}>`]);
    const fresh = buildMakeTradable(ids, { sender, gameId: A(1), licenceId: A(2), kiosks: [] });
    expect(calls(fresh)).toEqual(["kiosk::new", `resale::make_tradable<${USDC}>`, "transfer::public_share_object<0x2::kiosk::Kiosk>"]);
  });
});
