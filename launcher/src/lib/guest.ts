// Guest mode: Rare running as a shortcut inside someone else's Steam Game Mode, where Steam owns
// the Guide and Quick buttons. There the Rare menu is a View hold and the
// hints say so; on Rare OS the Rare and Quick buttons are the way. The native side detects the mode
// (overlay/guest.rs: a SteamLaunch parent, or a gamescope session that isn't Rare's own) and
// `startGuest` asks it once at start; ?guest=1 and localStorage "rare.guest" force it for dev and
// screenshots.

import { isTauri } from "./bridge";

let native = false;

/** Asks the native side once (Tauri only); hints rendered after this read the answer. */
export async function startGuest(): Promise<void> {
  if (!isTauri) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    native = await invoke<boolean>("guest_mode");
  } catch {
    native = false;
  }
}

// Asked as soon as anything imports this (the store does, at start).
void startGuest();

export function isGuest(): boolean {
  try {
    if (native) return true;
    if (new URLSearchParams(location.search).get("guest") === "1") return true;
    return localStorage.getItem("rare.guest") === "1";
  } catch {
    return false;
  }
}

/** The way back to Rare from a game, as a sentence start: "Press the Rare button" or, in guest mode, "Hold View". */
export const rareWay = () => (isGuest() ? "Hold View" : "Press the Rare button");
