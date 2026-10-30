import { describe, expect, it } from "vitest";
import {
  GAS_RESERVE_MIST,
  SUI_LADDER,
  SUI_TYPE,
  USDC_TYPE,
  fmtLeverage,
  fromBase,
  impliedPrice,
  leverageStops,
  minOut,
  normalizeType,
  pctOf,
  spendable,
  step,
  toBase,
  validPair,
} from "./amount";

describe("toBase / fromBase", () => {
  it("converts whole-token amounts exactly", () => {
    expect(toBase("1.5", 9)).toBe(1_500_000_000n);
    expect(toBase("0.000000001", 9)).toBe(1n);
    expect(toBase("25", 6)).toBe(25_000_000n);
    expect(toBase(0.1, 9)).toBe(100_000_000n);
    expect(toBase(1e-7, 9)).toBe(100n);
  });

  it("drops digits past the coin's decimals instead of rounding up", () => {
    expect(toBase("1.0000000019", 9)).toBe(1_000_000_001n);
    expect(toBase("0.1234567", 6)).toBe(123_456n);
  });

  it("treats bad input as zero", () => {
    expect(toBase("", 9)).toBe(0n);
    expect(toBase(".", 9)).toBe(0n);
    expect(toBase("1e3", 9)).toBe(0n);
    expect(toBase("-1", 9)).toBe(0n);
    expect(toBase(Number.NaN, 9)).toBe(0n);
  });

  it("prints base units back without trailing zeros", () => {
    expect(fromBase(1_500_000_000n, 9)).toBe("1.5");
    expect(fromBase(1n, 9)).toBe("0.000000001");
    expect(fromBase(25_000_000n, 6)).toBe("25");
    expect(fromBase(0n, 9)).toBe("0");
    expect(fromBase(-2_500_000n, 6)).toBe("-2.5");
    expect(fromBase(7n, 0)).toBe("7");
  });

  it("round trips", () => {
    for (const s of ["0.1", "12.345678", "1000000", "0.000001"]) expect(fromBase(toBase(s, 6), 6)).toBe(s);
  });
});

describe("step (LT / RT)", () => {
  it("walks the ladder both ways", () => {
    expect(step(SUI_LADDER, 1, 1)).toBe(2);
    expect(step(SUI_LADDER, 2, -1)).toBe(1);
    expect(step(SUI_LADDER, 1.3, 1)).toBe(2);
    expect(step(SUI_LADDER, 1.3, -1)).toBe(1);
  });

  it("lands on the maximum instead of passing it", () => {
    expect(step(SUI_LADDER, 10, 1, 12.4)).toBe(12.4);
    expect(step(SUI_LADDER, 12.4, 1, 12.4)).toBe(12.4);
    expect(step(SUI_LADDER, 12.4, -1, 12.4)).toBe(10);
  });

  it("stops at the ends", () => {
    expect(step(SUI_LADDER, 0.1, -1)).toBe(0.1);
    expect(step(SUI_LADDER, 10000, 1)).toBe(10000);
    expect(step([1, 2, 3], 2, -1, Infinity, 1.5)).toBe(1.5);
  });

  it("clamps a step down to the maximum when the balance shrank", () => {
    expect(step(SUI_LADDER, 100, -1, 20)).toBe(20);
  });
});

describe("balances", () => {
  it("keeps gas back on SUI only", () => {
    expect(spendable(1_000_000_000n, SUI_TYPE)).toBe(1_000_000_000n - GAS_RESERVE_MIST);
    expect(spendable(10_000_000n, SUI_TYPE)).toBe(0n);
    expect(spendable(5_000_000n, USDC_TYPE)).toBe(5_000_000n);
  });

  it("takes exact shares", () => {
    expect(pctOf(1_000n, 25)).toBe(250n);
    expect(pctOf(999n, 100)).toBe(999n);
    expect(pctOf(1_000n, 33)).toBe(330n);
    expect(pctOf(1_000n, 0)).toBe(0n);
  });
});

describe("leverage", () => {
  it("offers the ladder up to the market's maximum, maximum included", () => {
    expect(leverageStops(5)).toEqual([1, 1.5, 2, 3, 4, 5]);
    expect(leverageStops(15).at(-1)).toBe(15);
    expect(leverageStops(7)).toEqual([1, 1.5, 2, 3, 4, 5, 6, 7]);
    expect(leverageStops(null)).toEqual([1]);
  });

  it("prints compactly", () => {
    expect(fmtLeverage(5)).toBe("5x");
    expect(fmtLeverage(1.5)).toBe("1.5x");
  });
});

describe("quotes", () => {
  it("applies slippage to the minimum received", () => {
    expect(minOut(10_000n, 100)).toBe(9_900n);
    expect(minOut(90_459n)).toBe(89_554n);
  });

  it("derives a price from a quote", () => {
    expect(impliedPrice(1_000_000_000n, 9, 2_000_000n, 6)).toBe(0.5);
    expect(impliedPrice(0n, 9, 1n, 6)).toBeNull();
  });

  it("compares coin types in their long form", () => {
    expect(normalizeType("0x2::sui::SUI")).toBe(SUI_TYPE);
    expect(validPair("0x2::sui::SUI", SUI_TYPE)).toBe(false);
    expect(validPair("0x2::sui::SUI", USDC_TYPE)).toBe(true);
    expect(validPair("sui", USDC_TYPE)).toBe(false);
  });
});
