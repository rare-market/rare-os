import { describe, expect, it } from "vitest";
import type { SuiGame } from "../lib/rare";
import type { Library } from "../lib/types";
import type { MarketEvent } from "../marketplace/api";
import { loadItems, loadMarket, pickTrophies, sortItems, structLabel, toNotes, type OverlayItem } from "./data";
import { catalogFor, resolveGame } from "./game";
import { parseSuiInput } from "./price";

const LAND = "0x09afb9a1c63a9bbaba650ef0a6b473b9874882cd63aab9570b99274e8f796f00::pawtato::LAND";
const TOOL = "0x42482d397dd0f1dc8dab9fdf8db9b81a6d4ce9e32597ed522cde660a8773b1a9::pawtato_tools::TOOL";
const PIONEER = "0xe60adf3d565f00ddc0d5a27e656ad604442c7ca15248679a50307c19fc358e24::warped_pioneers::Nft";
const ME = "0x7aa0c2206ef649591f9b12db3baa4572e01a30727b949f7749fc50e08f5ad784";
const OTHER = "0x00000000000000000000000000000000000000000000000000000000000000aa";

const game = (slug: string, name: string, collections: Array<{ nftType: string; name: string }>, platforms: SuiGame["platforms"] = ["web"]): SuiGame => ({
  slug,
  name,
  studio: "",
  genre: "",
  status: "live",
  platforms,
  website: "",
  x: "",
  description: "",
  logoUrl: `https://img/${slug}.png`,
  bannerUrl: `https://img/${slug}-banner.jpg`,
  collections,
  token: null,
  featured: false,
});

const CATALOG = [
  game("pawtato-land", "Pawtato Land", [
    { nftType: LAND, name: "Pawtato Land" },
    { nftType: TOOL, name: "Pawtato Tools" },
  ]),
  game("warped-universe", "Warped Universe", [{ nftType: PIONEER, name: "Warped Pioneers" }], ["pc", "suiplay"]),
];

const lib = (games: Array<Partial<Library["games"][number]> & { id: string; title: string }>): Library =>
  ({
    games: games.map((g) => ({ source: "steam", installed: true, installDir: null, sizeBytes: null, lastPlayed: null, art: { cover: [], hero: [], logo: [], header: [] }, origin: "", foreign: false, ...g })),
    sources: [],
  }) as unknown as Library;

describe("which game is under the overlay", () => {
  it("maps web, Steam and title ids to the catalog", () => {
    expect(catalogFor("web:pawtato-land", null, CATALOG)?.slug).toBe("pawtato-land");
    // Installed by URL under another id: matched by the profile's hosts.
    expect(catalogFor("web:my-potato", lib([{ id: "web:my-potato", title: "Potato", source: "web", launch: { kind: "web", url: "https://pawtato.gg/play" } }]), CATALOG)?.slug).toBe("pawtato-land");
    // Steam app id from the launcher's profiles.
    expect(catalogFor("steam:3277880", null, CATALOG)?.slug).toBe("warped-universe");
    // A non-Steam install by title.
    expect(catalogFor("local:wu", lib([{ id: "local:wu", title: "Warped Universe", source: "local" }]), CATALOG)?.slug).toBe("warped-universe");
    expect(catalogFor("steam:413150", null, CATALOG)).toBeNull();
    expect(catalogFor(null, null, CATALOG)).toBeNull();
  });

  it("collects the game's collections and packages", () => {
    const g = resolveGame("web:pawtato-land", null, CATALOG);
    expect(g.title).toBe("Pawtato Land");
    expect(g.collections.map((c) => c.name)).toEqual(["Pawtato Land", "Pawtato Tools"]);
    expect(g.packages).toHaveLength(2);
    expect(g.packages[0]).toMatch(/^0x09afb9a1/);
    const unknown = resolveGame("steam:413150", lib([{ id: "steam:413150", title: "Stardew Valley" }]), CATALOG);
    expect(unknown.title).toBe("Stardew Valley");
    expect(unknown.collections).toEqual([]);
    expect(unknown.steamAppId).toBe(413150);
    expect(g.steamAppId).toBeNull();
    expect(resolveGame("local:wu", lib([{ id: "local:wu", title: "Warped Universe", source: "local" }]), CATALOG).steamAppId).toBe(3277880);
  });
});

const item = (id: string, extra: Partial<OverlayItem> = {}): OverlayItem => ({ id, name: id, type: LAND, imageUrl: null, collection: null, floorMist: "30000000000", ...extra });

