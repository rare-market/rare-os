// The Rare menu's and the Switcher's commands on the native side (overlay/switcher.rs, freeze.rs).
// In a plain browser (UI development, screenshots) they answer from mock state: ?paused=<id>[,<id>]
// marks running games as paused.

import { isTauri } from "../lib/bridge";
import { useApp } from "../lib/store";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export interface SwitcherCard {
  rid: string;
  /** The still frame taken when the game was left (a local PNG). */
  frame: string | null;
  /** Frozen: the card says Paused. */
  paused: boolean;
}

export interface SwitcherState {
  /** Running games, most recently used first. */
  cards: SwitcherCard[];
  /** What had the screen under the Switcher; null for the launcher. */
  from: string | null;
  maxPaused: number;
}

export type BackgroundWhy = "default" | "online" | "app" | "webGame" | "stream" | "you";

/** How a game behaves when you switch away from it (freeze.rs Background). */
export interface Background {
  run: string;
  kind: "game" | "webWindow" | "webChrome" | "app" | "stream";
  /** Frozen when you switch away. */
  pause: boolean;
  /** The game page offers "Keep running in the background". */
  canChange: boolean;
  why: BackgroundWhy;
  frozen: boolean;
}

/** Sent when leaving a game would make three paused: the player picks one to close. */
export interface FreezeFull {
  result: "full";
  pending: string;
  frozen: string[];
}

const mockPaused = () => (typeof location === "undefined" ? [] : (new URLSearchParams(location.search).get("paused") ?? "").split(",").filter(Boolean));
const mockKeep: Record<string, boolean | null> = {};

export const overlayNative = {
  switcherState: (): Promise<SwitcherState> =>
    isTauri
      ? call("switcher_state")
      : Promise.resolve({
          cards: [...useApp.getState().running].reverse().map((rid) => ({ rid, frame: null, paused: mockPaused().includes(rid) })),
          from: null,
          maxPaused: 2,
        }),
  /** A Switcher card was picked (null: Home). The UI resumes the game itself after. */
  switch: (to: string | null): Promise<void> => (isTauri ? call("switcher_switch", { to }) : Promise.resolve()),
  keepPending: (): Promise<void> => (isTauri ? call("freeze_keep_pending") : Promise.resolve()),
  background: (id: string): Promise<Background> => {
    if (isTauri) return call("freeze_policy", { id });
    const kind: Background["kind"] = id.startsWith("stream:") ? "stream" : id.startsWith("app:") ? "app" : id.startsWith("web:") ? "webWindow" : "game";
    const fixed = kind === "stream" || kind === "webWindow";
    const keep = mockKeep[id];
    const pause = fixed ? false : keep != null ? !keep : kind === "game";
    return Promise.resolve({ run: id, kind, pause, canChange: !fixed, why: keep != null && !fixed ? "you" : kind === "game" ? "default" : kind === "app" ? "app" : kind === "stream" ? "stream" : "webGame", frozen: mockPaused().includes(id) });
  },
  /** "Keep running in the background": true keeps it running, false pauses it, null is the default. */
  setBackground: (id: string, keep: boolean | null): Promise<void> => {
    if (isTauri) return call("freeze_policy_set", { id, keep });
    mockKeep[id] = keep;
    return Promise.resolve();
  },
  /** Over Steam's own UI: one of its pages (library, store, downloads, settings). */
  steamPage: (page: string): Promise<void> => (isTauri ? call("rare_menu_steam_page", { page }) : (useApp.getState().toast(`Steam ${page}`), Promise.resolve())),
  /** Tells the native side a game is online (a Sui game, anti-cheat), so it keeps running by default. */
  note: (id: string, online: boolean): Promise<void> => (isTauri ? call("freeze_note", { id, online }) : Promise.resolve()),
};

/** The native side asks which paused game to close (freeze.rs Outcome::Full). */
export async function onFreezeFull(cb: (e: FreezeFull) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<FreezeFull>("freeze-full", (e) => cb(e.payload));
}
