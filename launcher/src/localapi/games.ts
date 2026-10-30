// Which Rare game a running game is, for code that shouldn't load the whole Local API: the
// catalog (plus the developer-mode game) resolved like the overlay does. Used to pick the game
// whose gas budget pays (category "game:<slug>").

import { native } from "../lib/bridge";
import { loadGames, type SuiGame } from "../lib/rare";
import { useApp } from "../lib/store";
import { resolveGame } from "../overlay/game";

/** The developer-mode game (`rare-launcher --dev-game`) isn't in the catalog: its collections come from the command line. */
export async function withDevGame(games: SuiGame[]): Promise<SuiGame[]> {
  const dev = await native.devGame().catch(() => null);
  if (!dev || games.some((g) => g.slug === dev.slug)) return games;
  const game: SuiGame = {
    slug: dev.slug,
    name: dev.title,
    studio: "",
    genre: "",
    platforms: ["web"],
    website: dev.url,
    playUrl: dev.url,
    x: "",
    description: "Developer mode game",
    logoUrl: "",
    bannerUrl: "",
    collections: dev.collections.map((nftType) => ({ nftType, name: nftType.split("::").pop() ?? nftType })),
    token: null,
    featured: false,
  };
  return [...games, game];
}

/** The game a request came from, as the review sheet names it and the sponsor knows it. */
export async function payingGame(gameId: string | null | undefined, title?: string | null): Promise<{ slug: string | null; name: string }> {
  if (!gameId) return { slug: null, name: title || "This game" };
  const games = await withDevGame((await loadGames()).games).catch(() => [] as SuiGame[]);
  const g = resolveGame(gameId, useApp.getState().library, games);
  return { slug: g.slug, name: g.slug ? g.title : title || g.title };
}
