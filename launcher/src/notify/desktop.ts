// Desktop notifications through tauri-plugin-notification, for when the launcher is not the
// focused window. A no-op in a browser (dev:ui with the mock native side).

import { isTauri } from "../lib/bridge";
import type { Notice } from "./types";

let tauriFocused: boolean | null = null;
let started = false;

/** Tracks window focus: Tauri's focus events in the app, the document's own in a browser. */
export function watchWindowFocus() {
  if (started) return;
  started = true;
  if (!isTauri) return;
  void import("@tauri-apps/api/window")
    .then(async ({ getCurrentWindow }) => {
      const w = getCurrentWindow();
      tauriFocused = await w.isFocused().catch(() => null);
      await w.onFocusChanged(({ payload }) => {
        tauriFocused = payload;
      });
    })
    .catch(() => {});
}

export function launcherFocused(): boolean {
  if (typeof document === "undefined") return true;
  if (document.visibilityState === "hidden") return false;
  return tauriFocused ?? document.hasFocus();
}

let permission: Promise<boolean> | null = null;
async function allowed(): Promise<boolean> {
  if (!permission) {
    permission = import("@tauri-apps/plugin-notification")
      .then(async ({ isPermissionGranted, requestPermission }) => (await isPermissionGranted()) || (await requestPermission()) === "granted")
      .catch(() => false);
  }
  return permission;
}

export async function sendDesktop(n: Notice): Promise<void> {
  if (!isTauri) return;
  if (!(await allowed())) return;
  try {
    const { sendNotification } = await import("@tauri-apps/plugin-notification");
    sendNotification({ title: n.title, body: n.body ?? undefined });
  } catch {
    /* the desktop has no notification service: the in-app toast is enough */
  }
}
