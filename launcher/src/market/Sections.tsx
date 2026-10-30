import { Glyph, Tabs } from "../ui";
import { useTradeNav } from "../trade/nav";
import { currentSection, MARKET_SECTIONS, useMarketArea, type MarketSection } from "./area";

/** The Market tab's sections, on the front page of both areas. */
export function MarketSections() {
  const area = useMarketArea((s) => s.area);
  const trade = useTradeNav((s) => s.section);
  return (
    <div className="flex h-[2.25rem] items-stretch gap-[1rem]">
      <span className="flex items-center">
        <Glyph b="lt" size="sm" />
      </span>
      <Tabs<MarketSection> items={MARKET_SECTIONS} value={currentSection(area, trade)} />
      <span className="flex items-center">
        <Glyph b="rt" size="sm" />
      </span>
    </div>
  );
}
