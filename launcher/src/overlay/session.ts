// The overlay's link to the native side. The Rust input thread opens it (the Rare button over a
// game: press for the Rare menu, hold for the Switcher) and sends an `overlay` event; Resume or B
// asks the native side to close it, which hands the screen back to the game and sends `overlay`
// again with open false. Pressing the Rare button again in the menu closes it with `home` set: the
// launcher goes Home.
//
// Browser dev and screenshots: ?overlay=<library id or catalog slug> opens it over a mock game,
// with &overlaymode=switcher as the Switcher (the Rare button held) or =quick as Quick Settings (the
// Quick button).

import { create } from "zustand";
import { isTauri } from "../lib/bridge";
import { useApp } from "../lib/store";
import { startBackground } from "./background";

export interface Session {
  open: boolean;
  gameId: string | null;
  web: boolean;
  /** A frame of the game (local PNG) to show dimmed behind the panel. */
  frame: string | null;
  mechanism: "gamescope" | "window" | "dev";
  /** The Rare menu (press the Rare button), the Switcher (hold it, or View + Menu), or Quick Settings (the Quick button). */
  mode: "menu" | "switcher" | "quick" | "power";
  /** Rare runs inside someone else's Steam Game Mode: Steam owns the Rare button there. */
  guest?: boolean;
  /** With open false: closed to go Home (the Rare button pressed again in the menu). */
  home?: boolean;
}

export const useOverlay = create<{ session: Session | null }>(() => ({ session: null }));

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const set = (s: Session | null) => {
  // Closed to go Home: the launcher shows Home, with nothing over it.
  if (s && !s.open && s.home) {
    const app = useApp.getState();
    app.close();
    app.setTab("home");
  }
  useOverlay.setState({ session: s?.open ? s : null });
};

let started = false;

export function startOverlay() {
  if (started) return;
  started = true;
  startBackground();
  if (!isTauri) {
    const q = new URLSearchParams(location.search).get("overlay");
    if (q) {
      const id = q.includes(":") ? q : `web:${q}`;
      const m = new URLSearchParams(location.search).get("overlaymode");
      const mode = m === "switcher" || m === "quick" || m === "power" ? m : "menu";
      setTimeout(() => set({ open: true, gameId: id, web: id.startsWith("web:"), frame: null, mechanism: "dev", mode }), 300);
    }
    return;
  }
  void (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<Session>("overlay", (e) => set(e.payload));
    // The launcher may have reloaded while the overlay was up.
    set(await call<Session | null>("overlay_state").catch(() => null));
  })();
}

/** Back to the game. */
export async function closeOverlay(): Promise<void> {
  if (!isTauri) return set(null);
  try {
    await call("overlay_close");
  } finally {
    // The native side sends the closed state too; don't wait on it.
    set(null);
  }
}

/** Leaves the game under the overlay for Home (Game details, a notice's action): it is left as on a
 * Switcher pick of Home, so it pauses when its setting says so. */
export async function leaveForHome(): Promise<void> {
  const { overlayNative } = await import("./native");
  await overlayNative.switch(null).catch(() => {});
  set(null);
  useApp.getState().setTab("home");
  // The native side's closed event (home) closes launcher sheets; let it land before the caller
  // opens the page it wants.
  await new Promise((r) => setTimeout(r, 80));
}

/** Opens the overlay for a game from inside Rare (the game page), mostly for testing. */
export async function openOverlay(gameId: string): Promise<void> {
  if (!isTauri) return set({ open: true, gameId, web: gameId.startsWith("web:"), frame: null, mechanism: "dev", mode: "menu" });
  set(await call<Session>("overlay_open", { gameId }));
}
