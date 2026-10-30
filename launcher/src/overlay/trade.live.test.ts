// Mainnet dry run of overlay selling, simulate only: LIVE=1 npx vitest run src/overlay/trade.live.test.ts
// Finds real open offers on rare.market's busiest collections and a current holder of an item each
// offer can take, builds buildAcceptOfferTransaction with that holder as the sender and reviews it
// with reviewTransaction (gRPC simulate). Same for buildListTransaction on a holder's unlisted item.
// Nothing is signed and nothing is executed.

import { describe, expect, it } from "vitest";
import type { ItemCard, Offer } from "../marketplace/api";
import { choosePolicy, sellerNetMist, type TransferPolicy } from "../marketplace/venues";
import { reviewTransaction, type Review } from "../wallet/review";
import { bestOffer, buildAcceptOfferTransaction, buildListTransaction, offerNetMist } from "./trade";

const get = async <T,>(p: string): Promise<T> => {
  const r = await fetch(`https://rare.market${p}`);
  if (!r.ok) throw new Error(`rare.market ${r.status} ${p}`);
  return (await r.json()) as T;
};
const norm = (id: string) => "0x" + id.replace(/^0x/, "").toLowerCase().padStart(64, "0");
const sui = (m: bigint) => `${Number(m) / 1e9} SUI`;
const SUI_RE = /::sui::SUI$/;

/** SUI the sender gains before gas (balance change plus the gas paid). */
function suiInBeforeGas(r: Review, sender: string): bigint {
  const s = r.simulation;
  const change = s.balanceChanges.filter((b) => norm(b.address) === norm(sender) && SUI_RE.test(b.coinType)).reduce((a, b) => a + BigInt(b.amount), 0n);
  const gas = BigInt(s.gas.computation) + BigInt(s.gas.storage) - BigInt(s.gas.rebate);
  return change + gas;
}

function log(r: Review) {
  console.log(`  simulate ok=${r.ok}${r.insights.reason ? ` reason="${r.insights.reason}"` : ""}${r.simulation.failure ? ` failure="${r.simulation.failure.message}"` : ""}`);
  for (const row of r.rows) console.log(`  ${row.label}: ${row.text}${row.sub ? ` (${row.sub})` : ""}`);
}

type Col = { slug: string };
type Holder = { address: string; inKiosk?: number; inWallet?: number; listed?: number };

const cols = (async () => (await get<{ data: Col[] }>("/v1/collections?window=1d&sort=volume&limit=30")).data)();
const itemCache = new Map<string, Promise<ItemCard[]>>();
const itemsOf = (slug: string) => {
  if (!itemCache.has(slug)) itemCache.set(slug, get<{ data: ItemCard[] }>(`/v1/collections/${slug}/items?limit=36`).then((r) => r.data).catch(() => []));
  return itemCache.get(slug)!;
};

/** Items the collection's top holders keep (in a kiosk or in the wallet), with their owner. */
const holderCache = new Map<string, Promise<ItemCard[]>>();
function holderItems(slug: string): Promise<ItemCard[]> {
  if (!holderCache.has(slug))
    holderCache.set(
      slug,
      (async () => {
        const hs = await get<{ data: Holder[] }>(`/v1/collections/${slug}/holders?limit=25`).then((r) => r.data).catch(() => [] as Holder[]);
        const pick = [...hs.slice(0, 3), ...hs.filter((x) => (x.inWallet ?? 0) > 0).slice(0, 3)];
        const out: ItemCard[] = [];
        for (const h of [...new Map(pick.map((x) => [x.address, x])).values()]) {
          const page = await get<{ data: ItemCard[] }>(`/v1/accounts/${h.address}/items?collection=${slug}&limit=36`).then((r) => r.data).catch(() => [] as ItemCard[]);
          for (const it of page) out.push({ ...it, owner: it.owner ?? h.address });
        }
        return out;
      })(),
    );
  return holderCache.get(slug)!;
}
const walletItems = async (slug: string) => (await holderItems(slug)).filter((it) => !it.kioskId && !it.listing);
const kioskItems = async (slug: string) => (await holderItems(slug)).filter((it) => it.kioskId && !it.listing);

