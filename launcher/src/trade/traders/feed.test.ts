import { beforeEach, describe, expect, it } from "vitest";
import type { TraderTrade, TradesPage } from "./api";
import { freshTrades, newestTime, onFollowedTrade, emitFollowedTrade, pollOnce, resetFeed, setFollowToasts, tradeSentence } from "./feed";

const A = `0x${"a".repeat(64)}`;
const t = (id: string, timeMs: number, buy = true): TraderTrade => ({ id, digest: id, timeMs, buy, usd: 120, amount: 10, priceUsd: 12, trader: A, coin: "0x2::sui::SUI" });

describe("freshTrades", () => {
  it("first read only sets the mark", () => {
    expect(freshTrades([t("a", 5)], undefined)).toEqual([]);
    expect(newestTime([t("a", 5), t("b", 9)], undefined)).toBe(9);
  });
  it("returns newer trades oldest first, never before the follow", () => {
    const items = [t("c", 30), t("b", 20), t("a", 10)];
    expect(freshTrades(items, 10).map((x) => x.id)).toEqual(["b", "c"]);
    expect(freshTrades(items, 10, 25).map((x) => x.id)).toEqual(["c"]);
    expect(newestTime([], 42)).toBe(42);
  });
});

describe("pollOnce", () => {
  beforeEach(() => resetFeed());
  it("reports a followed trader's new trades on the next read", async () => {
    let page: TradesPage = { items: [t("a", 10)], nextBefore: null, tokens: {} };
    const get = async <T,>(path: string) => {
      expect(path).toBe(`/users/${A}/trades?limit=10`);
      return page as T;
    };
    const list = [{ address: A, name: "@whale", since: 0 }];
    expect(await pollOnce(list, get)).toEqual([]);
    page = { items: [t("b", 20, false), t("a", 10)], nextBefore: null, tokens: { "0x2::sui::SUI": { coinType: "0x2::sui::SUI", symbol: "SUI", name: "Sui", logo: null, priceUsd: 1 } } };
    const out = await pollOnce(list, get);
    expect(out.map((e) => [e.trade.id, e.token?.symbol, e.trader.name])).toEqual([["b", "SUI", "@whale"]]);
    expect(tradeSentence(out[0])).toBe("@whale sold $120.00 of SUI");
    expect(await pollOnce(list, get)).toEqual([]);
  });
  it("a failing read is skipped", async () => {
    const get = async <T,>(): Promise<T> => {
      throw new Error("down");
    };
    expect(await pollOnce([{ address: A, name: "x", since: 0 }], get)).toEqual([]);
  });
});

describe("events", () => {
  it("subscribers get every followed trade and can unsubscribe", () => {
    setFollowToasts(false);
    const got: string[] = [];
    const off = onFollowedTrade((e) => got.push(e.trade.id));
    emitFollowedTrade({ trader: { address: A, name: "x" }, trade: t("z", 1), token: null });
    off();
    emitFollowedTrade({ trader: { address: A, name: "x" }, trade: t("y", 2), token: null });
    expect(got).toEqual(["z"]);
  });
});
