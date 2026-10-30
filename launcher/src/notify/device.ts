// This launcher's device id: random, made once and kept in local storage. Price alerts belong to
// it, and the stream and recent calls pass it so device-only notices (a price alert) arrive.

import { readLocal, writeLocal } from "./store";

const K_DEVICE = "rare.notify.device";

let device: string | null = null;
/** A random id made once per launcher install (kept in local storage); price alerts belong to it. */
export function deviceId(): string {
  if (device) return device;
  const saved = readLocal<unknown>(K_DEVICE, null);
  if (typeof saved === "string" && /^[A-Za-z0-9_-]{16,}$/.test(saved)) return (device = saved);
  const bytes = new Uint8Array(18);
  crypto.getRandomValues(bytes);
  device = btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  writeLocal(K_DEVICE, device);
  return device;
}
