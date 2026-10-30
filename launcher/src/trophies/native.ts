// Native side of Rare Trophies (src-tauri/src/trophies): Steam achievements from this device's
// Steam files, the new-unlock watcher, and the attestation POST (rare.market sends no CORS
// headers, so the Rust side makes it). In a plain browser (UI dev, screenshots) mock.ts answers.

import { isTauri } from "../lib/bridge";
import { mockAttest, mockSteam } from "./mock";
import type { NewUnlock, SteamGame } from "./types";

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export interface HttpReply {
  status: number;
  body: unknown;
}

export const trophiesNative = {
  /** Every game with achievements for the Steam user signed in on this device. */
  steam: (): Promise<SteamGame[]> => (isTauri ? invoke<SteamGame[]>("trophies_steam") : Promise.resolve(mockSteam())),
  /** One game, read fresh from disk. */
  steamApp: (appid: number): Promise<SteamGame | null> =>
    isTauri ? invoke<SteamGame | null>("trophies_steam_app", { appid }) : Promise.resolve(mockSteam().find((g) => g.appid === appid) ?? null),
  /** POST https://rare.market/v1/trophies/attest */
  attest: (body: unknown): Promise<HttpReply> => (isTauri ? invoke<HttpReply>("trophy_attest", { body }) : mockAttest(body)),
};

/** New local Steam unlocks, as the native watcher sees them. */
export async function onSteamUnlocks(cb: (unlocks: NewUnlock[]) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ unlocks: NewUnlock[] }>("steam-achievements", (e) => cb(e.payload.unlocks));
}
