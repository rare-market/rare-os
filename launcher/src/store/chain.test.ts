import { bcs } from "@mysten/sui/bcs";
import { describe, expect, it } from "vitest";
import { currentPriceUsd, GameBcs, parseGame, releaseFor, type StoreChainReader } from "./chain";

const A = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;
const rel = (version: number, edition: number) => ({
  version,
  label: `1.${version}`,
  edition,
  builds: [{ platform: "linux-x86_64", sha256: Array.from({ length: 32 }, (_, i) => i), size: 1234, path: `games/g/${version}/linux-x86_64.tar.zst` }],
  published_at_ms: 99,
});

function gameBytes() {
  const head = GameBcs.serialize({
    id: A(1),
    version: 1,
    studio: A(2),
    slug: "driftline",
    name: "Driftline",
    image_url: "",
    metadata_url: "https://rare.market/v1/store/meta/driftline.json",
    price_usd: 14_990_000,
    sale_price_usd: 0,
    sale_ends_ms: 0,
    accept_sui: true,
    listed: true,
    blocked: false,
    launch_check: true,
    edition: 2,
    latest: rel(5, 2),
    editions: { id: A(3), size: 2 },
    resale: { enabled: true, royalty_bps: 1000, min_hold_ms: 604800000, min_royalty_mist: 100000000, royalty_to: A(4) },
  }).toBytes();
  // The rest of the Game (sales, escrow, balances) follows; the reader ignores it.
  return new Uint8Array([...head, ...new Uint8Array(80).fill(7)]);
}

describe("Game object", () => {
  it("parses the fields the launcher reads", () => {
    const g = parseGame(gameBytes());
    expect(g.slug).toBe("driftline");
    expect(g.priceUsd).toBe(14_990_000n);
    expect(g.launchCheck).toBe(true);
    expect(g.edition).toBe(2);
    expect(g.latest?.version).toBe(5);
    expect(g.latest?.builds[0].sha256).toBe(Array.from({ length: 32 }, (_, i) => i.toString(16).padStart(2, "0")).join(""));
    expect(g.latest?.builds[0].size).toBe(1234n);
    expect(g.editionsTable).toBe(A(3));
    expect(g.resale.royaltyBps).toBe(1000);
  });

  it("reads the release of the licence's edition from the editions table, not latest", async () => {
    const g = parseGame(gameBytes());
    const asked: string[] = [];
    const client: StoreChainReader = {
      getObject: async () => ({ object: { content: gameBytes(), type: "x" } }),
      getDynamicField: async ({ parentId, name }) => {
        asked.push(`${parentId}:${bcs.u64().parse(name.bcs)}`);
        const edition = Number(bcs.u64().parse(name.bcs));
        if (edition !== 1) throw new Error("Dynamic field not found");
        const R = bcs.struct("Release", {
          version: bcs.u64(),
          label: bcs.string(),
          edition: bcs.u64(),
          builds: bcs.vector(bcs.struct("Build", { platform: bcs.string(), sha256: bcs.vector(bcs.u8()), size: bcs.u64(), path: bcs.string() })),
          published_at_ms: bcs.u64(),
        });
        return { dynamicField: { value: { bcs: R.serialize(rel(3, 1)).toBytes() } } };
      },
    };
    const r = await releaseFor(g, 1, client);
    expect(r?.version).toBe(3);
    expect(r?.edition).toBe(1);
    expect(asked).toEqual([`${A(3)}:1`]);
    expect(await releaseFor(g, 9, client)).toBeNull();
  });

  it("charges the sale price only while the sale runs", () => {
    expect(currentPriceUsd({ priceUsd: 10n, salePriceUsd: 7n, saleEndsMs: 100 }, 99)).toBe(7n);
    expect(currentPriceUsd({ priceUsd: 10n, salePriceUsd: 7n, saleEndsMs: 100 }, 100)).toBe(10n);
    expect(currentPriceUsd({ priceUsd: 10n, salePriceUsd: 0n, saleEndsMs: 100 }, 1)).toBe(10n);
  });
});
