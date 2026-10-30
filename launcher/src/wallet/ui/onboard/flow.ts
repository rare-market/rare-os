// Small helpers the onboarding pages share: hints, finishing a flow, plain error text.

import { useEffect } from "react";
import { usePadHandler } from "../../../nav/input";
import { keystore } from "../../index";
import { requestUnlock } from "../unlock";
import type { Hint } from "../../../components/Chrome";
import { native } from "../../../lib/bridge";
import { NativeError } from "../../native";
import { useWallet, useWalletHints, useWalletNav } from "../store";

export const H = {
  select: { b: "a", label: "Select" } as Hint,
  back: { b: "b", label: "Back" } as Hint,
  type: { b: "a", label: "Type" } as Hint,
  del: { b: "x", label: "Delete" } as Hint,
  ok: { b: "start", label: "Confirm" } as Hint,
};

/** Bottom-bar hints while this page is on top. */
export function useHints(active: boolean, hints: Hint[]) {
  const sig = hints.map((h) => h.b + h.label).join("|");
  useEffect(() => {
    if (active) useWalletHints.setState({ hints });
  }, [active, sig]); // eslint-disable-line react-hooks/exhaustive-deps
}

/** An account was added: select it, go to the wallet home, and remember sign-in is done. */
export async function finishAdd(address?: string) {
  if (address) await useWallet.getState().select(address);
  else await useWallet.getState().refresh();
  useWalletNav.getState().reset();
  void native.config().then((c) => (c.onboarded || c.onboardingStep ? undefined : native.setConfig({ ...c, onboarded: true }))).catch(() => {});
}

/** Plain words for errors from the keystore and the Rust side. Never includes secrets. */
export function plainError(e: unknown, fallback = "That didn't go through. Try again."): string {
  if (e instanceof NativeError) {
    switch (e.code) {
      case "cancelled":
        return "Cancelled.";
      case "timeout":
        return "That took too long. Try again.";
      case "ble-off":
        return "Bluetooth is off. Turn it on and try again.";
      case "notfound":
        return "No device found. Check that it is connected and try again.";
      case "busy":
        return "The device is busy. Finish what it is showing, then try again.";
      case "network":
        return "Couldn't reach the network. Check your connection and try again.";
      case "wrong-pin":
        return "That PIN is wrong. Try again.";
      case "unsupported":
        return "This is not supported on this device.";
    }
  }
  const m = e instanceof Error ? e.message : typeof e === "string" ? e : "";
  return m && m.length < 140 ? m : fallback;
}

/** The one-line fix for a missing USB permission, from the error's data, when there is one. */
export function udevFix(e: unknown): string | null {
  const d = (e as { data?: Record<string, unknown>; code?: string } | null) ?? null;
  const fix = d?.data?.fix;
  return d?.code === "udev" && typeof fix === "string" ? fix : null;
}

/** B steps back inside a multi-step page; at the first step it falls through to the page stack. */
export function useStepBack(active: boolean, canBack: boolean, back: () => void) {
  usePadHandler((b) => {
    if (b === "b" && canBack) {
      back();
      return true;
    }
    return false;
  }, active);
}

/** "ready": the vault is open; "pin": no vault yet, ask for a new PIN; "cancelled": unlock refused. */
export async function vaultGate(reason?: string): Promise<"ready" | "pin" | "cancelled"> {
  if (!(await keystore.hasVault())) return "pin";
  return (await requestUnlock(reason)) ? "ready" : "cancelled";
}
