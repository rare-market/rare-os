import { beforeEach, describe, expect, it, vi } from "vitest";
import { canonType } from "../sui/chain";
import type { SuiGame } from "../lib/rare";
import { applyOp, applyPending, groupItems, isStaked, normalizeInventory, reflects, settle, summarize, unstakeLine } from "./logic";
import { loadInventory } from "./api";
import type { InvItem, Inventory, Pending } from "./types";

const T = { hero: "0xa1::hero::Hero", pop: "0xb2::popkin::Popkin", misc: "0xd4::art::Piece" };
const game = (slug: string, type: string): SuiGame => ({ slug, name: slug.toUpperCase(), studio: "", genre: "", platforms: [], website: "", x: "", description: "", logoUrl: "", bannerUrl: "", collections: [{ nftType: type, name: slug }], token: null, featured: false });
const GAMES = [game("pawtato", T.hero), game("popkins", T.pop)];
const mk = (id: string, type: string, floor: string | null = null, extra: Partial<InvItem> = {}): InvItem => ({ id, name: id, type, imageUrl: null, collection: null, floorMist: floor, ...extra }) as InvItem;

describe("home summary", () => {
  it("totals game items, ranks games by value, counts items with offers and leaves non-game collectibles out", () => {
    const inv = normalizeInventory({ data: groupItems([mk("1", T.pop, "5"), mk("2", T.misc, "100"), mk("3", T.hero, "7", { topOfferMist: "9" }), mk("4", T.hero, "3", { topOfferMist: "0" })], GAMES) });
    const sum = summarize(inv);
    expect(sum.items).toBe(3);
    expect(sum.valueMist).toBe("15");
    expect(sum.offers).toBe(1);
    expect(sum.games.map((g) => g.slug)).toEqual(["pawtato", "popkins"]);
  });
  it("is empty for no items", () => {
    expect(summarize({ groups: [], total: 0 })).toEqual({ items: 0, valueMist: "0", offers: 0, games: [] });
  });
});

describe("grouping", () => {
  it("groups by game in catalog order and puts the rest last", () => {
    const g = groupItems([mk("1", T.pop, "5"), mk("2", T.misc), mk("3", "0xA1::hero::Hero", "7"), mk("4", T.hero, "3")], GAMES);
    expect(g.map((x) => x.game?.slug ?? null)).toEqual(["pawtato", "popkins", null]);
    expect(g[0].items.map((x) => x.id)).toEqual(["3", "4"]);
    expect(g[0].floorValueMist).toBe("10");
    expect(g[2].floorValueMist).toBe("0");
  });
  it("fills floors from the collection floors when an item has none", () => {
    const g = groupItems([mk("1", T.hero)], GAMES, { [canonType(T.hero)]: "9" });
    expect(g[0].items[0].floorMist).toBe("9");
  });
  it("normalizes the API answer and drops empty groups", () => {
    const inv = normalizeInventory({ data: [{ game: null, items: [mk("1", T.misc, "2")], floorValueMist: "x" }, { game: null, items: [] }] });
    expect(inv.total).toBe(1);
    expect(inv.groups[0].floorValueMist).toBe("2");
    expect(normalizeInventory(null)).toEqual({ groups: [], total: 0 });
  });
});

const base = (): Inventory => ({ groups: groupItems([mk("1", T.hero, "4"), mk("2", T.hero, "4"), mk("3", T.pop, "50")], GAMES), total: 3 });

