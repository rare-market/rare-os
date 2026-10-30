// Auto-lock: after N idle minutes the vault locks (keystore.lock) and the UI refreshes. Any key,
// pointer or controller press counts as activity. The choice is kept in localStorage.

import { useEffect } from "react";
import { onPad } from "../../../lib/bridge";
import { keystore } from "../../index";
import { useWallet } from "../store";

const KEY = "rare.wallet.autolockMin";
export const AUTOLOCK_OPTIONS = [0, 1, 5, 15] as const;
const DEFAULT_MIN = 15;

export function getAutoLock(): number {
  try {
    const v = localStorage.getItem(KEY);
    if (v == null) return DEFAULT_MIN;
    const n = Number(v);
    return (AUTOLOCK_OPTIONS as readonly number[]).includes(n) ? n : DEFAULT_MIN;
  } catch {
    return DEFAULT_MIN;
  }
}

export function setAutoLock(min: number) {
  try {
    localStorage.setItem(KEY, String(min));
  } catch {
    /* storage blocked: the choice lasts until reload */
  }
  arm();
}

let users = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let stop: (() => void) | undefined;

async function fire() {
  try {
    if (await keystore.isUnlocked()) {
      await keystore.lock();
      await useWallet.getState().refresh();
    }
  } catch {
    /* nothing to lock */
  }
}

function arm() {
  clearTimeout(timer);
  const min = getAutoLock();
  if (users > 0 && min > 0) timer = setTimeout(() => void fire(), min * 60_000);
}

function attach() {
  const events = ["keydown", "pointerdown", "pointermove", "wheel"] as const;
  const on = () => arm();
  for (const e of events) window.addEventListener(e, on, { passive: true, capture: true });
  const unpad = onPad(() => arm());
  arm();
  return () => {
    for (const e of events) window.removeEventListener(e, on, true);
    void unpad.then((u) => u());
    clearTimeout(timer);
  };
}

/** Starts the idle timer while at least one caller wants it; returns the stop function. */
export function startAutoLock(): () => void {
  if (users++ === 0) stop = attach();
  return () => {
    if (--users === 0) {
      stop?.();
      stop = undefined;
    }
  };
}

export function useAutoLock() {
  useEffect(() => startAutoLock(), []);
}
