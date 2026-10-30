import { beforeEach, describe, expect, it } from "vitest";
import { TABS } from "../lib/store";
import { useTradeNav } from "../trade/nav";
import { currentSection, goSection, MARKET_SECTIONS, stepSection, useMarketArea } from "./area";

describe("top tabs", () => {
  it("are the five: Home, Library, Sui, Market, Wallet", () => {
    expect(TABS.map((t) => t.label)).toEqual(["Home", "Library", "Sui", "Market", "Wallet"]);
    expect(TABS.map((t) => t.id)).toEqual(["home", "library", "games", "market", "wallet"]);
  });
  it("leave out Trade, Friends and Settings", () => {
    const ids = TABS.map((t) => t.id as string);
    for (const gone of ["trade", "friends", "settings", "store"]) expect(ids).not.toContain(gone);
  });
  it("wrap with LB / RB in at most two presses to any tab", () => {
    for (let from = 0; from < TABS.length; from++) for (let to = 0; to < TABS.length; to++) {
      const d = (to - from + TABS.length) % TABS.length;
      expect(Math.min(d, TABS.length - d)).toBeLessThanOrEqual(2);
    }
  });
});

describe("market sections", () => {
  beforeEach(() => {
    useMarketArea.setState({ area: "collections" });
    useTradeNav.getState().setSection("home");
  });
  it("are Collections, Coins, Perps, Traders", () => {
    expect(MARKET_SECTIONS.map((s) => s.label)).toEqual(["Collections", "Coins", "Perps", "Traders"]);
  });
  it("map to the marketplace and Trade's own sections", () => {
    goSection("perps");
    expect(useMarketArea.getState().area).toBe("trade");
    expect(useTradeNav.getState().section).toBe("perps");
    goSection("coins");
    expect(useTradeNav.getState().section).toBe("home");
    goSection("collections");
    expect(useMarketArea.getState().area).toBe("collections");
  });
  it("count Positions as Perps", () => {
    expect(currentSection("trade", "positions")).toBe("perps");
  });
  it("LT / RT step through all four and wrap", () => {
    const seen: string[] = [];
    for (let i = 0; i < 4; i++) {
      stepSection(1);
      seen.push(currentSection(useMarketArea.getState().area, useTradeNav.getState().section));
    }
    expect(seen).toEqual(["coins", "perps", "traders", "collections"]);
    stepSection(-1);
    expect(currentSection(useMarketArea.getState().area, useTradeNav.getState().section)).toBe("traders");
  });
});