describe("optimistic operations", () => {
  it("removes sold items and empty groups, and updates totals", () => {
    const a = applyOp(base(), { kind: "remove", ids: ["3"], why: "sold" });
    expect(a.total).toBe(2);
    expect(a.groups.map((g) => g.game?.slug)).toEqual(["pawtato"]);
    expect(applyOp(a, { kind: "remove", ids: ["1"], why: "sent" }).groups[0].floorValueMist).toBe("4");
  });
  it("lists and delists", () => {
    const l = applyOp(base(), { kind: "list", id: "1", priceMist: "6" });
    expect(l.groups[0].items[0].listing?.priceMist).toBe("6");
    expect(applyOp(l, { kind: "delist", id: "1" }).groups[0].items[0].listing).toBeNull();
  });
  it("knows when a fetch reflects an operation", () => {
    const b = base();
    expect(reflects(b, { kind: "remove", ids: ["1"], why: "sold" })).toBe(false);
    expect(reflects(applyOp(b, { kind: "remove", ids: ["1"], why: "sold" }), { kind: "remove", ids: ["1"], why: "sold" })).toBe(true);
    expect(reflects(b, { kind: "list", id: "1", priceMist: "6" })).toBe(false);
    expect(reflects(applyOp(b, { kind: "list", id: "1", priceMist: "6" }), { kind: "list", id: "1", priceMist: "6" })).toBe(true);
    expect(reflects(b, { kind: "delist", id: "1" })).toBe(true);
  });
  it("settles caught-up and stale operations, keeps the rest", () => {
    const p = (key: string, op: Pending["op"], at: number): Pending => ({ key, op, at });
    const fetched = applyOp(base(), { kind: "remove", ids: ["1"], why: "sold" });
    const left = settle([p("a", { kind: "remove", ids: ["1"], why: "sold" }, 1000), p("b", { kind: "remove", ids: ["2"], why: "sent" }, 1000), p("c", { kind: "list", id: "3", priceMist: "9" }, 1000)], fetched, 2000, 90_000);
    expect(left.map((x) => x.key)).toEqual(["b", "c"]);
    expect(settle(left, fetched, 200_000)).toEqual([]);
    expect(applyPending(base(), left).total).toBe(2);
  });
});

describe("store: optimistic update and rollback", () => {
  beforeEach(async () => {
    vi.resetModules();
  });
  it("shows the change at once, keeps it on success and rolls back on failure", async () => {
    const s = await import("./store");
    s.useInventory.setState({ address: "0xme", base: base(), pending: [] });
    let during = -1;
    await s.runOptimistic({ kind: "remove", ids: ["1"], why: "sold" }, async () => {
      during = s.currentView().total;
    });
    expect(during).toBe(2);
    expect(s.currentView().total).toBe(2);
    s.stopFollowUps();
    await expect(
      s.runOptimistic({ kind: "remove", ids: ["2"], why: "sold" }, async () => {
        expect(s.currentView().total).toBe(1);
        throw new Error("rejected");
      }),
    ).rejects.toThrow("rejected");
    expect(s.currentView().total).toBe(2);
    expect(s.useInventory.getState().pending).toHaveLength(1);
  });
  it("a fetch that shows the sale ends the pending operation", async () => {
    const s = await import("./store");
    s.useInventory.setState({ address: "0xme", base: base(), pending: [] });
    await s.runOptimistic({ kind: "remove", ids: ["1"], why: "sold" }, async () => {});
    s.stopFollowUps();
    const after = applyOp(base(), { kind: "remove", ids: ["1"], why: "sold" });
    s.setInventorySource((async (p: string) => (/inventory/.test(p) ? { data: after.groups, total: after.total } : Promise.reject(new Error("rare.market 404")))) as never);
    await s.refreshInventory("0xme");
    expect(s.useInventory.getState().pending).toEqual([]);
    expect(s.currentView().total).toBe(2);
  });
  it("a sold notice triggers a refetch", async () => {
    vi.useFakeTimers();
    const s = await import("./store");
    let calls = 0;
    s.setInventorySource((async () => {
      calls++;
      return { data: [], total: 0 };
    }) as never);
    s.useInventory.setState({ address: "0xme" });
    s.onNotices([{ id: "9", kind: "sold" } as never], new Set());
    await vi.advanceTimersByTimeAsync(1500);
    expect(calls).toBe(1);
    vi.useRealTimers();
  });
});

