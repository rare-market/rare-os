import { describe, expect, it, vi } from "vitest";
import { balances, mergePortfolio, portfolio, type BalanceClient } from "./balances";
import { nameOf } from "./names";
import { nftDetail, walletNfts, type NftDeps } from "./nfts";
import { activityDetail, parseTx } from "./txdetail";
import { PriceClient } from "./prices";

const ME = "0x" + "a1".repeat(32);
const BOB = "0x" + "b2".repeat(32);
const SUI = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
const USDC = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
const DMC = "0x4c981f3ff786cdb9e514da897ab8a953647dae2ace9679e8358eec1e3e8871ac::dmc::DMC";
const JUNK = "0x" + "99".repeat(32) + "::junk::JUNK";

const client: BalanceClient = {
  core: {
    listBalances: async () => ({
      balances: [
        { coinType: DMC, balance: "188000000000" },
        { coinType: USDC, balance: "2500000" },
        { coinType: SUI, balance: "57497388949" },
        { coinType: JUNK, balance: "5000000000000" },
        { coinType: "0x" + "88".repeat(32) + "::zero::ZERO", balance: "0" },
      ],
      hasNextPage: false,
      cursor: null,
    }),
    getCoinMetadata: async ({ coinType }) => {
      if (coinType === USDC) return { coinMetadata: { decimals: 6, symbol: "USDC", name: "USDC", iconUrl: "https://usdc.png" } };
      if (coinType === SUI) return { coinMetadata: { decimals: 9, symbol: "SUI", name: "Sui" } };
      if (coinType === JUNK) throw new Error("no metadata");
      return { coinMetadata: { decimals: 9, symbol: "DMC", name: "DeLorean" } };
    },
  },
};

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
const tradeBody = {
  totalUsd: 55,
  change24hUsd: -0.8,
  holdings: [
    { coinType: "0x2::sui::SUI", symbol: "SUI", decimals: 9, logo: "https://sui.png", priceUsd: 1, change24h: -2, verified: true, amount: 57.49, valueUsd: 57.49 },
    { coinType: USDC, symbol: "USDC", decimals: 6, priceUsd: 1, change24h: 0, verified: true, amount: 2.5, valueUsd: 2.5 },
    { coinType: DMC, symbol: "DMC", decimals: 9, logo: "/logos/dmc.png", priceUsd: 0.0003, change24h: 5, verified: false, amount: 188, valueUsd: 0.0564 },
  ],
};

describe("balances and portfolio", () => {
  it("balances: gRPC amounts, SUI first, zero dropped, metadata fallback", async () => {
    const list = await balances(ME, client);
    expect(list.map((b) => [b.symbol, b.amount])).toEqual([
      ["SUI", 57.497388949],
      ["JUNK", 5000],
      ["DMC", 188],
      ["USDC", 2.5],
    ]);
  });

  it("portfolio: trade prices, values and 24h change; sorted SUI first then by value", async () => {
    const trade = new PriceClient({ baseUrl: "https://y.test", fetch: vi.fn(async () => json(tradeBody)) as unknown as typeof fetch });
    const p = await portfolio(ME, { client, trade });
    expect(p.priced).toBe(true);
    expect(p.tokens.map((t) => t.symbol)).toEqual(["SUI", "USDC", "DMC", "JUNK"]);
    const sui = p.tokens[0];
    expect(sui).toMatchObject({ priceUsd: 1, change24h: -2, verified: true, iconUrl: "https://sui.png" });
    expect(sui.valueUsd).toBeCloseTo(57.497388949);
    expect(p.tokens[2].iconUrl).toBe("https://y.test/logos/dmc.png");
    expect(p.tokens[3]).toMatchObject({ priceUsd: null, valueUsd: null, verified: false });
    expect(p.totalUsd).toBeCloseTo(57.497388949 + 2.5 + 188 * 0.0003);
    // SUI fell 2%: 57.497 - 57.497 / 0.98; DMC rose 5%
    const exp = 57.497388949 - 57.497388949 / 0.98 + (188 * 0.0003 - (188 * 0.0003) / 1.05);
    expect(p.change24hUsd).toBeCloseTo(exp);
  });

  it("portfolio still works when trade is down: no prices, same tokens", async () => {
    const trade = new PriceClient({ baseUrl: "https://y.test", fetch: vi.fn(async () => json({ error: "x" }, 502)) as unknown as typeof fetch });
    const p = await portfolio(ME, { client, trade });
    expect(p).toMatchObject({ priced: false, totalUsd: null, change24hUsd: null });
    expect(p.tokens.map((t) => t.symbol)).toEqual(["SUI", "JUNK", "DMC", "USDC"]);
  });

  it("mergePortfolio with no balances", () => {
    expect(mergePortfolio([], null)).toEqual({ totalUsd: null, change24hUsd: null, tokens: [], priced: false });
  });
});

describe("names", () => {
  it("reverse lookup returns the default name", async () => {
    const c = { resolveNameServiceAddress: vi.fn(), defaultNameServiceName: vi.fn(async () => ({ data: { name: "bob.sui" } })) };
    expect(await nameOf(BOB, c)).toBe("bob.sui");
    expect(await nameOf(BOB, c)).toBe("bob.sui");
    expect(c.defaultNameServiceName).toHaveBeenCalledTimes(1);
  });
});

