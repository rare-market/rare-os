import { describe, expect, it, vi } from "vitest";
import { pastedType, searchCoins, searchPath, type SearchDeps } from "./search";

vi.mock("../lib/bridge", () => ({ isTauri: false }));

const TYPE = `0x${"ab".repeat(32)}::meme::MEME`;
const row = (coinType: string, symbol = "X") => ({ coinType, symbol, name: symbol, decimals: 9, logo: null, priceUsd: 1 });

describe("coin search", () => {
  it("asks trade's search with the text", async () => {
    const get = vi.fn(async () => ({ items: [row("0x2::sui::SUI", "SUI")] }));
    const r = await searchCoins("sui", { get, metadata: async () => null } as unknown as SearchDeps);
    expect(get).toHaveBeenCalledWith(searchPath("sui"));
    expect(r.map((t) => t.symbol)).toEqual(["SUI"]);
  });
  it("opens a pasted type that is not indexed from the chain's metadata", async () => {
    const get = vi.fn(async (p: string) => {
      if (p.startsWith("/search")) return { items: [] };
      throw new Error("404");
    });
    const metadata = vi.fn(async () => ({ symbol: "MEME", name: "Meme Coin", decimals: 6, iconUrl: "https://x/y.png" }));
    const r = await searchCoins(` ${TYPE} `, { get, metadata } as unknown as SearchDeps);
    expect(r).toHaveLength(1);
    expect(r[0]).toMatchObject({ coinType: TYPE, symbol: "MEME", decimals: 6, logo: "https://x/y.png", priceUsd: null });
  });
  it("prefers trade's lean answer to the chain", async () => {
    const get = vi.fn(async (p: string) => (p.startsWith("/search") ? { items: [] } : row(TYPE, "MEME")));
    const metadata = vi.fn();
    const r = await searchCoins(TYPE, { get, metadata } as unknown as SearchDeps);
    expect(r[0].symbol).toBe("MEME");
    expect(metadata).not.toHaveBeenCalled();
  });
  it("a failing search still resolves a pasted type", async () => {
    const get = vi.fn(async () => {
      throw new Error("down");
    });
    const metadata = async () => ({ symbol: "M", name: "M", decimals: 9, iconUrl: null });
    expect((await searchCoins(TYPE, { get, metadata } as unknown as SearchDeps))[0].symbol).toBe("M");
  });
  it("tells types from words", () => {
    expect(pastedType("0x2::sui::SUI")).toBe(`0x${"0".repeat(63)}2::sui::SUI`);
    expect(pastedType("sui")).toBeNull();
  });
});
