// The Market tab holds two page stacks: Collections (the marketplace) and Trade (coins, perps, traders).
// The sections along the top of each front page (LT / RT) move between them.

import { create } from "zustand";
import { useTradeNav } from "../trade/nav";

export type Area = "collections" | "trade";
export type MarketSection = "collections" | "coins" | "perps" | "traders";

export const MARKET_SECTIONS: Array<{ value: MarketSection; label: string }> = [
  { value: "collections", label: "Collections" },
  { value: "coins", label: "Coins" },
  { value: "perps", label: "Perps" },
  { value: "traders", label: "Traders" },
];

export const useMarketArea = create<{ area: Area }>(() => ({ area: "collections" }));

export function setArea(area: Area) {
  if (useMarketArea.getState().area !== area) useMarketArea.setState({ area });
}

/** Which of the four sections is showing. Trade's Positions page counts as Perps. */
export function currentSection(area: Area, trade: string): MarketSection {
  if (area === "collections") return "collections";
  return trade === "perps" || trade === "positions" ? "perps" : trade === "traders" ? "traders" : "coins";
}

export function goSection(s: MarketSection) {
  if (s === "collections") return setArea("collections");
  setArea("trade");
  useTradeNav.getState().setSection(s === "coins" ? "home" : s);
}

/** LT / RT from a front page: the next or previous section. */
export function stepSection(dir: 1 | -1) {
  const cur = currentSection(useMarketArea.getState().area, useTradeNav.getState().section);
  const i = MARKET_SECTIONS.findIndex((x) => x.value === cur);
  goSection(MARKET_SECTIONS[(i + dir + MARKET_SECTIONS.length) % MARKET_SECTIONS.length].value);
}
