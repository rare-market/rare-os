// Joining a friend's game, and describing the game the player is in for presence and parties.
//
// Web games: the same URL, with the party's invite value in the game's own parameter when the
// game takes one (INVITE_PARAM). ACT has squads and parties, but they live in its own account
// system (its social server invites by ACT account and follows the leader between modes); its URL
// only takes `?go=<mode>` and `#/<scene>`, never a party. So ACT gets the same URL and players
// group up in ACT's party screen. A game that adds an invite parameter goes in INVITE_PARAM.
//
// Steam games: Steam's own join (`steam://joinlobby/<app>/<lobby>/<steamid>`) needs a lobby id
// only the game and Steam know, and would tie a Sui address to a Steam account. So Join starts
// the same game through Steam (`steam://rungameid/<app>`, the library's launch), where the
// friend's in-game Steam invite lands; a game this device does not have opens its Steam page.

import { native } from "../lib/bridge";
import { exitTipOnce, launchAllowed, useApp } from "../lib/store";
import type { Game } from "../lib/types";
import type { GameRef } from "./types";

/** Web game id (library id without `web:`) -> the URL parameter it reads an invite from. */
export const INVITE_PARAM: Record<string, string> = {};

const bare = (id: string) => id.replace(/^web:/, "");

/** The game ref for a library game, with `join` set when the game takes invites and a party is given. */
export function gameRef(g: Game, partyId?: string | null): GameRef {
  if (g.source === "steam" && g.launch?.appid) return { id: g.id, title: g.title, kind: "steam", appid: g.launch.appid };
  const appid = g.source === "steam" ? Number(g.id.replace(/^steam:/, "")) : NaN;
  if (g.source === "steam" && Number.isInteger(appid) && appid > 0) return { id: g.id, title: g.title, kind: "steam", appid };
  if (g.source === "web" && g.launch?.url) {
    const ref: GameRef = { id: g.id, title: g.title, kind: "web", url: g.launch.url };
    if (partyId && INVITE_PARAM[bare(g.id)]) ref.join = partyId;
    return ref;
  }
  return { id: g.id, title: g.title, kind: "other" };
}

/** What the player is running now, for presence (the first running game). */
export function runningRef(partyId?: string | null): GameRef | null {
  const s = useApp.getState();
  const id = s.running[0];
  if (!id) return null;
  const g = s.library?.games.find((x) => x.id === id);
  return g ? gameRef(g, partyId) : null;
}

/** The URL a web game opens with to join: the invite parameter added when the game has one. */
export function joinUrl(ref: GameRef): string | null {
  if (ref.kind !== "web" || !ref.url) return null;
  const param = INVITE_PARAM[bare(ref.id)];
  if (!param || !ref.join) return ref.url;
  const u = new URL(ref.url);
  u.searchParams.set(param, ref.join);
  return u.toString();
}

export type JoinPlan =
  | { kind: "launch"; game: Game; url: string | null }
  | { kind: "web"; url: string }
  | { kind: "steam-store"; appid: number }
  | { kind: "missing" };

/** What Join does for a game ref on this device (pure, for tests). */
export function joinPlan(ref: GameRef, library: Game[]): JoinPlan {
  const own = library.find((g) => g.id === ref.id) ?? (ref.appid ? library.find((g) => g.source === "steam" && (g.launch?.appid === ref.appid || g.id === `steam:${ref.appid}`)) : undefined);
  const url = joinUrl(ref);
  if (own) return { kind: "launch", game: own, url: own.source === "web" && url && url !== own.launch?.url ? url : null };
  if (ref.kind === "web" && url) return { kind: "web", url };
  if (ref.kind === "steam" && ref.appid) return { kind: "steam-store", appid: ref.appid };
  return { kind: "missing" };
}

/** Join: start the friend's game here, the closest way this device can. */
export async function joinGame(ref: GameRef): Promise<void> {
  if (!launchAllowed()) return;
  const app = useApp.getState();
  const plan = joinPlan(ref, app.library?.games ?? []);
  switch (plan.kind) {
    case "launch":
      if (plan.url) {
        await native.webgameOpen(bare(plan.game.id), plan.game.title, plan.url);
        app.toast(`Joining ${ref.title}`);
        exitTipOnce();
      } else await app.launch(plan.game);
      return;
    case "web":
      await native.webgameOpen(bare(ref.id), ref.title, plan.url);
      app.toast(`Joining ${ref.title}`);
      exitTipOnce();
      return;
    case "steam-store":
      await native.steamStore(plan.appid);
      app.toast(`${ref.title} is not on this device. Opening it in Steam`);
      return;
    case "missing":
      app.toast(`${ref.title} is not on this device`, "error");
  }
}
