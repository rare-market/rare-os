// Web-game requests (Wallet Standard in the game window) as pure mappings: request -> the sign
// call to make, sign result -> the reply value. The host (WalletHost) does the side effects.

import { Transaction } from "@mysten/sui/transactions";
import { fromBase64, toBase64 } from "@mysten/sui/utils";
import type { WebGameRequest } from "../../native";
import { SignRejected, type SignResult } from "../../sign";

export type GameCall =
  | { kind: "connect" }
  | { kind: "transaction"; tx: Transaction | Uint8Array; execute: boolean }
  | { kind: "message"; message: Uint8Array };

export type Mapped = { ok: true; call: GameCall } | { ok: false; error: string };

/** The game's transaction: Transaction JSON becomes a Transaction, base64 stays the built bytes. */
export function parseGameTransaction(raw: string): Transaction | Uint8Array {
  const s = raw.trim();
  if (s.startsWith("{")) return Transaction.from(s);
  return fromBase64(s);
}

export function mapGameRequest(req: Pick<WebGameRequest, "method" | "params">): Mapped {
  try {
    switch (req.method) {
      case "connect":
        return { ok: true, call: { kind: "connect" } };
      case "signTransaction":
      case "signAndExecuteTransaction": {
        const raw = req.params?.transaction;
        if (!raw) return { ok: false, error: "The game did not send a transaction" };
        return { ok: true, call: { kind: "transaction", tx: parseGameTransaction(raw), execute: req.method === "signAndExecuteTransaction" } };
      }
      case "signPersonalMessage": {
        const raw = req.params?.message;
        if (!raw) return { ok: false, error: "The game did not send a message" };
        return { ok: true, call: { kind: "message", message: fromBase64(raw) } };
      }
      default:
        return { ok: false, error: "This request is not supported" };
    }
  } catch {
    return { ok: false, error: "Could not read the game's request" };
  }
}

/** What goes back to the game for a finished signature. */
export function replyValue(call: GameCall, r: SignResult): Record<string, unknown> {
  if (call.kind === "message") return { bytes: r.bytes, signature: r.signature };
  const base = { bytes: r.bytes, signature: r.signature };
  if (call.kind === "transaction" && call.execute) return { ...base, digest: r.digest ?? "", effects: (r as { effects?: string }).effects ?? "" };
  return base;
}

/** The error text the game sees (it only gets a string). */
export function errorText(e: unknown): string {
  if (e instanceof SignRejected) return "User rejected the request";
  return e instanceof Error ? e.message : "Request failed";
}

/** Origin a request came from, normalised for the approved list. */
export const normOrigin = (o: string) => o.trim().replace(/\/+$/, "").toLowerCase();

export const messageBase64 = (m: Uint8Array) => toBase64(m);

/** A connect from an origin the player already approved is answered without a sheet: with the selected account
 * (or the first one). Null when a sheet is needed: a new origin, or no account to answer with. */
export function silentConnectAccount<A extends { address: string }>(approved: readonly string[], origin: string, accounts: readonly A[] | null | undefined, current: A | null | undefined): A | null {
  if (!approved.includes(normOrigin(origin))) return null;
  if (!accounts?.length) return null;
  return (current && accounts.find((a) => a.address === current.address)) || accounts[0];
}
