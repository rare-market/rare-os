// Downloads and updates as notices. Steam downloads are read from the appmanifests (native
// steam_downloads) and kept in `useDownloads` for the tile bar and the Rare menu; one that
// completes becomes a System notice ("Download finished"). Emulator releases newer than what is
// installed become a System notice once per release. OS updates are in OsUpdateNotice.tsx.
// Everything here is class System: a toast in the launcher, list only over a game or with DND.

import { create } from "zustand";
import { native } from "../lib/bridge";
import { poll } from "../lib/idle";
import { notifyLocal } from "./stream";
import { readLocal, writeLocal } from "./store";

export interface Download {
  id: string;
  title: string;
  done: number;
  total: number;
  paused: boolean;
}

/** 0..1, or null for an unknown size. */
export const downloadFraction = (d: Pick<Download, "done" | "total">) => (d.total > 0 ? Math.min(1, d.done / d.total) : null);

/** Downloads that were running last poll and are gone now, and had reached the end: they finished (not cancelled). */
export function finishedDownloads(prev: Download[], next: Download[]): Download[] {
  const still = new Set(next.map((d) => d.id));
  return prev.filter((d) => !still.has(d.id) && !d.paused && d.total > 0 && d.done / d.total >= 0.98);
}

/** Emulator releases to announce: available and not announced before ("<id>@<tag>" keys). */
export function emuToAnnounce(id: string, tag: string, available: boolean, announced: string[]): string | null {
  const key = `${id}@${tag}`;
  return available && !announced.includes(key) ? key : null;
}

export const useDownloads = create<{ list: Download[] }>(() => ({ list: [] }));

const K_EMU = "rare.notify.emuAnnounced";
const EMU_EVERY_MS = 6 * 3600_000;

async function pollDownloads() {
  const prev = useDownloads.getState().list;
  const next = await native.steamDownloads().catch(() => prev);
  useDownloads.setState({ list: next });
  for (const d of finishedDownloads(prev, next)) {
    notifyLocal({ kind: "download_done", title: `${d.title} finished downloading`, body: null, action: { kind: "downloads" } });
  }
}

async function checkEmulators() {
  const emus = await native.emus().catch(() => []);
  const announced = readLocal<string[]>(K_EMU, []);
  for (const e of emus) {
    if (!e.path) continue;
    const u = await native.emuCheck(e.id).catch(() => null);
    if (!u) continue;
    const key = emuToAnnounce(e.id, u.tag, u.available, announced);
    if (!key) continue;
    announced.push(key);
    writeLocal(K_EMU, announced.slice(-60));
    notifyLocal({ kind: "emu_update", title: `${e.name} ${u.tag} is available`, body: "Update it in Settings > Emulators.", action: { kind: "emulators" } });
  }
}

let started = false;
/** Called once from startNotify. */
export function startDownloadNotices() {
  if (started) return;
  started = true;
  poll(pollDownloads, { active: 5_000, idle: 30_000, away: 30_000 }, { runNow: true });
  setTimeout(() => void checkEmulators(), 60_000);
  setInterval(() => {
    if (!document.hidden) void checkEmulators();
  }, EMU_EVERY_MS);
}
