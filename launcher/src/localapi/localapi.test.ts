import { describe, expect, it } from "vitest";
import { normalizeSuiAddress } from "@mysten/sui/utils";
import type { SuiGame } from "../lib/rare";
import type { Notice } from "../notify";
import { ApiFail, inventory, me, owns, prepareBuy, prepareClaim, prepareSell, type Deps } from "./methods";
import { gameNotice, gamesFor } from "./notices";
import { b64any, buildItemClaim, buildStateClaim, decodeVoucher, readClass, readItem, rejectReason, splitGeneric, VoucherError } from "./voucher";

const A = (b: string) => "0x" + b.repeat(64 / b.length);
const PLAYER = A("09");
const HERO = "0x0000000000000000000000000000000000000000000000000000000000000a11::act::Hero";
const OTHER = "0x00000000000000000000000000000000000000000000000000000000000000b2::other::Thing";
const MINE = A("1a");
const ELSEWHERE = A("2b");

const games: SuiGame[] = [
  { slug: "act", name: "ACT", studio: "", genre: "", platforms: ["web"], website: "", x: "", description: "", logoUrl: "", bannerUrl: "", collections: [{ nftType: HERO, name: "Heroes" }], token: null, featured: false },
];

const card = (id: string, type: string, extra: Record<string, unknown> = {}) => ({ id, name: "Hero #1", type, imageUrl: null, collection: { slug: "act-heroes", name: "Heroes" }, ...extra });

function deps(o: Partial<Deps> & { routes?: Record<string, unknown> } = {}): Deps {
  const routes: Record<string, unknown> = {
    [`/v1/accounts/${encodeURIComponent(PLAYER)}/items?collection=${encodeURIComponent(HERO)}&limit=60`]: { data: [card(MINE, HERO)], nextCursor: null },
    [`/v1/collections/${encodeURIComponent(HERO)}`]: { slug: "act-heroes", name: "Heroes", stats: { floorMist: "2000000000", topOfferMist: "1500000000" } },
    [`/v1/items/${MINE}`]: card(MINE, HERO, { owner: PLAYER }),
    [`/v1/items/${ELSEWHERE}`]: card(ELSEWHERE, OTHER, { listing: { venue: "tradeport", priceMist: "100", allInMist: "103" } }),
    [`/v1/collections/act-heroes/items?sort=price_asc&limit=24`]: {
      data: [card(A("3c"), HERO, { listing: { venue: "kiosk", priceMist: "900", allInMist: "950", seller: A("77") } }), card(A("4d"), HERO, { listing: { venue: "kiosk", priceMist: "500", allInMist: "520", seller: PLAYER } })],
    },
    ...o.routes,
  };
  return {
    get: async <T,>(path: string) => {
      if (!(path in routes)) throw new Error(`rare.market 404 ${path}`);
      return routes[path] as T;
    },
    account: async () => ({ address: PLAYER, kind: "native" }),
    canSign: (a) => !!a && a.kind !== "watch",
    games: async () => games,
    library: () => null,
    username: async () => "ghost",
    object: async () => null,
    ...o,
  };
}

describe("me and inventory are this game's", () => {
  it("me names the account and the game", async () => {
    expect(await me("web:act", deps())).toEqual({ address: PLAYER, username: "ghost", network: "mainnet", canSign: true, game: { id: "web:act", slug: "act", title: "ACT" } });
  });
  it("me without an account is an error the SDK can branch on", async () => {
    await expect(me("web:act", deps({ account: async () => null }))).rejects.toMatchObject({ code: "no_account" });
  });
  it("inventory lists only the game's collections", async () => {
    const r = await inventory("web:act", deps());
    expect(r.items.map((i) => i.id)).toEqual([MINE]);
    expect(r.items[0].floorMist).toBe("2000000000");
  });
  it("a game with no collections has no items", async () => {
    expect(await inventory("steam:1", deps())).toEqual({ items: [] });
  });
  it("owns matches generics by base type only when the catalog names the base", () => {
    expect(owns({ collections: [{ nftType: HERO, name: "" }] }, HERO)).toBe(true);
    expect(owns({ collections: [{ nftType: HERO, name: "" }] }, OTHER)).toBe(false);
    expect(owns({ collections: [] }, HERO)).toBe(false);
  });
});