describe("accept an offer from its notification", () => {
  const notice = (raw: string) => ({ id: "5", kind: "offer_received", addr: "0xme", title: "", body: null, imageUrl: null, amount: { raw, decimals: 9, symbol: "SUI", coinType: "", usd: null }, item: { id: "0x1", name: "Hero #1", imageUrl: null, collection: null }, action: { kind: "item", itemId: "0x1", slug: null }, digest: null, cpTs: null, createdAt: 0, sentAt: 0 });
  async function setup(quote: unknown, sell = vi.fn(async () => {})) {
    vi.resetModules();
    const toast = vi.fn();
    vi.doMock("../wallet/market", () => ({ currentSigningAccount: async () => ({ address: "0xme", kind: "mnemonic" }) }));
    vi.doMock("../lib/store", () => ({ useApp: { getState: () => ({ toast, setTab: vi.fn() }) } }));
    vi.doMock("./trade", () => ({ tradeApi: () => ({ quote: async () => quote, sell, list: vi.fn(), cancel: vi.fn() }) }));
    const s = await import("./store");
    s.setInventorySource((async () => ({ id: "0x1", name: "Hero #1", type: T.hero })) as never);
    s.useInventory.setState({ address: "0xme", base: { groups: groupItems([mk("0x1", T.hero, "4")], GAMES), total: 1 }, pending: [] });
    const n = await import("./notice");
    return { s, n, toast, sell };
  }
  const offer = (price: string) => ({ offer: { priceMist: price }, netMist: BigInt(price), blocked: null, sponsored: false });

  it("sells into the announced offer and removes the item at once", async () => {
    const { s, n, sell, toast } = await setup(offer("64000000000"));
    expect(await n.acceptFromNotice(notice("64000000000") as never)).toBe("sold");
    expect(sell).toHaveBeenCalled();
    expect(s.currentView().total).toBe(0);
    expect(toast).toHaveBeenCalledWith(expect.stringContaining("Sold Hero #1 for 64"));
    s.stopFollowUps();
  });
  it("signs nothing when a better offer is open now, and opens the item sheet", async () => {
    const { s, n, sell } = await setup(offer("70000000000"));
    expect(await n.acceptFromNotice(notice("64000000000") as never)).toBe("sheet");
    expect(sell).not.toHaveBeenCalled();
    expect(s.useInventory.getState().sheetId).toBe("0x1");
  });
  it("rolls the item back when the sale fails", async () => {
    const { s, n, toast } = await setup(offer("64000000000"), vi.fn(async () => { throw new Error("The network rejected this transaction."); }));
    expect(await n.acceptFromNotice(notice("64000000000") as never)).toBe("failed");
    expect(s.currentView().total).toBe(1);
    expect(toast).toHaveBeenCalledWith("The network rejected this transaction.");
  });
});

describe("staked items", () => {
  const stake = { protocol: "0x38f0::staking_contract", name: "staking_contract", via: "staked" as const };
  const pop = { slug: "popkins", name: "Popkins", imageUrl: null, verified: true };
  const staked = () => mk("s1", T.pop, "50", { staked: stake, collection: pop });

  it("keeps the API's staked marker and counts the item in the totals and the game value", () => {
    const inv = normalizeInventory({ data: groupItems([mk("1", T.pop, "50"), staked()], GAMES), total: 2 });
    expect(inv.total).toBe(2);
    expect(inv.groups[0].floorValueMist).toBe("100");
    expect(inv.groups[0].items[1].staked).toEqual(stake);
    expect(summarize(inv)).toMatchObject({ items: 2, valueMist: "100" });
    expect(isStaked(inv.groups[0].items[1])).toBe(true);
    expect(isStaked(inv.groups[0].items[0])).toBe(false);
  });
  it("says where to unstake, by project", () => {
    expect(unstakeLine(staked())).toBe("Unstake on Popkins to list or send");
    expect(unstakeLine(mk("x", T.pop))).toBe("Unstake on its project to list or send");
  });
  it("never lists a staked item, even optimistically", () => {
    const inv: Inventory = { groups: groupItems([staked()], GAMES), total: 1 };
    expect(applyOp(inv, { kind: "list", id: "s1", priceMist: "9" }).groups[0].items[0].listing ?? null).toBeNull();
  });
  it("the fallback path (items endpoint) carries staked items through", async () => {
    const calls: string[] = [];
    const get = vi.fn(async (path: string) => {
      calls.push(path);
      if (path.endsWith("/inventory")) throw new Error("rare.market 404");
      if (path.includes("/items?")) return { data: [staked()], nextCursor: null };
      return { stats: { floorMist: "50" } };
    }) as never;
    const inv = await loadInventory(get, "0xabc", async () => GAMES);
    expect(inv.total).toBe(1);
    expect(inv.groups[0].items[0].staked).toEqual(stake);
  });
});