const policyCache = new Map<string, Promise<TransferPolicy | null>>();
const policyOf = (type: string) => {
  if (!policyCache.has(type)) policyCache.set(type, get<{ data: TransferPolicy[] }>(`/v1/policies/${encodeURIComponent(type)}`).then((r) => choosePolicy(r.data)));
  return policyCache.get(type)!;
};

const WALLET_ONLY = new Set(["tradeport/escrow-bids"]);

/** Holder items an offer can take: its own item for item offers, any held item for collection bids. */
async function candidates(slug: string, o: Offer): Promise<ItemCard[]> {
  const ok = (it: ItemCard) => !!it.owner && norm(it.owner) !== norm(o.bidder) && (!it.listing || it.listing.venue === "kiosk") && (!WALLET_ONLY.has(o.source) || !it.kioskId);
  if (o.kind === "item" && o.item) {
    const it = await get<ItemCard>(`/v1/items/${o.item.id}`).catch(() => null);
    return it && ok(it) ? [it] : [];
  }
  if (WALLET_ONLY.has(o.source)) return (await walletItems(slug)).filter(ok).slice(0, 3);
  // A kiosk item, a wallet item and one listed natively in its owner's kiosk.
  const pools = [await kioskItems(slug), await walletItems(slug), (await itemsOf(slug)).filter((it) => it.listing?.venue === "kiosk")];
  return pools.flatMap((p) => p.filter(ok).slice(0, 1));
}