describe("sell is scoped to the game and the player", () => {
  it("opens for the player's own item from this game", async () => {
    const r = await prepareSell("web:act", { itemId: MINE, mode: "list", priceMist: "2200000000" }, deps());
    expect("status" in r).toBe(false);
    if ("status" in r) return;
    expect(r.item.id).toBe(MINE);
    expect(r.mode).toBe("list");
    expect(r.priceMist).toBe(2_200_000_000n);
  });
  it("refuses an item from another game", async () => {
    await expect(prepareSell("web:act", { itemId: ELSEWHERE }, deps())).rejects.toMatchObject({ code: "not_this_game" });
  });
  it("an item of this game the player doesn't hold is unavailable", async () => {
    const other = A("5e");
    const r = await prepareSell("web:act", { itemId: other }, deps({ routes: { [`/v1/items/${other}`]: card(other, HERO) } }));
    expect(r).toEqual({ status: "unavailable", reason: "You don't hold this item" });
  });
  it("a watch-only account can't sell", async () => {
    await expect(prepareSell("web:act", { itemId: MINE }, deps({ account: async () => ({ address: PLAYER, kind: "watch" }) }))).rejects.toBeInstanceOf(ApiFail);
  });
});

describe("buy is scoped to the game", () => {
  it("the cheapest listing of the game's items, skipping the player's own", async () => {
    const r = await prepareBuy("web:act", { collection: HERO }, deps());
    expect("status" in r ? r : r.item.id).toBe(A("3c"));
  });
  it("respects maxMist", async () => {
    expect(await prepareBuy("web:act", { collection: HERO, maxMist: "900" }, deps())).toEqual({ status: "unavailable", reason: "Nothing listed at or under maxMist" });
  });
  it("refuses another game's item or collection", async () => {
    await expect(prepareBuy("web:act", { itemId: ELSEWHERE }, deps())).rejects.toMatchObject({ code: "not_this_game" });
    await expect(prepareBuy("web:act", { collection: OTHER }, deps())).rejects.toMatchObject({ code: "not_this_game" });
  });
  it("by kind reads the item's kind attribute", async () => {
    const r = await prepareBuy("web:act", { kind: 3 }, deps({ routes: { [`/v1/items/${A("3c")}`]: { ...card(A("3c"), HERO), attributes: [{ type: "kind", value: "3" }] } } }));
    expect("status" in r ? r : r.item.id).toBe(A("3c"));
    expect(await prepareBuy("web:act", { kind: 4 }, deps({ routes: { [`/v1/items/${A("3c")}`]: { ...card(A("3c"), HERO), attributes: [{ type: "kind", value: "3" }] } } }))).toMatchObject({ status: "unavailable" });
  });
});

