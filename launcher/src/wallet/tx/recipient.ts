// Recipient field: parse a 0x address or SuiNS name and combine it with the name lookups into
// one field state. Parsing is pure; resolveRecipient() does the gRPC lookups for screens that
// don't want to wire the pieces themselves.

import { nameOf, nameToAddress } from "../data/names";

export type Recipient =
  | { kind: "empty" }
  /** full 32-byte address, lowercased */
  | { kind: "address"; address: string }
  /** a SuiNS name in dot form (`alice.sui`, `pay.alice.sui`), as the user wrote it in `display` */
  | { kind: "name"; name: string; display: string }
  | { kind: "invalid"; reason: string };

export type NameLookup = { status: "idle" } | { status: "loading" } | { status: "found"; address: string } | { status: "missing" } | { status: "error" };

export type RecipientState =
  | { s: "empty" }
  /** still typing something that is not valid yet; no error until they pause */
  | { s: "typing" }
  | { s: "invalid"; msg: string }
  | { s: "loading"; name: string }
  /** `suins` is the name typed, or the address's default name */
  | { s: "ready"; address: string; suins: string | null; typed: "address" | "name" }
  | { s: "missing"; name: string; msg: string }
  | { s: "error"; name: string; msg: string };

const ADDRESS = /^0x[0-9a-f]{64}$/;
const LABEL = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/;

export const isAddress = (s: string) => ADDRESS.test(s.trim().toLowerCase());

/** 0x1234...abcd */
export const shortAddress = (a: string) => (a.length > 14 ? `${a.slice(0, 6)}...${a.slice(-4)}` : a);

/** `@alice` -> `alice.sui`, `pay@alice` -> `pay.alice.sui`, `Alice.SUI` -> `alice.sui`. Null when not a well formed name. */
export function toDotSui(input: string): string | null {
  const s = input.trim().toLowerCase();
  let labels: string[];
  if (s.includes("@")) {
    const [sub, top, ...rest] = s.split("@");
    if (rest.length || !top) return null;
    labels = [...(sub ? sub.split(".") : []), top];
  } else if (s.endsWith(".sui")) {
    labels = s.slice(0, -4).split(".");
  } else return null;
  if (!labels.length || labels.some((l) => !LABEL.test(l) || l.length > 63)) return null;
  return `${labels.join(".")}.sui`;
}

/** What the user typed into the recipient field. */
export function parseRecipient(input: string): Recipient {
  const s = input.trim();
  if (!s) return { kind: "empty" };
  if (/^0x/i.test(s)) {
    if (!/^0x[0-9a-fA-F]*$/.test(s)) return { kind: "invalid", reason: "Address has characters that are not hex" };
    if (s.length !== 66) return { kind: "invalid", reason: "Address should be 0x and 64 characters" };
    return { kind: "address", address: s.toLowerCase() };
  }
  if (s.includes("@") || /\.sui$/i.test(s)) {
    const name = toDotSui(s);
    return name ? { kind: "name", name, display: s.toLowerCase() } : { kind: "invalid", reason: "Names use letters, numbers and hyphens" };
  }
  return { kind: "invalid", reason: "Enter a 0x address or a .sui name" };
}

/** Combine the parsed input, the forward lookup and the reverse name into one field state. */
export function recipientState(p: Recipient, lookup: NameLookup, reverseName: string | null, paused: boolean): RecipientState {
  switch (p.kind) {
    case "empty":
      return { s: "empty" };
    case "invalid":
      return paused ? { s: "invalid", msg: p.reason } : { s: "typing" };
    case "address":
      return { s: "ready", address: p.address, suins: reverseName, typed: "address" };
    case "name":
      if (lookup.status === "found") return { s: "ready", address: lookup.address, suins: p.name, typed: "name" };
      if (lookup.status === "missing") return { s: "missing", name: p.name, msg: `No address is set for ${p.name}` };
      if (lookup.status === "error") return { s: "error", name: p.name, msg: `Could not look up ${p.name}. Check your connection.` };
      return { s: "loading", name: p.name };
  }
}

/** Parse and look up in one go (the field calls this after the user pauses typing). */
export async function resolveRecipient(text: string): Promise<RecipientState> {
  const p = parseRecipient(text);
  if (p.kind === "address") return recipientState(p, { status: "idle" }, await nameOf(p.address).catch(() => null), true);
  if (p.kind !== "name") return recipientState(p, { status: "idle" }, null, true);
  try {
    const address = await nameToAddress(p.name);
    return recipientState(p, address ? { status: "found", address } : { status: "missing" }, null, true);
  } catch {
    return recipientState(p, { status: "error" }, null, true);
  }
}
