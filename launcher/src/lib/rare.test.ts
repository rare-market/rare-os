import { describe, expect, it } from "vitest";
import { fromApi } from "./rare";

const A = "0x" + "1".repeat(64);

describe("fromApi (/v1/games)", () => {
  it("keeps the games.json shape and drops unproven collections", () => {
    const games = fromApi({
      data: [
        {
          slug: "moon",
          name: "Moon",
          studio: "Moon Labs",
          status: "live",
          platforms: ["web"],
          website: "https://moon.example",
          playUrl: "https://play.moon.example",
          x: null,
          collections: [
            { nftType: `${A}::hat::Hat`, name: "Hats", verified: true },
            { nftType: `${A}::fake::Fake`, name: "Fake", verified: false },
          ],
          token: null,
          featured: false,
          type: "web",
          gameKey: null,
        },
        { name: "no slug" },
      ],
      units: {},
    });
    expect(games).toHaveLength(1);
    expect(games[0].x).toBe("");
    expect(games[0].playUrl).toBe("https://play.moon.example");
    expect(games[0].collections.map((c) => c.name)).toEqual(["Hats"]);
  });

  it("anything else is no games, so the caller falls back", () => {
    expect(fromApi(null)).toEqual([]);
    expect(fromApi({ error: { code: "unavailable" } })).toEqual([]);
    expect(fromApi([{ slug: "a", name: "A" }])).toEqual([]);
  });
});