// @rare/sdk's documented test vector (rare-sdk packages/sdk/test/server.test.ts): the envelope its
// itemVoucher makes, as the base64url token the game server sends.
const VECTOR_SIG = "0c429736bcdb62babb3ac80cca7108d51a3e02a46bf335c7ff401e067e1f2226fb12d888f34dc40101afdd19529552c7a91292f0343efd11e90ade19c96f3d0a";
const hexBytes = (h: string) => Uint8Array.from(h.match(/../g)!.map((x) => parseInt(x, 16)));
const b64 = (b: Uint8Array) => btoa(String.fromCharCode(...b));
const envelope = (o: Record<string, unknown>) => JSON.stringify({ v: 1, sig: b64(hexBytes(VECTOR_SIG)), ...o });
const VECTOR = envelope({ type: "item", class: A("c1"), player: PLAYER, kind: "3", keys: ["power"], vals: ["12"], nonce: "7", expiresMs: "1800000600000" });
const base64url = (s: string) => b64(new TextEncoder().encode(s)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const base64 = (s: string) => b64(new TextEncoder().encode(s));

describe("vouchers: @rare/sdk's envelope", () => {
  it("reads the SDK's base64url token and the padded base64 the SDK sends to claim", () => {
    for (const t of [base64url(VECTOR), base64(VECTOR)]) {
      const v = decodeVoucher(t);
      expect(v).toMatchObject({ type: "item", classId: A("c1"), player: PLAYER, kind: 3n, keys: ["power"], vals: ["12"], nonce: 7n, expiresMs: 1_800_000_600_000n });
      expect(Array.from(v.sig)).toEqual(Array.from(hexBytes(VECTOR_SIG)));
    }
  });
  it("reads state and trophy vouchers", () => {
    expect(decodeVoucher(base64url(envelope({ type: "state", item: A("1a"), rev: "4", keys: ["power"], vals: ["13"], nonce: "1", expiresMs: "9" })))).toMatchObject({ type: "state", itemId: MINE, rev: 4n });
    expect(decodeVoucher(base64url(envelope({ type: "trophy", registry: A("e1"), player: PLAYER, game: A("e2"), trophyId: "2", nonce: "1", expiresMs: "9" })))).toMatchObject({ type: "trophy", trophyId: 2n });
  });
  it("refuses what isn't a voucher", () => {
    const bad = (o: Record<string, unknown>) => () => decodeVoucher(base64url(envelope(o)));
    expect(() => decodeVoucher("not base64!")).toThrow(VoucherError);
    expect(() => decodeVoucher(base64url("[1]"))).toThrow(VoucherError);
    expect(bad({ type: "item", v: 2 })).toThrow(/newer version/);
    expect(bad({ type: "loot", nonce: "1", expiresMs: "1" })).toThrow(VoucherError);
    expect(bad({ type: "item", class: "nope", player: PLAYER, kind: "1", keys: [], vals: [], nonce: "1", expiresMs: "1" })).toThrow(/item class/);
    expect(bad({ type: "item", class: A("c1"), player: PLAYER, kind: "1", keys: ["a", "a"], vals: ["1", "2"], nonce: "1", expiresMs: "1" })).toThrow(/twice/);
    expect(bad({ type: "item", class: A("c1"), player: PLAYER, kind: "1", keys: ["a"], vals: [], nonce: "1", expiresMs: "1" })).toThrow(/line up/);
    expect(bad({ type: "item", class: A("c1"), player: PLAYER, kind: "18446744073709551616", keys: [], vals: [], nonce: "1", expiresMs: "1" })).toThrow(/kind/);
    expect(() => decodeVoucher(base64url(JSON.stringify({ v: 1, type: "item", class: A("c1"), player: PLAYER, kind: "1", keys: [], vals: [], nonce: "1", expiresMs: "1", sig: "AAAA" })))).toThrow(/signature/);
  });
  it("only for the player's account, and not expired", () => {
    const v = decodeVoucher(base64url(VECTOR));
    expect(rejectReason(v, PLAYER, 1_800_000_000_000)).toBeNull();
    expect(rejectReason(v, A("77"), 1_800_000_000_000)).toMatch(/another account/);
    expect(rejectReason(v, PLAYER, 1_800_000_600_000)).toMatch(/expired/);
  });
  it("b64any takes both alphabets", () => {
    expect(Array.from(b64any("-_8"))).toEqual(Array.from(b64any("+/8=")));
  });
});

describe("claims read the class from chain, never from the voucher", () => {
  const PKG = normalizeSuiAddress("0x5");
  const marker = "0x0000000000000000000000000000000000000000000000000000000000000a11::act::Hero";
  const reader = async (id: string) =>
    id === A("c1")
      ? { type: `${PKG}::class::Class<${marker}>`, json: { policy_cap: { id: A("d0"), policy_id: A("d1") } } }
      : id === MINE
        ? { type: `${PKG}::item::Item<${marker}>`, json: { class: A("c1"), rev: "4" } }
        : id === A("f0")
          ? { type: `0x9::class::Class<${marker}>`, json: {} }
          : null;
  it("reads marker type and policy", async () => {
    expect(await readClass(PKG, A("c1"), reader)).toEqual({ marker, policyId: A("d1") });
    expect(await readItem(PKG, MINE, reader)).toEqual({ marker, classId: A("c1"), rev: 4n });
    await expect(readClass(PKG, A("f0"), reader)).rejects.toThrow(/isn't a Rare items class/);
    await expect(readClass(PKG, A("ee"), reader)).rejects.toThrow(VoucherError);
  });
  it("splits generic types", () => {
    expect(splitGeneric("0x5::item::Item<0x1::a::B<0x2::c::D>>")).toEqual([`${PKG}::item::Item`, "0x1::a::B<0x2::c::D>"]);
    expect(splitGeneric("0x5::item::Item")).toBeNull();
  });
  it("builds mint::claim into a plain kiosk, claim_fresh without one, and apply through borrow_val", () => {
    const v = decodeVoucher(base64url(VECTOR));
    if (v.type !== "item") throw new Error("item");
    const calls = (tx: ReturnType<typeof buildItemClaim>) => tx.getData().commands.map((c) => (c.MoveCall ? `${c.MoveCall.module}::${c.MoveCall.function}` : c.$kind));
    expect(calls(buildItemClaim(PKG, v, { marker, policyId: A("d1") }, PLAYER, { kioskId: A("ab"), ownerCapId: A("ac") }))).toEqual(["mint::claim"]);
    expect(calls(buildItemClaim(PKG, v, { marker, policyId: A("d1") }, PLAYER, null))).toEqual(["mint::claim_fresh"]);
    const s = decodeVoucher(base64url(envelope({ type: "state", item: MINE, rev: "4", keys: ["power"], vals: ["13"], nonce: "1", expiresMs: "9" })));
    if (s.type !== "state") throw new Error("state");
    expect(calls(buildStateClaim(PKG, s, { marker, classId: A("c1") }, PLAYER, { kioskId: A("ab"), ownerCapId: A("ac") }))).toEqual(["kiosk::borrow_val", "state::apply", "kiosk::return_val"]);
    expect(calls(buildStateClaim(PKG, s, { marker, classId: A("c1") }, PLAYER, null))).toEqual(["state::apply"]);
  });
  it("prepareClaim checks claims against the live mainnet rare_items and Rare Trophies packages", async () => {
    const d = deps();
    const now = 1_800_000_000_000;
    // Both packages are published on mainnet (deployment.json, items.json): no "isn't live yet".
    // The test vector's class is not from the mainnet rare_items package.
    expect(await prepareClaim("web:act", base64url(VECTOR), d, now)).toEqual({ status: "rejected", reason: "The voucher's item class isn't a Rare items class" });
    const trophy = await prepareClaim("web:act", base64url(envelope({ type: "trophy", registry: A("e1"), player: PLAYER, game: A("e2"), trophyId: "2", nonce: "1", expiresMs: String(now + 1) })), d, now);
    expect(trophy).not.toMatchObject({ status: "unavailable" });
    expect(await prepareClaim("web:act", base64url(VECTOR), deps({ account: async () => ({ address: A("77"), kind: "native" }) }), now)).toEqual({ status: "rejected", reason: "This voucher is for another account" });
    await expect(prepareClaim("web:act", "@@", d, now)).rejects.toMatchObject({ code: "bad_request" });
  });
});

describe("notices reach the game whose items they are about", () => {
  const notice = (kind: string, slug: string | null): Notice =>
    ({
      id: "n1",
      kind,
      addr: PLAYER,
      title: "Offer: 31 SUI for Hero #1",
      body: null,
      imageUrl: null,
      amount: { raw: "31000000000", decimals: 9, symbol: "SUI", coinType: "0x2::sui::SUI", usd: null },
      item: slug ? { id: MINE, name: "Hero #1", imageUrl: null, collection: { slug, name: "Heroes" } } : null,
      action: { kind: "item", itemId: MINE, slug },
      digest: "D1",
      cpTs: null,
      createdAt: 1,
      sentAt: 1,
    }) as Notice;
  it("matches by the collection's slug, for offer_received, sold and bought only", async () => {
    const d = deps();
    expect(await gamesFor(notice("offer_received", "act-heroes"), ["web:act", "steam:1"], d)).toEqual(["web:act"]);
    expect(await gamesFor(notice("sold", "someone-else"), ["web:act"], d)).toEqual([]);
    expect(await gamesFor(notice("coin_received", "act-heroes"), ["web:act"], d)).toEqual([]);
    expect(await gamesFor(notice("bought", null), ["web:act"], d)).toEqual([]);
  });
  it("sends the shape @rare/sdk reads (kind, title, itemId, amountMist, digest)", () => {
    expect(gameNotice(notice("offer_received", "act-heroes"))).toMatchObject({ id: "n1", kind: "offer_received", title: "Offer: 31 SUI for Hero #1", itemId: MINE, amountMist: "31000000000", digest: "D1" });
  });
});