describe("items", () => {
  it("puts the most sellable first", () => {
    const sorted = sortItems([item("plain"), item("listed", { listing: { venue: "kiosk", priceMist: "1" } }), item("offer small", { topOfferMist: "10" }), item("offer big", { topOfferMist: "20" })]);
    expect(sorted.map((x) => x.id)).toEqual(["offer big", "offer small", "listed", "plain"]);
  });

  it("reads items per collection with the collection floor", async () => {
    const seen: string[] = [];
    const get = async <T,>(p: string): Promise<T> => {
      seen.push(p);
      if (p.startsWith("/v1/accounts/")) return { data: p.includes(encodeURIComponent(LAND)) ? [item("land 1", { floorMist: null })] : [item("tool 1", { type: TOOL, floorMist: null })], nextCursor: null } as T;
      if (p.startsWith("/v1/collections/")) return { slug: p.includes(encodeURIComponent(LAND)) ? "pawtato-land" : "pawtato-tools", name: "x", stats: { floorMist: p.includes(encodeURIComponent(LAND)) ? "32960000000" : "77000000", topOfferMist: null } } as T;
      throw new Error(p);
    };
    const r = await loadItems(get, ME, resolveGame("web:pawtato-land", null, CATALOG));
    expect(r.items.map((x) => [x.id, x.floorMist])).toEqual([
      ["land 1", "32960000000"],
      ["tool 1", "77000000"],
    ]);
    expect(r.floors[LAND].slug).toBe("pawtato-land");
    expect(seen.filter((p) => p.startsWith("/v1/accounts/")).length).toBe(2);
  });

  it("market: cheapest all-in first, never the player's own listings", async () => {
    const card = (id: string, all: string, seller = OTHER) => ({ id, name: id, type: LAND, imageUrl: null, collection: null, listing: { venue: "tradeport", priceMist: all, allInMist: all, seller } });
    const get = async <T,>(p: string): Promise<T> =>
      ({ data: p.includes("pawtato-land") ? [card("a", "50"), card("mine", "1", ME)] : [card("b", "20"), card("c", "90")], nextCursor: null }) as T;
    const g = resolveGame("web:pawtato-land", null, CATALOG);
    const floors = { [g.collections[0].nftType]: { slug: "pawtato-land", name: "", floorMist: null, topOfferMist: null }, [g.collections[1].nftType]: { slug: "pawtato-tools", name: "", floorMist: null, topOfferMist: null } };
    expect((await loadMarket(get, g, floors, ME)).map((x) => x.id)).toEqual(["b", "a", "c"]);
  });
});

describe("notifications", () => {
  const ev = (id: string, kind: MarketEvent["kind"], from: string | null, to: string | null, at: string, priceMist: string | null = "42000000000"): MarketEvent => ({
    id,
    kind,
    venue: "tradeport",
    item: { id: `item-${id}`, name: `Land #${id}`, imageUrl: null },
    collection: null,
    priceMist,
    from,
    to,
    fromName: null,
    toName: null,
    at,
  });

  it("says what happened to the player's items, newest first", () => {
    const notes = toNotes(
      ME,
      [
        ev("1", "sale", ME, OTHER, "2026-10-20T10:00:00Z"),
        ev("2", "sale", OTHER, ME, "2026-10-21T10:00:00Z", "5000000000"),
        ev("3", "list", ME, null, "2026-10-22T10:00:00Z", "75000000000"),
        ev("4", "sale", OTHER, OTHER, "2026-10-23T10:00:00Z"),
        ev("5", "transfer", OTHER, ME, "2026-10-19T10:00:00Z", null),
        ev("1", "sale", ME, OTHER, "2026-10-20T10:00:00Z"),
      ],
      [{ id: "o1", priceMist: "30000000000", kind: "item", item: { id: "x", name: "Land #9" }, createdAt: "2026-10-24T10:00:00Z" }],
    );
    expect(notes.map((n) => n.text)).toEqual([
      "Offer of 30 SUI on Land #9",
      "Listed Land #3 for 75 SUI",
      "Bought Land #2 for 5 SUI",
      "Sold Land #1 for 42 SUI",
      "Received Land #5",
    ]);
    expect(notes.find((n) => n.text.startsWith("Sold"))?.tone).toBe("positive");
    expect(notes.find((n) => n.text.startsWith("Bought"))?.tone).toBe("muted");
    expect(notes.every((n) => !n.text.includes("\u2014"))).toBe(true);
  });
});

describe("trophies", () => {
  it("keeps other objects from the game's packages, not its collections or other games", () => {
    const g = resolveGame("web:pawtato-land", null, CATALOG);
    const pkg = LAND.split("::")[0];
    const t = pickTrophies(
      [
        { objectId: "0x1", type: LAND, display: { name: "Land #1" } },
        { objectId: "0x2", type: `${pkg}::badges::SeasonOneBadge`, display: { name: "Season one", image_url: "https://img/b.png" } },
        { objectId: "0x3", type: `${pkg}::quest::Quest_Reward`, display: null },
        { objectId: "0x4", type: "0x2::coin::Coin<0x2::sui::SUI>", display: null },
      ],
      g,
    );
    expect(t.map((x) => [x.id, x.name, x.imageUrl])).toEqual([
      ["0x2", "Season one", "https://img/b.png"],
      ["0x3", "Quest Reward", null],
    ]);
    expect(structLabel("0x2::badges::SeasonOneBadge<0x2::sui::SUI>")).toBe("Season One Badge");
  });
});

describe("price input", () => {
  it("reads SUI amounts", () => {
    expect(parseSuiInput("12")).toBe(12_000_000_000n);
    expect(parseSuiInput("12.5")).toBe(12_500_000_000n);
    expect(parseSuiInput(".25")).toBe(250_000_000n);
    expect(parseSuiInput("1,200 SUI")).toBe(1_200_000_000_000n);
    expect(parseSuiInput("0.000000001")).toBe(1n);
    expect(parseSuiInput("0")).toBeNull();
    expect(parseSuiInput("1.0000000001")).toBeNull();
    expect(parseSuiInput("abc")).toBeNull();
    expect(parseSuiInput("")).toBeNull();
  });
});
