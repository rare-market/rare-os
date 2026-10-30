import { describe, expect, it } from "vitest";
import { encodeQuilt } from "@mysten/walrus";
import type { SuiGame } from "../lib/rare";
import type { Game, Library } from "../lib/types";
import { buildShareJson, estimateQuiltBytes, fitWithin, fmtDuration, posterTime, shareAttributes, shareEpochs, shareLink, slugFor } from "./format";
import type { Capture } from "./native";
import { shareFiles } from "./upload";

const shot: Capture = {
  id: "shot-1",
  kind: "screenshot",
  path: "/home/rare/Pictures/Rare/shot-1.webp",
  mime: "image/webp",
  bytes: 810_598,
  width: 1920,
  height: 1080,
  durationMs: null,
  createdMs: 1_761_500_000_000,
  game: { id: "web:act", name: "ACT" },
};
const clip: Capture = { ...shot, id: "clip-1", kind: "clip", mime: "video/mp4", bytes: 15_059_947, durationMs: 30_000, game: { id: "steam:1145350", name: "Hades II" } };

describe("share.json", () => {
  it("describes a screenshot", () => {
    expect(buildShareJson(shot, { bytes: 810_598, slug: "act", poster: false })).toEqual({
      format: "rare.share/1",
      kind: "screenshot",
      file: "shot.webp",
      mime: "image/webp",
      width: 1920,
      height: 1080,
      durationMs: null,
      bytes: 810_598,
      poster: null,
      game: { name: "ACT", slug: "act" },
      created: 1_761_500_000_000,
    });
  });
  it("describes a clip with its poster, and a Steam game by name only", () => {
    const j = buildShareJson(clip, { bytes: 15_059_947, slug: null, poster: true });
    expect(j).toMatchObject({ kind: "clip", file: "clip.mp4", mime: "video/mp4", durationMs: 30_000, poster: "poster.jpg", game: { name: "Hades II", slug: null } });
  });
  it("never carries the player's address and has no game when none was playing", () => {
    const j = buildShareJson({ ...shot, game: null }, { bytes: 1, slug: null, poster: false });
    expect(j.game).toBeNull();
    expect(JSON.stringify(j)).not.toMatch(/0x[0-9a-f]{6,}/i);
    expect(Object.keys(j).sort()).toEqual(["bytes", "created", "durationMs", "file", "format", "game", "height", "kind", "mime", "poster", "width"]);
  });
  it("a screenshot never names a poster", () => {
    expect(buildShareJson(shot, { bytes: 1, slug: null, poster: true }).poster).toBeNull();
  });
  it("quilt files use the exact identifiers, poster only for clips", () => {
    const media = new Uint8Array(10);
    const s = shareFiles(buildShareJson(shot, { bytes: 10, slug: null, poster: false }), media, null);
    expect(s.map((f) => [f.identifier, f.mime])).toEqual([
      ["share.json", "application/json"],
      ["shot.webp", "image/webp"],
    ]);
    const c = shareFiles(buildShareJson(clip, { bytes: 10, slug: null, poster: true }), media, new Uint8Array(3));
    expect(c.map((f) => f.identifier)).toEqual(["share.json", "clip.mp4", "poster.jpg"]);
    expect(JSON.parse(new TextDecoder().decode(c[0].bytes)).format).toBe("rare.share/1");
  });
  it("tags the blob for a later list of shares", () => {
    expect(shareAttributes("clip")).toEqual({ app: "rare-share", v: "1", kind: "clip" });
  });
});

describe("links", () => {
  it("mainnet and testnet", () => {
    expect(shareLink("abc_DEF-123", "mainnet")).toBe("https://rare.market/s/abc_DEF-123");
    expect(shareLink("abc_DEF-123", "testnet")).toBe("https://rare.market/s/abc_DEF-123?n=testnet");
  });
});

