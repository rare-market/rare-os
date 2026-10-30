import { describe, expect, it } from "vitest";
import type { Game } from "../lib/types";
import { INVITE_PARAM, joinPlan, joinUrl } from "./join";
import type { GameRef } from "./types";

const art = { cover: [], hero: [], logo: [], header: [] };
const game = (g: Partial<Game> & Pick<Game, "id" | "source" | "title">): Game => ({ installed: true, art, origin: "", foreign: false, ...g });
const act = game({ id: "web:act", source: "web", title: "ACT", launch: { kind: "web", url: "https://play.actuniverse.com/" } });
const balatro = game({ id: "steam:2379780", source: "steam", title: "Balatro", launch: { kind: "steam", appid: 2379780 } });

describe("joining a friend's game", () => {
  it("starts the same game when this device has it", () => {
    const ref: GameRef = { id: "web:act", title: "ACT", kind: "web", url: "https://play.actuniverse.com/" };
    expect(joinPlan(ref, [act, balatro])).toEqual({ kind: "launch", game: act, url: null });
    expect(joinPlan({ id: "steam:2379780", title: "Balatro", kind: "steam", appid: 2379780 }, [act, balatro])).toEqual({ kind: "launch", game: balatro, url: null });
  });

  it("finds a Steam game by app id even under another id", () => {
    const other = game({ id: "steam:x", source: "steam", title: "Balatro", launch: { kind: "steam", appid: 2379780 } });
    expect(joinPlan({ id: "steam:2379780", title: "Balatro", kind: "steam", appid: 2379780 }, [other])).toMatchObject({ kind: "launch", game: other });
  });

  it("opens Steam's page for a Steam game this device does not have", () => {
    expect(joinPlan({ id: "steam:1145360", title: "Hades II", kind: "steam", appid: 1145360 }, [act])).toEqual({ kind: "steam-store", appid: 1145360 });
  });

  it("opens a web game it does not have at the friend's URL", () => {
    expect(joinPlan({ id: "web:new", title: "New", kind: "web", url: "https://new.game/" }, [])).toEqual({ kind: "web", url: "https://new.game/" });
  });

  it("adds the party invite for web games that take one, and only those", () => {
    const ref: GameRef = { id: "web:squads", title: "Squads", kind: "web", url: "https://squads.game/play?x=1", join: "p_abc" };
    expect(joinUrl(ref)).toBe("https://squads.game/play?x=1");
    INVITE_PARAM.squads = "party";
    expect(joinUrl(ref)).toBe("https://squads.game/play?x=1&party=p_abc");
    const own = game({ id: "web:squads", source: "web", title: "Squads", launch: { kind: "web", url: "https://squads.game/play?x=1" } });
    expect(joinPlan(ref, [own])).toEqual({ kind: "launch", game: own, url: "https://squads.game/play?x=1&party=p_abc" });
    delete INVITE_PARAM.squads;
    // ACT keeps its parties in its own accounts: no invite parameter.
    expect(INVITE_PARAM.act).toBeUndefined();
  });

  it("says when a game cannot be joined from here", () => {
    expect(joinPlan({ id: "epic:fortnite", title: "Fortnite", kind: "other" }, [])).toEqual({ kind: "missing" });
  });
});
