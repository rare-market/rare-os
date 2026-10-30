// OS updates, launcher side. The image downloads the next version in the background and stages
// it; it applies on the next restart. This keeps what the native side reports (osupdate.rs),
// polls it cheaply, and holds the two sheets' open state. Pure helpers first (tested).

import { create } from "zustand";
import { native } from "./bridge";
import type { OsUpdate } from "./types";

export const NO_UPDATE: OsUpdate = { bootedVersion: null, staged: false, stagedVersion: null, checkedAt: null, checkOk: null, checkError: null };

/** "3 hours ago", "yesterday": when the last check ran, in plain words. */
export function agoText(atSec: number | null, nowMs: number): string {
  if (!atSec) return "not yet";
  const s = Math.max(0, Math.floor(nowMs / 1000 - atSec));
  if (s < 90) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} minutes ago`;
  const h = Math.round(s / 3600);
  if (h < 24) return h === 1 ? "1 hour ago" : `${h} hours ago`;
  const d = Math.round(s / 86400);
  return d === 1 ? "yesterday" : `${d} days ago`;
}

export interface UpdateLine {
  title: string;
  body: string;
}

/** The Updates row's words for a state. `checking` is a check started from this launcher. */
export function updateLine(u: OsUpdate, checking: boolean, nowMs: number): UpdateLine {
  if (u.staged) {
    return {
      title: u.stagedVersion ? `Update ${u.stagedVersion} is ready` : "An update is ready",
      body: "It is downloaded. It installs when the device restarts. Restart now, or keep playing and do it later.",
    };
  }
  if (checking) return { title: "Checking for updates", body: "Downloads in the background. You can keep playing." };
  if (u.checkOk === false) {
    return { title: "Could not check for updates", body: `${u.checkError ?? "The last check did not finish."} It tries again by itself.` };
  }
  return { title: "Rare OS is up to date", body: `Last checked ${agoText(u.checkedAt, nowMs)}. Updates download by themselves and install at the next restart.` };
}

/** The staged version to announce once per launch, or null (nothing staged, or already announced). */
export function toAnnounce(u: OsUpdate, announced: string | null): string | null {
  if (!u.staged) return null;
  const key = u.stagedVersion ?? "staged";
  return key === announced ? null : key;
}

/** A check we started is done when the updater wrote a newer time (or reported a failure). */
export function checkDone(u: OsUpdate, startedSec: number): boolean {
  return u.checkedAt !== null && u.checkedAt >= startedSec;
}

interface State {
  update: OsUpdate;
  loaded: boolean;
  checking: boolean;
  restartOpen: boolean;
  restoreOpen: boolean;
  /** Version already announced in the notification centre this launch ("Later" keeps it quiet until the next launch). */
  announced: string | null;
  refresh: () => Promise<OsUpdate>;
  check: () => Promise<void>;
}

export const useOsUpdate = create<State>((set, get) => ({
  update: NO_UPDATE,
  loaded: false,
  checking: false,
  restartOpen: false,
  restoreOpen: false,
  announced: null,
  refresh: async () => {
    const update = await native.osUpdate().catch(() => get().update);
    set({ update, loaded: true });
    return update;
  },
  check: async () => {
    if (get().checking) return;
    const started = Math.floor(Date.now() / 1000) - 1;
    set({ checking: true });
    try {
      await native.osUpdateCheck();
      // The download runs in the background; poll for the updater's result, at most 15 minutes.
      for (let i = 0; i < 225; i++) {
        const u = await get().refresh();
        if (checkDone(u, started)) break;
        await new Promise((r) => setTimeout(r, 4000));
      }
    } finally {
      set({ checking: false });
    }
  },
}));

export const openOsRestart = () => useOsUpdate.setState({ restartOpen: true });
