import { describe, expect, it } from "vitest";
import seed from "../lib/games.seed.json";
import type { SuiGame } from "../lib/rare";
import type { Game, Library } from "../lib/types";
import { installedNative, isActive, playUrl, profileForUrl, unavailable, withBuiltins } from "./catalog";

const games = seed as unknown as SuiGame[];
const bySlug = (s: string) => games.find((g) => g.slug === s)!;

describe("Sui Games catalog", () => {
  it("shows only live and beta games, the rest wait in Coming", () => {
    const active = games.filter(isActive).map((g) => g.slug);
    expect(active).toContain("act");
    expect(active).toContain("xociety");
    for (const g of games.filter((g) => !isActive(g))) expect(["announced", "paused", "dead", "coming"]).toContain(g.status);
    expect(active).not.toContain("sui-8192");
  });

  it("resolves where web games play", () => {
    expect(playUrl(bySlug("act"))).toBe("https://play.actuniverse.com/");
    expect(playUrl(bySlug("suuuiplash"))).toBeNull();
    expect(playUrl(bySlug("pawtato-land"))).toBe("https://pawtato.gg/");
    expect(playUrl(bySlug("pebble-city"))).toBeNull();
    expect(unavailable("pebble-city")).toMatch(/offline/);
    expect(playUrl(bySlug("eve-frontier"))).toBeNull();
  });

  it("keeps ACT as the one built-in web game", () => {
    const all = withBuiltins(games);
    expect(all.filter((g) => g.slug === "act")).toHaveLength(1);
  });

  it("finds profiles by host for games added by URL", () => {
    expect(profileForUrl("web:my-copy", "https://www.pawtato.gg/")?.input).toBe("focus");
    expect(profileForUrl("web:act", null)?.input).toBe("gamepad");
  });

  it("matches native Sui games to installed library games", () => {
    const g = (id: string, title: string): Game => ({ id, title, source: id.startsWith("steam") ? "steam" : "epic", installed: true, art: { cover: [], hero: [], logo: [], header: [] }, origin: "", foreign: false });
    const lib = { games: [g("steam:3277880", "Warped Universe Playtest"), g("epic:abc", "XOCIETY")] } as Library;
    expect(installedNative("warped-universe", "Warped Universe", lib)?.id).toBe("steam:3277880");
    expect(installedNative("xociety", "XOCIETY", lib)?.id).toBe("epic:abc");
    expect(installedNative("eve-frontier", "EVE Frontier", lib)).toBeNull();
  });
});
