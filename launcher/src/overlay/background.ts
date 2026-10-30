// What the native freezer (freeze.rs) can't know by itself: which running games are online. A Sui
// game (on rare.market's catalog) holds live server and chain sessions, and a Steam game with
// anti-cheat (AreWeAntiCheatYet's list, the same one the game page uses) is an online game whose
// anti-cheat would drop a stalled client. Those keep running in the background by default; the
// rest pause when you switch away. Also how long each running game has been playing, for the
// Rare menu's game card.

import ANTICHEAT from "../lib/anticheat.json";
import { loadGames, type SuiGame } from "../lib/rare";
import { useApp } from "../lib/store";
import type { Library } from "../lib/types";
import { catalogFor } from "./game";
import { overlayNative } from "./native";

const AC = ANTICHEAT as unknown as Record<string, [string, string[]]>;

/** The Steam app id of a run id or library game, if it is a Steam game. */
export function steamAppId(rid: string, library: Library | null): number | null {
  const m = /^steam:(\d+)$/.exec(rid);
  if (m) return Number(m[1]);
  const g = library?.games.find((x) => x.id === rid);
  return g?.launch?.kind === "steam" && g.launch.appid ? Number(g.launch.appid) : null;
}

/** Online: a Sui game, or a Steam game with anti-cheat. Apps, streams and web games are decided natively. */
export function onlineFor(rid: string, library: Library | null, games: SuiGame[]): boolean {
  if (catalogFor(rid, library, games)) return true;
  const appid = steamAppId(rid, library);
  return appid != null && !!AC[String(appid)];
}

const started = new Map<string, number>();

/** When a running game started, as far as this launcher saw it (null after a launcher restart). */
export function startedAt(rid: string): number | null {
  return started.get(rid) ?? null;
}

/** "Playing for 42 min", "Playing for 1 h 5 min"; null when unknown. */
export function playingFor(since: number | null, now = Date.now()): string | null {
  if (since == null) return null;
  const min = Math.max(0, Math.floor((now - since) / 60_000));
  if (min < 1) return "Just started";
  if (min < 60) return `Playing for ${min} min`;
  const h = Math.floor(min / 60);
  const m = min % 60;
  return m ? `Playing for ${h} h ${m} min` : `Playing for ${h} h`;
}

let on = false;

export function startBackground() {
  if (on) return;
  on = true;
  let games: SuiGame[] = [];
  const noted = new Map<string, boolean>();
  // Running before this launcher started: how long it has been playing is unknown.
  const before = new Set(useApp.getState().running);
  const sync = (running: string[]) => {
    const lib = useApp.getState().library;
    for (const rid of running) {
      if (!started.has(rid) && !before.has(rid)) started.set(rid, Date.now());
      const online = onlineFor(rid, lib, games);
      if (noted.get(rid) !== online) {
        noted.set(rid, online);
        void overlayNative.note(rid, online).catch(() => {});
      }
    }
    for (const rid of [...started.keys()]) if (!running.includes(rid)) started.delete(rid);
    for (const rid of [...before]) if (!running.includes(rid)) before.delete(rid);
  };
  sync(useApp.getState().running);
  void loadGames()
    .then((r) => {
      games = r.games;
      sync(useApp.getState().running);
    })
    .catch(() => {});
  useApp.subscribe((s, prev) => {
    if (s.running !== prev.running || s.library !== prev.library) sync(s.running);
  });
}
