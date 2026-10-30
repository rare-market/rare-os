// Rare Trophies types. Steam shapes mirror src-tauri/src/trophies/steam.rs; chain shapes mirror the
// rare_trophies Move package (rare-trophies/sources: trophy.move, game.move, profile.move).

export interface SteamAchievement {
  /** Steam's API name, the key the trophy registry maps to a trophy id */
  apiName: string;
  name: string;
  description: string;
  iconUrl: string | null;
  iconGrayUrl: string | null;
  hidden: boolean;
  unlocked: boolean;
  /** unix seconds; null when locked or unlocked before Steam kept times */
  unlockTime: number | null;
  /** share of Steam players who have it, when Steam's library cache knows */
  globalPercent: number | null;
}

export interface SteamGame {
  appid: number;
  /** empty when only the library cache had data: the UI uses the library title */
  name: string;
  /** SteamID64 as a decimal string */
  steamId: string;
  achievements: SteamAchievement[];
  total: number;
  unlocked: number;
  lastUnlock: number | null;
  source: "stats" | "librarycache";
}

/** A new local unlock the native watcher saw. */
export interface NewUnlock {
  appid: number;
  gameName: string;
  apiName: string;
  name: string;
  description: string;
  iconUrl: string | null;
  unlockTime: number | null;
}

export type Tier = 0 | 1 | 2 | 3;
export const TIER_NAME: Record<Tier, string> = { 0: "Bronze", 1: "Silver", 2: "Gold", 3: "Platinum" };
export const TIER_POINTS: Record<Tier, number> = { 0: 15, 1: 30, 2: 90, 3: 300 };

/** 0 studio issued (the game's studio minted it), 1 self-reported (Rare attested local data). */
export type TrophySource = 0 | 1;
export const SOURCE_NAME: Record<TrophySource, string> = { 0: "Studio issued", 1: "Self-reported" };

/** A soulbound `trophy::Trophy` the player owns. */
export interface ChainTrophy {
  objectId: string;
  /** `Game` object id */
  game: string;
  gameName: string;
  trophyId: number;
  name: string;
  description: string;
  imageUrl: string;
  tier: Tier;
  points: number;
  source: TrophySource;
  /** this player was the n-th to unlock it */
  number: number;
  unlockedAtMs: number;
}

/** `game::TrophyDef`, with its id. */
export interface ChainDef {
  trophyId: number;
  key: string;
  name: string;
  description: string;
  imageUrl: string;
  tier: Tier;
  points: number;
  hidden: boolean;
  inPlatinumSet: boolean;
  /** players who unlocked it */
  unlocked: number;
}

/** A shared `game::Game` and its trophy list. */
export interface ChainGame {
  id: string;
  slug: string;
  name: string;
  attested: boolean;
  paused: boolean;
  /** distinct players with at least one trophy: rarity = def.unlocked / players */
  players: number;
  platinum: number | null;
  defs: ChainDef[];
}

/** A shared `profile::Profile`: the gamerscore. */
export interface ChainProfile {
  id: string;
  points: number;
  trophies: number;
  bronze: number;
  silver: number;
  gold: number;
  platinum: number;
  games: number;
  selfReported: number;
  selfReportedPoints: number;
}
