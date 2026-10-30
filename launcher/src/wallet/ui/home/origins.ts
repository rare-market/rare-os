// Games the player approved for the wallet (web games over the Wallet Standard). The list lives in
// localStorage under "rare.wallet.approvedOrigins"; Rust is told the same list so it stops serving
// the address to a removed origin.

import { webgameWallet as walletNative } from "../../native";

const KEY = "rare.wallet.approvedOrigins";

const originOf = (x: unknown): string | null => (typeof x === "string" ? x : x && typeof x === "object" && typeof (x as { origin?: unknown }).origin === "string" ? (x as { origin: string }).origin : null);

function raw(): unknown[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v : v && typeof v === "object" ? Object.keys(v) : [];
  } catch {
    return [];
  }
}

export const approvedOrigins = (): string[] => raw().map(originOf).filter((o): o is string => !!o);

export async function removeOrigin(origin: string): Promise<void> {
  const rest = raw().filter((x) => originOf(x) !== origin);
  try {
    localStorage.setItem(KEY, JSON.stringify(rest));
  } catch {
    /* storage blocked */
  }
  await walletNative.setApproved(rest.map(originOf).filter((o): o is string => !!o)).catch(() => {});
}

export const hostOf = (origin: string) => origin.replace(/^https?:\/\//, "");
