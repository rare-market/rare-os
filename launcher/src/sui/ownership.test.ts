import { describe, expect, it } from "vitest";
import type { SuiGame } from "../lib/rare";
import { matchOwnership, typeMatches } from "./ownership";

const game = (slug: string, types: string[]): SuiGame => ({
  slug,
  name: slug,
  studio: "",
  genre: "",
  platforms: [],
  website: "",
  x: "",
  description: "",
  logoUrl: "",
  bannerUrl: "",
  collections: types.map((t) => ({ nftType: t, name: `${slug} items` })),
  token: null,
  featured: false,
});

describe("typeMatches", () => {
  it("matches the same type written two ways", () => {
    expect(typeMatches("0x09afb9a1c63a9bbaba650ef0a6b473b9874882cd63aab9570b99274e8f796f00::pawtato::LAND", "0x9afb9a1c63a9bbaba650ef0a6b473b9874882cd63aab9570b99274e8f796f00::pawtato::LAND")).toBe(true);
  });
  it("a catalog type without type arguments matches any instantiation", () => {
    expect(typeMatches("0xee::suifrens::SuiFren<0x88::capy::Capy>", "0xee::suifrens::SuiFren")).toBe(true);
  });
  it("a catalog type with type arguments must match exactly", () => {
    expect(typeMatches("0xbd::nonfungible::Nft<0xe2::a::A>", "0xbd::nonfungible::Nft<0xe3::b::B>")).toBe(false);
    expect(typeMatches("0xbd::nonfungible::Nft<0xe2::a::A>", "0xbd::nonfungible::Nft<0xe2::a::A>")).toBe(true);
  });
  it("different modules or structs never match", () => {
    expect(typeMatches("0x1::a::B", "0x1::a::C")).toBe(false);
    expect(typeMatches("0x1::a::B", "0x2::a::B")).toBe(false);
  });
});

describe("matchOwnership", () => {
  it("joins held collections to games and sums counts", () => {
    const games = [game("pawtato", ["0x1::pawtato::LAND", "0x2::heroes::HERO"]), game("other", ["0x9::x::Y"])];
    const held = [
      { type: "0x1::pawtato::LAND", count: 2, slug: "pawtato-land", name: "Pawtato Land", items: [] },
      { type: "0x2::heroes::HERO", count: 3, slug: "", name: "Heroes", items: [] },
      { type: "0x5::z::Z", count: 1, slug: "z", name: "Z", items: [] },
    ];
    const m = matchOwnership(games, held);
    expect([...m.keys()]).toEqual(["pawtato"]);
    expect(m.get("pawtato")!.count).toBe(5);
    expect(m.get("pawtato")!.collections.map((c) => c.slug)).toEqual(["pawtato-land", null]);
  });
});