describe("nfts", () => {
  const NFT = "0x" + "22".repeat(32);
  const NFT_T = "0x" + "e5".repeat(32) + "::popkins::Popkins";
  const deps = (o: Partial<NftDeps> & { owner?: unknown } = {}): NftDeps => ({
    rareGet: async <T,>() => ({ id: NFT, name: "Popkins #1", type: NFT_T, imageUrl: "https://img/1.png", collection: { slug: "popkins", name: "Popkins", imageUrl: null, verified: true }, kioskId: null }) as T,
    getObject: async () => ({ object: { objectId: NFT, type: NFT_T, owner: o.owner ?? { $kind: "AddressOwner", AddressOwner: ME } } }),
    hasStore: async () => true,
    ...o,
  });

  it("walletNfts reads the account's items from rare.market", async () => {
    const rareGet = vi.fn(async () => ({ data: [], nextCursor: null }));
    await walletNfts(ME, { deps: { rareGet: rareGet as never } });
    expect(rareGet).toHaveBeenCalledWith(`/v1/accounts/${ME}/items?limit=60`);
  });

  it("an owned NFT with store can be sent", async () => {
    const d = await nftDetail(NFT, ME, deps());
    expect(d).toMatchObject({ name: "Popkins #1", type: NFT_T, owner: { kind: "address", address: ME }, hasStore: true, canSend: true, inKiosk: false });
  });

  it("no store ability: can't send this one", async () => {
    const d = await nftDetail(NFT, ME, deps({ hasStore: async () => false }));
    expect(d.canSend).toBe(false);
    expect(d.cantSend).toMatch(/^Can't send this one/);
  });

  it("kiosk items are sendable; someone else's are not", async () => {
    const FIELD = "0x" + "77".repeat(32);
    const KIOSK = "0x" + "66".repeat(32);
    // kiosk found from the chain: NFT -> dynamic field -> kiosk, even when rare.market has no kioskId
    const kiosk = deps({
      rareGet: async <T,>() => ({ id: NFT, name: "P", type: NFT_T, imageUrl: null, collection: null, kioskId: null }) as T,
      getObject: async ({ objectId }) =>
        objectId === FIELD
          ? { object: { objectId, type: "0x2::dynamic_field::Field<0x2::dynamic_object_field::Wrapper<0x2::kiosk::Item>, 0x2::object::ID>", owner: { $kind: "ObjectOwner", ObjectOwner: KIOSK } } }
          : { object: { objectId, type: NFT_T, owner: { $kind: "ObjectOwner", ObjectOwner: FIELD } } },
    });
    expect(await nftDetail(NFT, ME, kiosk)).toMatchObject({ inKiosk: true, kioskId: KIOSK, canSend: true });
    // wrapped in something that is not a kiosk
    const wrapped = deps({ getObject: async ({ objectId }) => ({ object: { objectId, type: objectId === FIELD ? "0x9::vault::Vault" : NFT_T, owner: { $kind: "ObjectOwner", ObjectOwner: FIELD } } }) });
    expect(await nftDetail(NFT, ME, wrapped)).toMatchObject({ inKiosk: false, canSend: false, cantSend: "It is held inside another object" });
    expect(await nftDetail(NFT, BOB, deps())).toMatchObject({ canSend: false, cantSend: "It is not in this wallet" });
  });

  it("falls back to chain display when rare.market has nothing", async () => {
    const d = await nftDetail(
      NFT,
      ME,
      deps({
        rareGet: async () => Promise.reject(new Error("404")),
        getObject: async () => ({ object: { objectId: NFT, type: NFT_T, owner: { $kind: "AddressOwner", AddressOwner: ME }, display: { output: { name: "Chain name", image_url: "https://c/1.png" } } } }),
      }),
    );
    expect(d).toMatchObject({ name: "Chain name", imageUrl: "https://c/1.png", rareMissing: true, canSend: true });
  });
});

describe("activity detail", () => {
  const tx = {
    digest: "D1",
    status: { success: true, error: null },
    timestampMs: 1700000000000,
    transaction: { sender: ME, commands: [{ $kind: "TransferObjects", TransferObjects: {} }] },
    balanceChanges: [
      { coinType: SUI, address: ME, amount: String(-1_000_000_000 - 1_500_000) },
      { coinType: SUI, address: BOB, amount: "1000000000" },
    ],
    objectTypes: {},
    effects: {
      gasUsed: { computationCost: "1000000", storageCost: "2000000", storageRebate: "1500000" },
      gasObject: { objectId: "0x" + "11".repeat(32), inputOwner: { $kind: "AddressOwner", AddressOwner: ME } },
      changedObjects: [],
    },
  };

  it("parses a SUI send for this wallet", () => {
    const d = parseTx(tx as never, "D1", ME);
    expect(d).toMatchObject({ status: "success", flows: [{ coinType: SUI, amount: "-1000000000" }], feeMist: "1500000", feePaid: true, counterparty: BOB });
  });

  it("returns null when the node pruned it", async () => {
    const client = { getTransaction: async () => Promise.reject(Object.assign(new Error("Transaction not found"), { code: "NOT_FOUND" })) };
    expect(await activityDetail("D1", ME, client as never)).toBeNull();
  });
});
