import type { WebGame } from "./types";

/** Web games that ship with Rare OS (mirrors builtin_web_games in src-tauri/src/library/mod.rs). */
export const BUILTIN_WEB_GAMES: WebGame[] = [
  { id: "act", title: "ACT", url: "https://play.actuniverse.com/", cover: "games/act/cover.jpg", hero: "games/act/hero.jpg" },
];
