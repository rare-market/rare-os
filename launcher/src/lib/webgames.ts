// Browser games installed as web apps. Stored in the launcher config; the native side
// lists them in the library and opens each in its own sandboxed window.

import { native } from "./bridge";
import { useApp } from "./store";
import type { WebGame } from "./types";
import { BUILTIN_WEB_GAMES } from "./builtins";

export { BUILTIN_WEB_GAMES };

const BUILTIN_IDS = BUILTIN_WEB_GAMES.map((g) => g.id);

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "game";

/** Hosts a developer's own machine answers on: plain http is fine there (testing a game before it is hosted). */
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);
const LOCAL_START = /^(localhost|127\.0\.0\.1|\[::1\])(:\d+)?([/?#]|$)/i;

/** An address the web game window may open: https anywhere, http only on localhost, 127.0.0.1 and [::1] (any port).
 * A bare local address like "localhost:3000" is read as http; everything else without a scheme as https. */
export function normaliseUrl(input: string): string {
  let u = input.trim();
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(u) && !/^https?:\/\//i.test(u)) throw new Error("Web games need an https address (http works only for localhost)");
  if (!/^https?:\/\//i.test(u)) u = `${LOCAL_START.test(u) ? "http" : "https"}://${u}`;
  const parsed = new URL(u);
  if (parsed.protocol === "http:" && LOCAL_HOSTS.has(parsed.hostname)) return parsed.toString();
  if (parsed.protocol !== "https:") throw new Error("Web games need an https address (http works only for localhost)");
  return parsed.toString();
}

export async function addWebGame(g: Omit<WebGame, "id"> & { id?: string }): Promise<WebGame> {
  const cfg = await native.config();
  const id = g.id ?? slug(g.title);
  const game: WebGame = { id, title: g.title, url: normaliseUrl(g.url), cover: g.cover ?? null, hero: g.hero ?? null };
  const webGames = [...(cfg.webGames ?? []).filter((w) => w.id !== id), game];
  await native.setConfig({ ...cfg, webGames });
  await useApp.getState().scan();
  return game;
}

export async function removeWebGame(id: string) {
  const cfg = await native.config();
  const bare = id.replace(/^web:/, "");
  const removedBuiltins = BUILTIN_IDS.includes(bare) ? [...new Set([...(cfg.removedBuiltins ?? []), bare])] : cfg.removedBuiltins ?? [];
  await native.setConfig({ ...cfg, removedBuiltins, webGames: (cfg.webGames ?? []).filter((w) => w.id !== bare) });
  await useApp.getState().scan();
}
