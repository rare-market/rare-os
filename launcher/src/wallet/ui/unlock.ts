// The PIN prompt as a promise: anything that needs the vault open (signing with a phrase or
// device key, revealing the phrase, adding a local key) awaits requestUnlock(); the global
// UnlockSheet (WalletHost) shows the pad and resolves it.

import { create } from "zustand";
import { keystore } from "../index";

interface Pending {
  reason: string;
  resolve: (ok: boolean) => void;
}

export const useUnlock = create<{ pending: Pending | null }>(() => ({ pending: null }));

/** Resolves true once the vault is unlocked (immediately if it already is), false if cancelled. */
export async function requestUnlock(reason = "Enter your PIN to continue"): Promise<boolean> {
  if (await keystore.isUnlocked()) return true;
  return new Promise((resolve) => {
    useUnlock.getState().pending?.resolve(false);
    useUnlock.setState({ pending: { reason, resolve } });
  });
}

/** Called by the UnlockSheet. */
export function finishUnlock(ok: boolean) {
  const p = useUnlock.getState().pending;
  useUnlock.setState({ pending: null });
  p?.resolve(ok);
}

/** Unlocks for `address` only when its kind needs the vault (Ledger, Keystone, watch never do). */
export async function ensureUnlockedFor(address: string, reason?: string): Promise<boolean> {
  if (!(await keystore.needsUnlock(address))) return true;
  return requestUnlock(reason);
}
