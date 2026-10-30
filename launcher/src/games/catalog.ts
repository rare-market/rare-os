// The Sui Games catalog as the launcher uses it: rare.market's live games.json (statuses are
// proven there and flow in without a launcher release) plus the launcher's own per-game
// profiles (src/games/profiles.json): where a web game really plays, its controller mode,
// and where native builds come from.

import PROFILES from "./profiles.json";
import type { SuiGame } from "../lib/rare";
import type { Game, Library } from "../lib/types";

export type InputMode = "keys" | "gamepad" | "focus";

export interface NativeInfo {
  store: string;
  storeUrl: string;
  steamAppIds?: number[];
  titles?: string[];
  how: string;
}

export interface Profile {
  title?: string;
  playUrl?: string | null;
  hosts?: string[];
  input?: InputMode;
  pointer?: boolean;
  keys?: Record<string, string>;
  controls?: string;
  builtin?: boolean;
  browser?: "chrome";
  unavailable?: string;
  native?: NativeInfo;
  genre?: string;
  description?: string;
}

const ALL = (PROFILES as unknown as { games: Record<string, Profile> }).games;

export const profile = (slug: string): Profile | undefined => ALL[slug.replace(/^web:/, "")];

/** The profile for a library web game: by id, then by the host of its URL. */
export function profileForUrl(id: string, url?: string | null): Profile | undefined {
  const byId = profile(id);
  if (byId) return byId;
  if (!url) return undefined;
  try {
    const host = new URL(url).hostname.replace(/^www\./, "");
    return Object.values(ALL).find((p) => p.hosts?.some((h) => h.replace(/^www\./, "") === host));
  } catch {
    return undefined;
  }
}

/** live and beta are shown as games; everything else waits in "Coming". */
export const isActive = (g: Pick<SuiGame, "status">) => g.status === "live" || g.status === "beta";

export const STATUS_LABEL: Record<string, string> = {
  live: "Live",
  beta: "Beta",
  coming: "Coming soon",
  announced: "Announced",
  paused: "Paused",
  dead: "Offline",
};

/** Where a catalog game plays in the browser, or null. The profile wins over the catalog. */
export function playUrl(g: SuiGame & { playUrl?: string | null }): string | null {
  const p = profile(g.slug);
  if (p && "playUrl" in p) return p.playUrl ?? null;
  if (g.playUrl && /^https:\/\//.test(g.playUrl)) return g.playUrl;
  if (g.platforms.includes("web") && /^https:\/\//.test(g.website)) return g.website;
  return null;
}

/** Why a web game can't be played right now, when the profile knows. */
export const unavailable = (slug: string): string | null => profile(slug)?.unavailable ?? null;

const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, "");

/** The installed library game for a native Sui title: by Steam app id, then by title. */
export function installedNative(slug: string, name: string, library: Library | null): Game | null {
  const n = profile(slug)?.native;
  const games = (library?.games ?? []).filter((g) => g.installed && g.source !== "web");
  const ids = new Set((n?.steamAppIds ?? []).map((i) => `steam:${i}`));
  const byId = games.find((g) => ids.has(g.id));
  if (byId) return byId;
  const titles = new Set([name, ...(n?.titles ?? [])].map(norm));
  return games.find((g) => titles.has(norm(g.title))) ?? null;
}

/** Games that ship with the launcher, as catalog entries (when rare.market's list lacks them). */
export function builtinCatalog(): SuiGame[] {
  return Object.entries(ALL)
    .filter(([, p]) => p.builtin && p.playUrl)
    .map(([slug, p]) => ({
      slug,
      name: p.title ?? slug.toUpperCase(),
      studio: "",
      genre: p.genre ?? "",
      status: "live" as const,
      platforms: ["web" as const],
      website: p.playUrl ?? "",
      x: "",
      description: p.description ?? "",
      logoUrl: `games/${slug}/icon.svg`,
      bannerUrl: `games/${slug}/hero.jpg`,
      collections: [],
      token: null,
      featured: true,
    }));
}

/** The catalog plus built-in games it doesn't list yet. */
export function withBuiltins(games: SuiGame[]): SuiGame[] {
  const have = new Set(games.map((g) => g.slug));
  return [...games, ...builtinCatalog().filter((b) => !have.has(b.slug))];
}
