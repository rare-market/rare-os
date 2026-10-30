// Launch progress for everything running (UX-BUILD item 14): each new run is "steam" (Steam is
// starting), then "game" (started, no window yet), then "playing" once gamescope shows a window
// (launch.rs phase, Rare OS game mode). The stage line on Home reads it; B cancels a start that
// is still going after STILL_STARTING (App.tsx, and lib.rs lets B through while nothing shows).

import { create } from "zustand";
import { native } from "./bridge";
import { useApp } from "./store";

export type Phase = "steam" | "game" | "playing" | "none";
/** After this long without a window the stage says "Still starting. B to cancel." */
export const STILL_STARTING = 15_000;

interface Start {
  phase: Phase;
  since: number;
}

export const useStart = create<{ starts: Record<string, Start> }>(() => ({ starts: {} }));

/** The line the stage shows for a run, or null once it plays. */
export function startLine(title: string, s: Start | undefined, now: number): string | null {
  if (!s || s.phase === "playing" || s.phase === "none") return null;
  if (now - s.since >= STILL_STARTING) return "Still starting. B to cancel.";
  return s.phase === "steam" ? "Starting Steam" : `Starting ${title}`;
}

/** The start B would cancel: the newest one still going after STILL_STARTING. */
export function cancellable(starts: Record<string, Start>, now: number): string | null {
  const slow = Object.entries(starts).filter(([, s]) => (s.phase === "steam" || s.phase === "game") && now - s.since >= STILL_STARTING);
  slow.sort((a, b) => b[1].since - a[1].since);
  return slow[0]?.[0] ?? null;
}

let timer: ReturnType<typeof setInterval> | null = null;

async function poll() {
  const running = useApp.getState().running;
  const prev = useStart.getState().starts;
  const next: Record<string, Start> = {};
  for (const id of running) {
    const was = prev[id];
    if (was?.phase === "playing") {
      next[id] = was;
      continue;
    }
    const phase = await native.launchPhase(id).catch(() => "playing" as Phase);
    next[id] = { phase, since: was?.since ?? Date.now() };
  }
  useStart.setState({ starts: next });
  const busy = Object.values(next).some((s) => s.phase === "steam" || s.phase === "game");
  if (!busy && timer) {
    clearInterval(timer);
    timer = null;
  }
}

/** Starts watching (call once): polls while a run has no window yet. */
export function startProgress() {
  const kick = () => {
    if (!timer) timer = setInterval(() => void poll(), 700);
    void poll();
  };
  useApp.subscribe((s, p) => {
    if (s.running !== p.running) kick();
  });
}
