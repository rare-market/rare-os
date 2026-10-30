import { describe, expect, it } from "vitest";
import type { SuiGame } from "../lib/rare";
import type { Game, Library } from "../lib/types";
import { onlineFor, playingFor, steamAppId } from "./background";
import { backgroundLine } from "./BackgroundSwitch";
import type { OverlayItem } from "./data";
import { gameKind, inOrder, inviteUp, itemsLine, menuTabs, openingTab, recentNotRunning } from "./menuModel";

const MIST = 1_000_000_000n;

describe("Rare menu tabs", () => {
  it("over a Sui game: Game, Items, Market, Trophies, Friends, Notifications", () => {
    expect(menuTabs({ overGame: true, collections: true, trophies: true })).toEqual(["game", "items", "market", "trophies", "friends", "news"]);
  });
  it("over a Steam game with no collections: Game, Trophies, Friends, Notifications", () => {
    expect(menuTabs({ overGame: true, collections: false, trophies: true })).toEqual(["game", "trophies", "friends", "news"]);
  });
  it("over an app: Game, Friends, Notifications", () => {
    expect(menuTabs({ overGame: true, collections: false, trophies: false })).toEqual(["game", "friends", "news"]);
  });
  it("over the launcher, Game is replaced by Rare and the game's tabs go", () => {
    expect(menuTabs({ overGame: false, collections: true, trophies: true })).toEqual(["rare", "friends", "news"]);
  });
  it("opens on Game over a game (Resume focused), whatever is unread", () => {
    expect(openingTab(true, 0)).toBe("game");
    expect(openingTab(true, 5)).toBe("game");
  });
  it("over the launcher opens on Notifications when any are unread, else Rare", () => {
    expect(openingTab(false, 2)).toBe("news");
    expect(openingTab(false, 0)).toBe("rare");
  });
  it("tells the kinds apart for the per-app rows", () => {
    expect(gameKind("steam:620")).toBe("steam");
    expect(gameKind("app:telegram")).toBe("app");
    expect(gameKind("stream:abc:1")).toBe("stream");
    expect(gameKind("web:act")).toBe("web");
    expect(gameKind("local:hades")).toBe("game");
    expect(gameKind(null)).toBe(null);
  });
});

describe("your items line", () => {
  const item = (floor: bigint | null, offer: bigint | null) => ({ floorMist: floor?.toString() ?? null, topOfferMist: offer?.toString() ?? null }) as unknown as OverlayItem;
  it("counts, worth at floor, and items with an offer", () => {
    expect(itemsLine([item(2n * MIST, MIST), item(3n * MIST, null), item(5n * MIST, 2n * MIST)])).toBe("3 items · worth 10.00 SUI · 2 offers");
  });
  it("leaves out what it doesn't know", () => {
    expect(itemsLine([item(null, null)])).toBe("1 item");
    expect(itemsLine([item(null, MIST)])).toBe("1 item · 1 offer");
  });
});

describe("Switcher order", () => {
  const g = (id: string, lastPlayed: number | null, installed = true) => ({ id, title: id, installed, lastPlayed }) as unknown as Game;
  it("recent games that aren't running, newest first, four at most", () => {
    const games = [g("a", 10), g("b", 50), g("c", 30), g("d", 40), g("e", 20), g("f", null), g("x", 99, false), g("r", 100)];
    expect(recentNotRunning(games, ["r"]).map((x) => x.id)).toEqual(["b", "d", "c", "e"]);
  });
  it("running cards follow the native most-recently-used order; unknown ones last", () => {
    const items = [{ rid: "a" }, { rid: "b" }, { rid: "c" }];
    expect(inOrder(items, ["c", "a"]).map((x) => x.rid)).toEqual(["c", "a", "b"]);
    expect(inOrder(items, []).map((x) => x.rid)).toEqual(["a", "b", "c"]);
  });
});

describe("quick resume defaults (what the UI tells the freezer)", () => {
  const sui = { slug: "warped", name: "Warped Universe", collections: [], platforms: ["windows"] } as unknown as SuiGame;
  const lib = { games: [{ id: "steam:999", title: "Warped Universe", launch: { kind: "steam", appid: 999 } }, { id: "shortcut:5", title: "Hades", launch: { kind: "steamShortcut" } }] } as unknown as Library;
  it("a Sui game is online", () => {
    expect(onlineFor("steam:999", lib, [sui])).toBe(true);
  });
  it("a Steam game with anti-cheat is online (AreWeAntiCheatYet's list)", () => {
    // 730 (Counter-Strike) is VAC in the list.
    expect(onlineFor("steam:730", null, [])).toBe(true);
  });
  it("a single-player game is not", () => {
    expect(onlineFor("steam:1145360", null, [])).toBe(false);
    expect(onlineFor("shortcut:5", lib, [])).toBe(false);
    expect(onlineFor("local:celeste", null, [])).toBe(false);
  });
  it("finds the Steam app id from the run id or the library", () => {
    expect(steamAppId("steam:620", null)).toBe(620);
    expect(steamAppId("shortcut:5", lib)).toBe(null);
  });
  it("says what the switch does", () => {
    expect(backgroundLine({ pause: true, why: "default" })).toMatch(/^Pauses when you switch away/);
    expect(backgroundLine({ pause: false, why: "online" })).toMatch(/online games/);
    expect(backgroundLine({ pause: false, why: "app" })).toMatch(/apps/);
    expect(backgroundLine({ pause: false, why: "you" })).toBe("Keeps running when you switch away.");
  });
});

describe("playing for", () => {
  const now = 10_000_000;
  it("reads like the model's game card", () => {
    expect(playingFor(null, now)).toBe(null);
    expect(playingFor(now - 20_000, now)).toBe("Just started");
    expect(playingFor(now - 42 * 60_000, now)).toBe("Playing for 42 min");
    expect(playingFor(now - 65 * 60_000, now)).toBe("Playing for 1 h 5 min");
    expect(playingFor(now - 120 * 60_000, now)).toBe("Playing for 2 h");
  });
});

describe("the Rare button acts on the toast that is up", () => {
  const t = (kind: string, at: number) => ({ notice: { kind }, at });
  it("a party invite on screen opens Friends with Join", () => {
    expect(inviteUp([t("party_invite", 1000)], 5000, 6000)).toBe(true);
  });
  it("not once it has gone, and not for other notices", () => {
    expect(inviteUp([t("party_invite", 1000)], 8000, 6000)).toBe(false);
    expect(inviteUp([t("sale", 1000), t("friend_online", 4000)], 5000, 6000)).toBe(false);
    expect(inviteUp([], 5000, 6000)).toBe(false);
  });
});
