// Mainnet dry run of in-app buying, simulate only: LIVE=1 npx vitest run src/marketplace/venues/live.test.ts
// Picks real listings under 50 SUI from rare.market (one per common source), builds each purchase
// with buildBuyTransaction for a funded address and reviews it with reviewTransaction (gRPC
// simulate). Nothing is signed and nothing is executed.

import { describe, expect, it } from "vitest";
import { reviewTransaction } from "../../wallet/review";
import type { ItemCard, Listing } from "../api";
import { buildBuyTransaction } from "../buy";

const BUYER = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const MAX = 50_000_000_000n;
const WANT = ["tradeport/simple", "kiosk/native", "tradeport/kiosk"];

const get = async <T,>(p: string): Promise<T> => {
  const r = await fetch(`https://rare.market${p}`);
  if (!r.ok) throw new Error(`rare.market ${r.status} ${p}`);
  return (await r.json()) as T;
};
const norm = (id: string) => "0x" + id.replace(/^0x/, "").toLowerCase().padStart(64, "0");

/** The cheapest listing under 50 SUI for each wanted source, from the busiest collections. */
async function pickListings(): Promise<Map<string, ItemCard & { listing: Listing & { source: string } }>> {
  const picks = new Map<string, ItemCard & { listing: Listing & { source: string } }>();
  const cols = await get<{ data: Array<{ slug: string }> }>("/v1/collections?window=1d&sort=volume&limit=30");
  for (const c of cols.data) {
    if (WANT.every((s) => picks.has(s))) break;
    const page = await get<{ data: ItemCard[] }>(`/v1/collections/${c.slug}/items?sort=price_asc&limit=36`).catch(() => ({ data: [] as ItemCard[] }));
    for (const it of page.data) {
      const l = it.listing as (Listing & { source: string }) | null | undefined;
      if (!l || !WANT.includes(l.source) || picks.has(l.source)) continue;
      if (BigInt(l.allInMist ?? l.priceMist) > MAX) continue;
      picks.set(l.source, { ...it, listing: l });
    }
  }
  return picks;
}

describe.skipIf(!process.env.LIVE)("buy on mainnet (simulate only, never signed)", () => {
  it("builds and simulates a purchase per venue source; SUI out, the NFT in", async () => {
    const picks = await pickListings();
    console.log(`buyer ${BUYER}`);
    expect(picks.size).toBeGreaterThan(0);
    for (const [source, item] of picks) {
      const l = item.listing;
      console.log(`\n== ${source}: ${item.name} ${item.id}\n  listed ${Number(l.priceMist) / 1e9} SUI, all-in ${Number(l.allInMist ?? l.priceMist) / 1e9} SUI`);
      const tx = await buildBuyTransaction(item, l, BUYER, get);
      const r = await reviewTransaction(tx, BUYER, { trade: null });
      console.log(`  simulate ok=${r.ok}${r.insights.reason ? ` reason="${r.insights.reason}"` : ""} bytes=${r.bytes?.length ?? "none"}`);
      for (const row of r.rows) console.log(`  ${row.label}: ${row.text}${row.sub ? ` (${row.sub})` : ""}`);
      for (const k of r.risks) console.log(`  risk${k.danger ? " [danger]" : ""}: ${k.text}`);
      expect(r.ok).toBe(true);
      const out = r.rows.find((x) => x.kind === "send" && x.coinType && /::sui::SUI$/.test(x.coinType));
      expect(out?.text).toMatch(/ SUI$/);
      // what leaves is about the all-in price (the rest of the change is the fee row)
      expect(out!.amount!).toBeGreaterThanOrEqual(BigInt(l.priceMist));
      expect(r.rows.some((x) => x.kind === "receive" && x.objectId && norm(x.objectId) === norm(item.id))).toBe(true);
    }
  }, 180_000);

  it("a buyer with no kiosk yet: the PTB makes one, the NFT still shows as received", async () => {
    const picks = await pickListings();
    const item = picks.get("kiosk/native") ?? picks.get("tradeport/kiosk") ?? [...picks.values()][0];
    // Pretend rare.market lists no kiosks for the buyer, so buildBuy creates and shares one.
    const noKiosks = async <T,>(p: string): Promise<T> => (p.endsWith("/kiosks") ? ({ data: [] } as T) : get<T>(p));
    const tx = await buildBuyTransaction(item, item.listing, BUYER, noKiosks);
    const r = await reviewTransaction(tx, BUYER, { trade: null });
    console.log(`\n== new kiosk, ${item.listing.source}: ${item.name}\n  simulate ok=${r.ok}${r.insights.reason ? ` reason="${r.insights.reason}"` : ""}`);
    for (const row of r.rows) console.log(`  ${row.label}: ${row.text}${row.sub ? ` (${row.sub})` : ""}`);
    for (const k of r.risks) console.log(`  risk${k.danger ? " [danger]" : ""}: ${k.text}`);
    expect(r.ok).toBe(true);
    expect(r.rows.some((x) => x.kind === "send" && x.coinType && /::sui::SUI$/.test(x.coinType))).toBe(true);
  }, 120_000);
});
