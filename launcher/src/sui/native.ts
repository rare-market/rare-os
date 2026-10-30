// Native side of cloud saves (src-tauri/src/sui/saves.rs): find save folders, pack one into a
// tar.gz, restore one. In a plain browser (UI dev, screenshots) a small mock answers instead.

import { isTauri } from "../lib/bridge";

export interface SaveSet {
  /** stable id: "rpcs3", "retroarch-saves", "webgame:act", "emudeck-<name>" */
  id: string;
  emulator: string;
  label: string;
  kind: "emulator" | "webgame";
  /** path relative to the home folder, the same on every Rare OS device */
  rel: string;
  path: string;
  files: number;
  bytes: number;
  /** newest file, unix ms */
  modified: number | null;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown> | Uint8Array, headers?: Record<string, string>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args as never, headers ? { headers } : undefined);
}

/** base64url, no padding, of the UTF-8 text (the header encoding saves.rs expects). */
export function b64url(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

const day = 86_400_000;
const MOCK: SaveSet[] = [
  { id: "retroarch-saves", emulator: "RetroArch", label: "Save files", kind: "emulator", rel: ".config/retroarch/saves", path: "~/.config/retroarch/saves", files: 214, bytes: 9_400_000, modified: Date.now() - 2 * 3600_000 },
  { id: "rpcs3", emulator: "RPCS3", label: "Save data", kind: "emulator", rel: ".config/rpcs3/dev_hdd0/home/00000001/savedata", path: "~/.config/rpcs3/dev_hdd0/home/00000001/savedata", files: 38, bytes: 61_000_000, modified: Date.now() - 3 * day },
  { id: "pcsx2-memcards", emulator: "PCSX2", label: "Memory cards", kind: "emulator", rel: ".config/PCSX2/memcards", path: "~/.config/PCSX2/memcards", files: 2, bytes: 16_777_216, modified: Date.now() - day },
  { id: "duckstation-memcards", emulator: "DuckStation", label: "Memory cards", kind: "emulator", rel: ".local/share/duckstation/memcards", path: "~/.local/share/duckstation/memcards", files: 6, bytes: 786_432, modified: Date.now() - 9 * day },
  { id: "cemu", emulator: "Cemu", label: "Save data", kind: "emulator", rel: ".local/share/Cemu/mlc01/usr/save", path: "~/.local/share/Cemu/mlc01/usr/save", files: 12, bytes: 2_100_000, modified: Date.now() - 20 * day },
  { id: "webgame:act", emulator: "Web game", label: "ACT", kind: "webgame", rel: ".local/share/rare/webgames/act", path: "~/.local/share/rare/webgames/act", files: 9, bytes: 410_000, modified: Date.now() - 5 * 3600_000 },
];

export const savesNative = {
  detect: (): Promise<SaveSet[]> => (isTauri ? invoke<SaveSet[]>("saves_detect") : Promise.resolve(MOCK)),
  /** tar.gz of a detected save set (by id only: the native side never packs an arbitrary path) */
  pack: async (id: string): Promise<Uint8Array> => {
    if (!isTauri) return new TextEncoder().encode(`mock archive for ${id}`);
    const r = await invoke<ArrayBuffer | number[]>("saves_pack", { id });
    return r instanceof ArrayBuffer ? new Uint8Array(r) : new Uint8Array(r);
  },
  /** Unpacks into the save set's folder; an existing folder is moved aside first. Returns where it went ("" if nothing was there). */
  restore: (id: string, rel: string, archive: Uint8Array): Promise<string> =>
    isTauri ? invoke<string>("saves_restore", archive, { "x-rare-id": b64url(id), "x-rare-rel": b64url(rel) }) : Promise.resolve(""),
};
