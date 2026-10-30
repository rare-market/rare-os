// Sleep and wake in the shell: auto-sleep after idle, and a quiet refresh after a resume.
//
// Idle sleep: 15 minutes on battery by default, never while charging unless the user sets it. It
// never fires while a game, app or stream runs, while audio plays, or during first run and
// sign-in. Input is whatever reaches the launcher: pointer, keys, touch and the controller.
// Resume: nothing is cleared. The old numbers stay on screen and the live data refreshes once
// the network is back (live.ts), so there is no flash of empty or stale-then-jump state.

import { native, onPad } from "./bridge";
import { resumeLive } from "./live";
import { useApp } from "./store";
import type { Config } from "./types";

export const DEFAULT_IDLE_BATTERY_MIN = 15;
/** Choices shown in Settings; 0 is never. */
export const IDLE_CHOICES = [0, 5, 10, 15, 30, 60] as const;

/** Minutes of idle before sleeping for this power source; 0 means never. */
export function idleLimitMin(cfg: Pick<Config, "sleepIdleBattery" | "sleepIdleAc"> | null, onBattery: boolean): number {
  const v = onBattery ? (cfg?.sleepIdleBattery ?? DEFAULT_IDLE_BATTERY_MIN) : (cfg?.sleepIdleAc ?? 0);
  return Number.isFinite(v) && v > 0 ? Math.floor(v) : 0;
}

export interface SleepInputs {
  idleMs: number;
  limitMin: number;
  /** Games, apps and streams that are running. */
  running: number;
  audioPlaying: boolean;
  /** First run or sign-in is on screen. */
  setup: boolean;
}

export function shouldSleep(i: SleepInputs): boolean {
  if (i.limitMin <= 0 || i.running > 0 || i.audioPlaying || i.setup) return false;
  return i.idleMs >= i.limitMin * 60_000;
}

/** The next label in Settings' cycle. */
export function nextIdleChoice(cur: number): number {
  const i = IDLE_CHOICES.indexOf(cur as (typeof IDLE_CHOICES)[number]);
  return IDLE_CHOICES[(i + 1) % IDLE_CHOICES.length];
}

export function idleLabel(min: number): string {
  return min <= 0 ? "Never" : min >= 60 ? `${min / 60} h` : `${min} min`;
}

let lastActive = Date.now();
let started = false;

export function startSleep() {
  if (started) return;
  started = true;
  const touch = () => {
    lastActive = Date.now();
  };
  for (const ev of ["pointerdown", "pointermove", "keydown", "touchstart", "wheel"]) window.addEventListener(ev, touch, { passive: true });
  void onPad(touch);

  setInterval(() => {
    void (async () => {
      const s = useApp.getState();
      const idleMs = Date.now() - lastActive;
      // Cheap exit first: nothing below runs until the shortest limit could have passed.
      if (idleMs < 5 * 60_000 || s.running.length > 0) return;
      const [cfg, sys] = await Promise.all([native.config(), native.sleepState()]);
      const limitMin = idleLimitMin(cfg, sys.onBattery);
      const setup = s.overlay?.kind === "firstrun" || s.overlay?.kind === "signin";
      if (shouldSleep({ idleMs, limitMin, running: s.running.length, audioPlaying: sys.audioPlaying, setup })) {
        lastActive = Date.now();
        await native.power("suspend").catch(() => {});
      }
    })().catch(() => {});
  }, 30_000);

  void native.onResume(() => {
    touch();
    resumeLive();
    void useApp.getState().refreshDevice();
    void useApp.getState().scan({ quiet: true });
  });
}
