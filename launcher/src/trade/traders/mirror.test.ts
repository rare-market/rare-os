import { describe, expect, it } from "vitest";
import { GAS_RESERVE_MIST, SUI_TYPE, USDC_TYPE } from "../amount";
import { mirrorPlan } from "./mirror";

const DEEP = "0xdeeb7a4662eec9f2f3def03fb937a663dddaa2e215b8078a284d026b7946c270::deep::DEEP";
const size = { buySui: 5, sellPct: 50 };

describe("mirrorPlan", () => {
  it("a buy spends the chosen SUI on the same coin", () => {
    expect(mirrorPlan({ buy: true, coin: DEEP, priceUsd: 0.1 }, size, 6, null)).toEqual({ ok: true, side: "buy", coinIn: SUI_TYPE, coinOut: DEEP, amountIn: 5_000_000_000n, decimalsIn: 9 });
  });
  it("a sell sells the chosen share of what the account holds, for SUI", () => {
    expect(mirrorPlan({ buy: false, coin: DEEP, priceUsd: 0.1 }, size, 6, 1_000_001n)).toEqual({ ok: true, side: "sell", coinIn: DEEP, coinOut: SUI_TYPE, amountIn: 500_000n, decimalsIn: 6 });
    expect(mirrorPlan({ buy: false, coin: DEEP, priceUsd: 0.1 }, { ...size, sellPct: 100 }, 6, 7n)).toMatchObject({ amountIn: 7n });
  });
  it("nothing held, or not known yet, is not a trade", () => {
    expect(mirrorPlan({ buy: false, coin: DEEP, priceUsd: 0.1 }, size, 6, 0n)).toEqual({ ok: false, reason: "You hold none of this coin." });
    expect(mirrorPlan({ buy: false, coin: DEEP, priceUsd: 0.1 }, size, 6, null).ok).toBe(false);
  });
  it("SUI itself trades against USDC, keeping gas back on a sell", () => {
    expect(mirrorPlan({ buy: true, coin: "0x2::sui::SUI", priceUsd: 0.9 }, size, 9, null)).toEqual({ ok: true, side: "buy", coinIn: USDC_TYPE, coinOut: SUI_TYPE, amountIn: 4_500_000n, decimalsIn: 6 });
    const held = 10_000_000_000n;
    expect(mirrorPlan({ buy: false, coin: SUI_TYPE, priceUsd: 0.9 }, { ...size, sellPct: 100 }, 9, held)).toEqual({ ok: true, side: "sell", coinIn: SUI_TYPE, coinOut: USDC_TYPE, amountIn: held - GAS_RESERVE_MIST, decimalsIn: 9 });
  });
  it("no size, no trade", () => {
    expect(mirrorPlan({ buy: true, coin: DEEP, priceUsd: 0.1 }, { buySui: 0, sellPct: 50 }, 6, null).ok).toBe(false);
  });
});
