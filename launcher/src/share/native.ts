// Native side of Share (src-tauri: screenshots and the 30 second clip buffer). Guide + Y and
// Guide + Select are caught natively and arrive here as "share-captured". In a plain browser
// (npm run dev:ui, screenshots) ./mock answers instead.

import { isTauri } from "../lib/bridge";

export type CaptureKind = "screenshot" | "clip";

export interface Capture {
  id: string;
  kind: CaptureKind;
  /** where the file is on this device */
  path: string;
  mime: string;
  bytes: number;
  width: number;
  height: number;
  durationMs: number | null;
  /** unix ms */
  createdMs: number;
  /** the library game that had the screen, when the native side knows */
  game: { id: string; name: string } | null;
}

export interface ShareStatus {
  session: "gamescope" | "kde" | "desktop" | "mac" | "other";
  screenshot: { available: boolean; method: string | null };
  clips: { available: boolean; enabled: boolean; recording: boolean; method: string | null; seconds: number };
  /** the folder captures are saved in */
  dir: string;
}

type Unlisten = () => void;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

const mock = () => import("./mock");

export const shareNative = {
  status: async (): Promise<ShareStatus> => (isTauri ? call("share_status") : (await mock()).status()),
  setClips: async (enabled: boolean): Promise<unknown> => (isTauri ? call("share_set_clips", { enabled }) : (await mock()).setClips(enabled)),
  screenshot: async (): Promise<Capture> => (isTauri ? call("share_screenshot") : (await mock()).screenshot()),
  /** Saves the last 30 seconds from the clip buffer. */
  clip: async (): Promise<Capture> => (isTauri ? call("share_clip") : (await mock()).clip()),
  /** Newest first. */
  list: async (): Promise<Capture[]> => (isTauri ? call("share_list") : (await mock()).list()),
  /** The file's bytes (a raw tauri::ipc::Response, no JSON). */
  read: async (id: string): Promise<Uint8Array> => {
    const buf = isTauri ? await call<ArrayBuffer>("share_read", { id }) : await (await mock()).read(id);
    return new Uint8Array(buf);
  },
  remove: async (id: string): Promise<void> => (isTauri ? call("share_delete", { id }) : (await mock()).remove(id)),
};

/** A screenshot or clip was saved (by a chord or a command). */
export async function onCaptured(cb: (c: Capture) => void): Promise<Unlisten> {
  if (!isTauri) return (await mock()).onCaptured(cb);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<Capture>("share-captured", (e) => cb(e.payload));
}

/** A chord or the clip buffer failed (no recorder, disk full). */
export async function onShareError(cb: (message: string) => void): Promise<Unlisten> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ message: string }>("share-error", (e) => cb(e.payload.message));
}
