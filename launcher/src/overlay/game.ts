// Which Sui game is under the overlay, and which collections are its items. rare.market's
// games.json lists every Sui game's collections by Move type; the launcher's profiles say where
// each game plays (web host, Steam app ids, titles), so any library id maps back to a catalog
// entry: "web:<slug>", a web game installed by URL, a Steam app, or a native title.

import { profile, profileForUrl } from "../games/catalog";
import type { SuiGame } from "../lib/rare";
import type { Game, Library } from "../lib/types";
import { canonType } from "../sui/chain";

export interface OverlayCollection {
  nftType: string;
  name: string;
}

export interface OverlayGame {
  /** Library id of the running game, when known. */
  gameId: string | null;
  slug: string | null;
  title: string;
  collections: OverlayCollection[];
  /** Move packages the game's types come from (trophies are other objects from them). */
  packages: string[];
  banner: string | null;
  logo: string | null;
  /** Steam app id when the game runs through Steam (its trophies come from Rare Trophies). */
  steamAppId: number | null;
}

const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

/** The package address of a Move type, canonical. */
export function packageOf(type: string): string {
  return canonType(type).split("::")[0];
}

/** The catalog entry for a library game id. */
export function catalogFor(gameId: string | null, library: Library | null, games: SuiGame[]): SuiGame | null {
  if (!gameId) return null;
  const bySlug = (slug: string) => games.find((g) => g.slug === slug) ?? null;
  // "web:<slug>" from the catalog or a built-in.
  if (gameId.startsWith("web:")) {
    const direct = bySlug(gameId.slice(4));
    if (direct) return direct;
  }
  const lib = library?.games.find((g) => g.id === gameId) ?? null;
  // A web game installed by URL: match the profile by host.
  if (lib?.launch?.url) {
    const p = profileForUrl(gameId, lib.launch.url);
    const hit = p && games.find((g) => profile(g.slug) === p);
    if (hit) return hit;
  }
  // A Steam app: the profiles name each native Sui game's Steam app ids.
  const steam = /^steam:(\d+)$/.exec(gameId)?.[1] ?? (lib?.launch?.appid ? String(lib.launch.appid) : undefined);
  if (steam) {
    const hit = games.find((g) => profile(g.slug)?.native?.steamAppIds?.includes(Number(steam)));
    if (hit) return hit;
  }
  // Anything else by title.
  const title = lib?.title;
  if (title) {
    const t = norm(title);
    const hit = games.find((g) => norm(g.name) === t || profile(g.slug)?.native?.titles?.some((x) => norm(x) === t));
    if (hit) return hit;
  }
  return null;
}

/** Everything the overlay needs to know about the game under it. */
export function resolveGame(gameId: string | null, library: Library | null, games: SuiGame[]): OverlayGame {
  const g = catalogFor(gameId, library, games);
  const lib: Game | null = (gameId && library?.games.find((x) => x.id === gameId)) || null;
  const collections = (g?.collections ?? []).map((c) => ({ nftType: canonType(c.nftType), name: c.name }));
  return {
    gameId,
    slug: g?.slug ?? null,
    title: g?.name ?? lib?.title ?? "Your game",
    collections,
    packages: Array.from(new Set(collections.map((c) => packageOf(c.nftType)))),
    banner: g?.bannerUrl || lib?.art.hero[0] || lib?.art.header[0] || null,
    logo: g?.logoUrl || lib?.art.logo[0] || null,
    steamAppId: Number(/^steam:(\d+)$/.exec(gameId ?? "")?.[1] ?? lib?.launch?.appid ?? (g ? profile(g.slug)?.native?.steamAppIds?.[0] : undefined) ?? 0) || null,
  };
}