describe.skipIf(!process.env.LIVE)("overlay selling on mainnet (simulate only, never signed)", () => {
  it("accepts a real offer per venue source as a real holder; SUI in, the NFT out", async () => {
    const passed = new Map<string, string>();
    const failed: string[] = [];
    const tried = new Set<string>();
    for (const c of await cols) {
      const offers = await get<{ data: Offer[] }>(`/v1/offers?collection=${c.slug}&limit=100`).then((r) => r.data).catch(() => [] as Offer[]);
      // One offer per (source, kind) per collection, cheapest first; a kiosk, a wallet and a listed holder item each.
      for (const o of [...offers].reverse()) {
        const shape = `${o.source} ${o.kind}`;
        if (!o.fillable || o.status !== "open") continue;
        if (tried.has(`${c.slug} ${shape}`)) continue;
        tried.add(`${c.slug} ${shape}`);
        for (const item of await candidates(c.slug, o)) {
          const seller = item.owner!;
          const tag = `${shape} ${item.listing ? "listed natively" : item.kioskId ? "kiosk" : "wallet"}`;
          if (passed.has(tag)) continue;
          console.log(`\n== ${tag}: ${c.slug} ${item.name} ${item.id}\n  offer ${o.id} ${sui(BigInt(o.priceMist))}, seller ${seller}`);
          let r: Review;
          try {
            const tx = await buildAcceptOfferTransaction(item, o, seller, get);
            r = await reviewTransaction(tx, seller, { trade: null });
          } catch (e) {
            console.log(`  build failed: ${(e as Error).message}`);
            failed.push(`${tag} ${item.id}: ${(e as Error).message}`);
            continue;
          }
          log(r);
          if (!r.ok) {
            failed.push(`${tag} ${item.id}: ${r.insights.reason ?? r.simulation.failure?.message}`);
            continue;
          }
          const got = suiInBeforeGas(r, seller);
          const net = sellerNetMist(o, await policyOf(item.type));
          console.log(`  seller gets ${sui(got)} before gas, sellerNetMist ${sui(net)}, offerNetMist without policy ${sui(offerNetMist(o))}`);
          expect(got).toBe(net);
          expect(r.rows.some((x) => x.kind === "send" && x.objectId && norm(x.objectId) === norm(item.id))).toBe(true);
          passed.set(tag, `item ${item.id} offer ${o.id}${net !== BigInt(o.priceMist) ? ` (net ${sui(net)} of ${sui(BigInt(o.priceMist))})` : ""}`);
        }
      }
    }
    console.log(`\ncovered:\n${[...passed].map(([k, v]) => `  ${k}: ${v}`).join("\n")}`);
    console.log(`not ok:\n${failed.map((f) => `  ${f}`).join("\n") || "  none"}`);
    expect(passed.size).toBeGreaterThan(0);
    expect([...passed.keys()].some((k) => k.startsWith("tradeport/bids"))).toBe(true);
  }, 600_000);

  it("bestOffer picks a fillable offer for a held item", async () => {
    for (const c of (await cols).slice(0, 10)) {
      for (const it of (await itemsOf(c.slug)).slice(0, 6)) {
        const o = await bestOffer(it.id, get);
        if (!o) continue;
        console.log(`\n== bestOffer ${c.slug} ${it.name}: ${o.source} ${o.kind} ${sui(BigInt(o.priceMist))} net ${sui(offerNetMist(o))}`);
        expect(o.status).toBe("open");
        expect(o.fillable).toBe(true);
        expect(norm(o.bidder)).not.toBe(norm(it.owner ?? "0x0"));
        return;
      }
    }
    throw new Error("no held item with a fillable offer found");
  }, 120_000);

  it("lists a holder's unlisted items (kiosk and wallet); the item stays in their kiosk", async () => {
    const cases: Array<{ tag: string; item: ItemCard }> = [];
    for (const c of await cols) {
      if (cases.some((x) => x.tag === "kiosk") && cases.some((x) => x.tag === "wallet") && cases.some((x) => x.tag === "relist")) break;
      const items = await itemsOf(c.slug);
      const inKiosk = (await kioskItems(c.slug))[0];
      if (inKiosk && !cases.some((x) => x.tag === "kiosk")) cases.push({ tag: "kiosk", item: inKiosk });
      const relist = items.find((it) => it.listing?.venue === "kiosk" && it.owner);
      if (relist && !cases.some((x) => x.tag === "relist")) cases.push({ tag: "relist", item: relist });
      if (!cases.some((x) => x.tag === "wallet")) {
        const w = (await walletItems(c.slug))[0];
        if (w) cases.push({ tag: "wallet", item: w });
      }
    }
    expect(cases.length).toBeGreaterThan(0);
    const ok: string[] = [];
    for (const { tag, item } of cases) {
      const seller = item.owner!;
      const price = 123_000_000_000n;
      console.log(`\n== list ${tag}: ${item.name} ${item.id} (${item.type}) seller ${seller}`);
      const tx = await buildListTransaction(item, price, seller, get);
      const r = await reviewTransaction(tx, seller, { trade: null });
      log(r);
      expect(r.ok).toBe(true);
      // Listing moves no SUI beyond gas and never sends the item away.
      expect(r.rows.some((x) => x.kind === "send" && x.objectId && norm(x.objectId) === norm(item.id))).toBe(false);
      expect(suiInBeforeGas(r, seller)).toBe(0n);
      const fns = tx.getData().commands.flatMap((cmd) => (cmd.MoveCall ? [`${cmd.MoveCall.module}::${cmd.MoveCall.function}`] : []));
      console.log(`  calls ${fns.join(", ")}`);
      expect(fns).toContain("kiosk::list");
      ok.push(`${tag} ${item.id}`);
    }
    console.log(`\nlisted ok:\n${ok.map((x) => `  ${x}`).join("\n")}`);
  }, 300_000);

  it("lists a wallet item for a seller rare.market knows no kiosk for: the PTB makes one", async () => {
    let item: ItemCard | undefined;
    for (const c of await cols) if ((item = (await walletItems(c.slug))[0])) break;
    expect(item).toBeTruthy();
    const seller = item!.owner!;
    const noKiosks = async <T,>(p: string): Promise<T> => (p.endsWith("/kiosks") ? ({ data: [] } as T) : get<T>(p));
    const tx = await buildListTransaction(item!, 77_000_000_000n, seller, noKiosks);
    const fns = tx.getData().commands.flatMap((cmd) => (cmd.MoveCall ? [`${cmd.MoveCall.module}::${cmd.MoveCall.function}`] : []));
    console.log(`\n== list into a new kiosk: ${item!.name} ${item!.id} seller ${seller}\n  calls ${fns.join(", ")}`);
    const r = await reviewTransaction(tx, seller, { trade: null });
    log(r);
    expect(r.ok).toBe(true);
    expect(fns[0]).toMatch(/^(kiosk::new|personal_kiosk::new)$/);
    expect(fns).toContain("kiosk::list");
    expect(fns).toContain("transfer::public_share_object");
  }, 120_000);
});