describe("durations", () => {
  it("1 month is 3 mainnet epochs (42 days), 1 year is 27", () => {
    expect(shareEpochs("month", "mainnet")).toEqual({ epochs: 3, days: 42 });
    expect(shareEpochs("year", "mainnet")).toEqual({ epochs: 27, days: 378 });
  });
  it("testnet epochs are a day, capped at the 53 epoch limit", () => {
    expect(shareEpochs("month", "testnet")).toEqual({ epochs: 30, days: 30 });
    expect(shareEpochs("year", "testnet")).toEqual({ epochs: 53, days: 53 });
  });
});

describe("estimated quilt size", () => {
  it("is never below the SDK's real quilt for screenshots and clips", () => {
    const cases = [
      [300, 200_000],
      [300, 810_598],
      [300, 3_000_000],
      [300, 15_059_947, 42_309],
      [300, 60_000_000, 150_000],
    ];
    for (const sizes of cases) {
      const blobs = sizes.map((n, i) => ({ identifier: `f${i}`, contents: new Uint8Array(n) }));
      const real = encodeQuilt({ blobs, numShards: 1000 }).quilt.length;
      const est = estimateQuiltBytes(sizes);
      expect(est).toBeGreaterThanOrEqual(real);
      expect(est).toBeLessThan(real * 1.25 + 300_000);
    }
  });
});

describe("poster", () => {
  it("fits the long edge in 1280", () => {
    expect(fitWithin(1920, 1080)).toEqual({ w: 1280, h: 720 });
    expect(fitWithin(1080, 1920)).toEqual({ w: 720, h: 1280 });
    expect(fitWithin(800, 600)).toEqual({ w: 800, h: 600 });
  });
  it("grabs about 1 s in, or 40 percent of a short clip", () => {
    expect(posterTime(30)).toBe(1);
    expect(posterTime(2)).toBeCloseTo(0.8);
    expect(posterTime(Number.NaN)).toBe(0);
  });
  it("formats durations", () => {
    expect(fmtDuration(30_000)).toBe("0:30");
    expect(fmtDuration(65_400)).toBe("1:05");
  });
});

const game = (slug: string, name: string, platforms: SuiGame["platforms"] = ["web"]): SuiGame => ({
  slug,
  name,
  studio: "",
  genre: "",
  platforms,
  website: "",
  x: "",
  description: "",
  logoUrl: "",
  bannerUrl: "",
  collections: [],
  token: null,
  featured: false,
});
const lib = (games: Partial<Game>[]): Library =>
  ({ games: games.map((g) => ({ installed: true, source: "steam", art: { cover: [], hero: [], logo: [], header: [] }, origin: "", foreign: false, title: "", id: "", ...g })), sources: [], foreignSteamLibraries: [], scannedAt: 0 }) as unknown as Library;

describe("rare.market game slug", () => {
  const catalog = [game("act", "ACT"), game("wordgame", "Word Game"), game("xociety", "XOCIETY", ["pc", "suiplay"]), game("cosmocadia", "Cosmocadia")];
  it("built-in web games by id", () => {
    expect(slugFor({ id: "web:act", name: "ACT" }, catalog, null)).toBe("act");
  });
  it("an added web game by its URL host", () => {
    const l = lib([{ id: "web:my-cosmo", source: "web", title: "My Cosmo", launch: { kind: "web", url: "https://www.cosmocadia.io/play" } }]);
    expect(slugFor({ id: "web:my-cosmo", name: "My Cosmo" }, catalog, l)).toBe("cosmocadia");
  });
  it("a native Sui game by its library title", () => {
    const l = lib([{ id: "epic:xo", source: "epic", title: "XOCIETY" }]);
    expect(slugFor({ id: "epic:xo", name: "XOCIETY" }, catalog, l)).toBe("xociety");
  });
  it("Steam and other games send null", () => {
    expect(slugFor({ id: "steam:1145350", name: "Hades II" }, catalog, lib([{ id: "steam:1145350", title: "Hades II" }]))).toBeNull();
    expect(slugFor(null, catalog, null)).toBeNull();
  });
});
