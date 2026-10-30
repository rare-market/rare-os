import { describe, expect, it } from "vitest";
import {
  age,
  count,
  pct,
  shortName,
  usdCompact,
  usdPrice,
  usdPriceFine,
} from "./format";

describe("usdCompact", () => {
  it("compacts by magnitude", () => {
    expect(usdCompact(812)).toBe("$812");
    expect(usdCompact(45_600)).toBe("$45.6K");
    expect(usdCompact(1_230_000)).toBe("$1.23M");
    expect(usdCompact(5_010_000_000)).toBe("$5.01B");
    expect(usdCompact(50_000)).toBe("$50K");
    expect(usdCompact(2_500_000)).toBe("$2.5M");
    expect(usdCompact(0.5)).toBe("$0.50");
    expect(usdCompact(100)).toBe("$100");
  });

  it("leaves missing values out", () => {
    expect(usdCompact(null)).toBe("");
    expect(usdCompact(Number.NaN)).toBe("");
  });
});

describe("usdPrice", () => {
  it("uses subscript zeros for tiny prices", () => {
    expect(usdPrice(0.0000041234)).toBe("$0.0₅4123");
  });

  it("keeps normal prices readable", () => {
    expect(usdPrice(1.2134)).toBe("$1.213");
    expect(usdPrice(0.0242)).toBe("$0.0242");
    expect(usdPrice(86_597.34)).toBe("$86,597.34");
    expect(usdPrice(4109.9)).toBe("$4,109.90");
  });

  it("leaves zero or missing prices out", () => {
    expect(usdPrice(0)).toBe("");
    expect(usdPrice(undefined)).toBe("");
  });
});

describe("pct / count / age", () => {
  it("formats percentages without a sign (the arrow carries it)", () => {
    expect(pct(-28.4567)).toBe("28.46%");
    expect(pct(240_016_237)).toBe("240,016,237%");
  });

  it("compacts counts", () => {
    expect(count(901)).toBe("901");
    expect(count(14_820)).toBe("14.8K");
    expect(count(317_300)).toBe("317.3K");
    expect(count(2_500_000_000)).toBe("2.5B");
    expect(count(1.2e15)).toBe("1.2Q");
    expect(count(18_000)).toBe("18K");
    expect(count(null)).toBe("");
  });

  it("gives short ages", () => {
    const now = 1_000_000_000_000;
    expect(age(now - 30_000, now)).toBe("30s");
    expect(age(now - 14 * 60_000, now)).toBe("14m");
    expect(age(now - 5 * 3_600_000, now)).toBe("5h");
    expect(age(now - 3 * 86_400_000, now)).toBe("3d");
  });
});

describe("shortName", () => {
  it("shortens a bare address used as a name", () => {
    expect(shortName("0x" + "ab".repeat(32))).toBe("0xabab\u2060…\u2060abab");
  });

  it("shortens a coin type", () => {
    expect(shortName("0x" + "cd".repeat(32) + "::foo::FOO")).toBe(
      "0xcdcd\u2060…\u2060cdcd"
    );
  });

  it("keeps an already short address on one line", () => {
    expect(shortName("@0x9a5b…5f2e")).toBe("@0x9a5b\u2060…\u20605f2e");
  });

  it("keeps real names", () => {
    expect(shortName("degen")).toBe("degen");
    expect(shortName("Big Whale 0x")).toBe("Big Whale 0x");
  });
});

describe("usdPriceFine", () => {
  it("shows per-second moves a normal price label rounds away", () => {
    expect(usdPriceFine(86_597.34)).toBe("$86,597.34");
    expect(usdPrice(81.2341)).toBe(usdPrice(81.2344));
    expect(usdPriceFine(81.2341)).not.toBe(usdPriceFine(81.2344));
    expect(usdPriceFine(0.02421)).not.toBe(usdPriceFine(0.02424));
    expect(usdPriceFine(0)).toBe("");
  });
});
