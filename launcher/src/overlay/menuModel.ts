// The Rare menu's and the Switcher's pure parts: which tabs show, where the menu opens, the per-kind
// rows, the items line, and the Switcher's order. Kept apart from the components for the tests.

import { fmtSui } from "../marketplace/format";
import type { Game } from "../lib/types";
import type { OverlayItem } from "./data";

export type MenuTab = "game" | "rare" | "items" | "market" | "trophies" | "friends" | "news";

/** The tabs, in order, for what is under the menu. */
export function menuTabs(o: { overGame: boolean; collections: boolean; trophies: boolean }): MenuTab[] {
  const t: MenuTab[] = [o.overGame ? "game" : "rare"];
  if (o.overGame && o.collections) t.push("items", "market");
  if (o.overGame && o.trophies) t.push("trophies");
  t.push("friends", "news");
  return t;
}

/** Where the menu opens: Game over a game; over the launcher Notifications when any are unread, else Rare. */
export function openingTab(overGame: boolean, unread: number): MenuTab {
  if (overGame) return "game";
  return unread > 0 ? "news" : "rare";
}

/** What is under the menu, for the rows that differ per kind. */
export function gameKind(rid: string | null): "steam" | "app" | "stream" | "web" | "game" | null {
  if (!rid) return null;
  if (rid.startsWith("steam:")) return "steam";
  if (rid.startsWith("app:")) return "app";
  if (rid.startsWith("stream:")) return "stream";
  if (rid.startsWith("web:")) return "web";
  return "game";
}

/** "22 items · worth 98.60 SUI · 9 offers" for your items from this game. */
export function itemsLine(items: OverlayItem[]): string {
  const worth = items.reduce((s, x) => s + BigInt(x.floorMist ?? "0"), 0n);
  const offers = items.filter((x) => x.topOfferMist && BigInt(x.topOfferMist) > 0n).length;
  const parts = [items.length === 1 ? "1 item" : `${items.length} items`];
  if (worth > 0n) parts.push(`worth ${fmtSui(worth, { dp: 2 })} SUI`);
  if (offers) parts.push(offers === 1 ? "1 offer" : `${offers} offers`);
  return parts.join(" · ");
}

/** Up to four recent games that aren't running, newest first. */
export function recentNotRunning(games: Game[], running: string[], n = 4): Game[] {
  return games
    .filter((g) => g.installed && !running.includes(g.id) && (g.lastPlayed ?? 0) > 0)
    .sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0))
    .slice(0, n);
}

/** Running items in the native side's order (most recently used first); any it doesn't list yet after. */
export function inOrder<T extends { rid: string }>(items: T[], order: string[]): T[] {
  const rank = (rid: string) => {
    const i = order.indexOf(rid);
    return i < 0 ? order.length : i;
  };
  return [...items].sort((a, b) => rank(a.rid) - rank(b.rid));
}


/** A notice toast still on screen, as the notify store keeps them. */
export interface ToastLike {
  notice: { kind: string };
  at: number;
}

/**
 * The Rare button acts on the toast that is up: a party invite opens the menu on Friends with Join focused.
 */
export function inviteUp(toasts: ToastLike[], now: number, showMs: number): boolean {
  return toasts.some((t) => t.notice.kind === "party_invite" && now - t.at < showMs);
}
