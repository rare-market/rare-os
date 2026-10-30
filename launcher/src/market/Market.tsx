// The Market tab: the marketplace (Collections) and Trade (Coins, Perps, Traders) as sections of one tab.
import { useMarketArea } from "./area";
import { Marketplace } from "../marketplace/Marketplace";
import { Trade } from "../trade/Trade";

export function Market() {
  const area = useMarketArea((s) => s.area);
  return area === "trade" ? <Trade /> : <Marketplace />;
}
