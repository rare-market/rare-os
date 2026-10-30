import { describe, expect, it } from "vitest";
// The Trade tab's numbers go through the UI kit's rules (../ui/format, the Trade API's format-num): this
// pins the rules Trade depends on.
import { changedChars, compactFigure, numParts, numText } from "../ui/format";

describe("money", () => {
  it("shows full cents and splits them off for dimming", () => {
    expect(numParts("money", 4151.49)).toEqual({
      sign: "",
      body: "$4,151",
      dim: ".49",
      tone: null,
    });
    expect(numText("money", 9366.96)).toBe("$9,366.96");
    expect(numText("money", 0)).toBe("$0.00");
  });

  it("uses a real minus and no plus", () => {
    expect(numText("money", -12)).toBe("−$12.00");
    expect(numText("money", 12)).toBe("$12.00");
  });

  it("never rounds a dust balance to a lie", () => {
    expect(numText("money", 0.004)).toBe("<$0.01");
    expect(numText("money", 0.006)).toBe("$0.01");
  });

  it("can drop cents or go compact when asked", () => {
    expect(numText("money", 412.6, { cents: false })).toBe("$413");
    expect(numText("money", 35_550.12, { compact: true })).toBe("$35.6K");
  });
});

describe("pnl", () => {
  it("is always signed and toned", () => {
    expect(numParts("pnl", 4151.49)).toEqual({
      sign: "+",
      body: "$4,151",
      dim: ".49",
      tone: "up",
    });
    expect(numParts("pnl", -12)).toEqual({
      sign: "−",
      body: "$12",
      dim: ".00",
      tone: "down",
    });
  });

  it("is flat and unsigned at zero, including values that show as zero", () => {
    expect(numParts("pnl", 0)).toEqual({
      sign: "",
      body: "$0",
      dim: ".00",
      tone: "flat",
    });
    expect(numText("pnl", -0.001)).toBe("$0.00");
    expect(numParts("pnl", 0.001)?.tone).toBe("flat");
  });

  it("supports headline and compact forms", () => {
    expect(numText("pnl", 412.6, { cents: false })).toBe("+$413");
    expect(numText("pnl", -35_600, { compact: true })).toBe("−$35.6K");
  });
});

describe("market", () => {
  it("is compact with at most four digits", () => {
    expect(numText("market", 10_910_000)).toBe("$10.91M");
    expect(numText("market", 247_300)).toBe("$247K");
    expect(numText("market", 54_110_000)).toBe("$54.11M");
    expect(numText("market", 1_980_000)).toBe("$1.98M");
    expect(numText("market", 247_300_000)).toBe("$247.3M");
    expect(numText("market", 5_010_000_000)).toBe("$5.01B");
  });

  it("never pads with zeros", () => {
    expect(numText("market", 81_000)).toBe("$81K");
    expect(numText("market", 6_300)).toBe("$6.3K");
    expect(numText("market", 123_000_000)).toBe("$123M");
    expect(numText("market", 94_050_000)).toBe("$94.05M");
    expect(numText("market", 973_000)).toBe("$973K");
    expect(numText("market", 100_000)).toBe("$100K");
    expect(numText("market", 500_000_000)).toBe("$500M");
    expect(numText("count", 10_000)).toBe("10K");
    expect(numText("market", 1_500_000)).toBe("$1.5M");
    expect(numText("market", 16_000)).toBe("$16K");
  });

  it("handles small figures and unit edges", () => {
    expect(numText("market", 812)).toBe("$812");
    expect(numText("market", 1.11)).toBe("$1.11");
    expect(numText("market", 0)).toBe("$0");
    expect(numText("market", 0.0042)).toBe("$0.0042");
    expect(numText("market", 999_960)).toBe("$1M");
    expect(numText("market", 999.6)).toBe("$1K");
  });
});

describe("price", () => {
  it("uses three significant digits", () => {
    expect(numText("price", 0.0109)).toBe("$0.0109");
    expect(numText("price", 0.223)).toBe("$0.223");
    expect(numText("price", 0.00931)).toBe("$0.00931");
  });

  it("uses subscript zeros under 0.001", () => {
    expect(numText("price", 0.0000041234)).toBe("$0.0₅4123");
  });

  it("keeps large prices whole", () => {
    expect(numText("price", 4104.58)).toBe("$4,104.58");
  });

  it("has nothing to show for zero", () => {
    expect(numParts("price", 0)).toBeNull();
  });
});

describe("change", () => {
  it("always carries a sign and a tone", () => {
    expect(numParts("change", 4.15)).toEqual({
      sign: "+",
      body: "4.15%",
      dim: "",
      tone: "up",
    });
    expect(numParts("change", -9.456)).toEqual({
      sign: "−",
      body: "9.46%",
      dim: "",
      tone: "down",
    });
  });

  it("uses fewer decimals from 100% up", () => {
    expect(numText("change", 255.55)).toBe("+255.6%");
    expect(numText("change", 4321.4)).toBe("+4,321%");
    expect(numText("change", 240_016_237)).toBe("+240M%");
  });

  it("is flat when it shows as zero", () => {
    expect(numParts("change", 0)).toEqual({
      sign: "",
      body: "0.00%",
      dim: "",
      tone: "flat",
    });
    expect(numText("change", -0.001)).toBe("0.00%");
  });
});

describe("count", () => {
  it("is exact under 1,000 and compact after", () => {
    expect(numText("count", 901)).toBe("901");
    expect(numText("count", 23_400)).toBe("23.4K");
    expect(numText("count", 317_300)).toBe("317.3K");
    expect(numText("count", 1_500)).toBe("1.5K");
    expect(numText("count", 2_500_000)).toBe("2.5M");
  });
});

describe("share", () => {
  it("has one decimal in tables and none in sentences", () => {
    expect(numText("share", 4.56)).toBe("4.6%");
    expect(numText("share", 36.2, { decimals: 0 })).toBe("36%");
  });
});

describe("amount", () => {
  it("formats coin quantities", () => {
    expect(numText("amount", 12.5)).toBe("12.5");
    expect(numText("amount", 3)).toBe("3");
    expect(numText("amount", 1_250_000)).toBe("1.25M");
    expect(numText("amount", 0.000123456)).toBe("0.0001235");
  });
});

describe("unknown values", () => {
  it("have no parts and no text, never dashes", () => {
    for (const kind of [
      "money",
      "pnl",
      "market",
      "price",
      "change",
      "count",
      "share",
      "amount",
    ] as const) {
      expect(numParts(kind, null)).toBeNull();
      expect(numParts(kind, undefined)).toBeNull();
      expect(numParts(kind, Number.NaN)).toBeNull();
      expect(numParts(kind, Number.POSITIVE_INFINITY)).toBeNull();
      expect(numText(kind, null)).toBe("");
    }
  });
});

describe("compactFigure", () => {
  it("has no currency sign", () => {
    expect(compactFigure(10_910_000)).toBe("10.91M");
  });
});

describe("changedChars", () => {
  it("marks only the digits that moved, aligned from the right", () => {
    expect(changedChars("$1,234.56", "$1,234.57")).toEqual([
      false,
      false,
      false,
      false,
      false,
      false,
      false,
      false,
      true,
    ]);
    expect(changedChars("$999.00", "$1,000.00").filter(Boolean).length).toBe(6);
  });

  it("marks everything when there was nothing before", () => {
    expect(changedChars("", "$1").every(Boolean)).toBe(true);
  });
});
