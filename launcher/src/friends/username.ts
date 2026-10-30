// Rare usernames (server: market api/src/usernames). Free, unique, picked by the player.
// Set with the friends device key when it is linked, else with one wallet signature (the review
// sheet shows the message). Availability is checked live while typing.

import { signPersonalMessage } from "../wallet/sign";
import * as api from "./api";
import { normalizeUsername } from "./username-rules";
import type { Identity } from "./display";

export interface Availability {
  username: string;
  available: boolean;
  reason: "invalid" | "reserved" | "profane" | "taken" | "held" | null;
  message: string | null;
  yours?: boolean;
}

export interface Mine {
  username: string;
  claimedAt: number;
  changedAt: number;
  nextChangeAt: number | null;
}

/** The text a wallet signs to set its username (server claim.ts claimText; it rebuilds it to compare). */
export function claimText(p: { address: string; username: string; issued: string }): string {
  return [
    "Rare username",
    "",
    `Set the Rare username @${p.username} for this wallet.`,
    "",
    `Address: ${p.address}`,
    `Username: ${p.username}`,
    `Issued: ${p.issued}`,
    "",
    "This does not move funds, send a transaction or cost gas.",
  ].join("\n");
}

// ---------- mock (?friends=mock) ----------

const mockTaken = new Set(["mira", "kenji", "lowkey", "otter", "vex", "sable", "quill", "rare_fan"]);
const mockSuins: Record<string, string> = { mira: "mira.sui", kenji: "kenji.sui", lowkey: "lowkey.sui" };
function mockCheck(raw: string): Availability {
  const u = normalizeUsername(raw);
  if (["admin", "rare", "support", "mysten", "sui"].includes(u)) return { username: u, available: false, reason: "reserved", message: "That name is not available." };
  if (mockTaken.has(u)) return { username: u, available: false, reason: "taken", message: "Someone already has that name." };
  return { username: u, available: true, reason: null, message: null };
}

// ---------- calls ----------

export async function check(raw: string, address?: string, signal?: AbortSignal): Promise<Availability> {
  if (api.useMock) return new Promise((r) => setTimeout(() => r(mockCheck(raw)), 250));
  const q = new URLSearchParams({ u: normalizeUsername(raw), ...(address ? { address } : {}) });
  void signal;
  return api.send<Availability>(null, "GET", `/v1/usernames/available?${q}`);
}

/** Sets (claims or changes) the username for `address`. `key` is the linked device key, if any. */
export async function set(address: string, raw: string, key: api.DeviceKeyLike | null): Promise<Mine> {
  const username = normalizeUsername(raw);
  if (api.useMock) {
    mockTaken.add(username);
    const now = Date.now();
    return { username, claimedAt: now, changedAt: now, nextChangeAt: null };
  }
  if (key) return api.send<Mine>(key, "PUT", "/v1/usernames/me", { username });
  const issued = new Date().toISOString();
  const message = claimText({ address, username, issued });
  const { signature } = await signPersonalMessage(new TextEncoder().encode(message), {
    account: address,
    origin: { name: "Rare username", kind: "wallet" },
    title: "Pick your username",
  });
  return api.send<Mine>(null, "POST", "/v1/usernames/claim", { address, message, signature });
}

/** Who has this username (null when nobody). */
export async function lookup(raw: string): Promise<(Identity & { avatarUrl: string | null }) | null> {
  const u = normalizeUsername(raw);
  if (api.useMock) return mockTaken.has(u) ? { address: `0x${"ab".repeat(32)}`, username: u, name: u, suins: mockSuins[u] ?? null, avatarUrl: null } : null;
  try {
    const r = await api.send<{ address: string; username: string; avatarUrl: string | null; suins: string | null }>(null, "GET", `/v1/users/${encodeURIComponent(u)}`);
    return { address: r.address, username: r.username, name: r.username, suins: r.suins, avatarUrl: r.avatarUrl };
  } catch (e) {
    if (e instanceof api.ApiError && e.status === 404) return null;
    throw e;
  }
}

/** Sets the signed-in account's username and updates the profile and friends. */
export async function claim(raw: string): Promise<Mine> {
  const { useProfile, currentAddress } = await import("../sui/profile");
  const { currentKey, refresh } = await import("./store");
  const cur = await currentAddress();
  if (!cur) throw new Error("Add a Rare Wallet account first.");
  const address = cur.address.toLowerCase();
  const key = currentKey();
  const m = await set(address, raw, key && key.address === address ? key : null);
  useProfile.setState({ username: m.username });
  void refresh();
  return m;
}
