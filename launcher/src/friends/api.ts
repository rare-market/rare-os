// rare.market's friends API, straight from the webview (CORS is open on /v1/friends, /v1/presence and
// /v1/players, like /v1/notify). Every private request is signed by this device's key (sign.ts).
// `?friends=mock` in a browser swaps in ./mock (sample friends, a party, chat) for UI work and
// screenshots, the same way notify/mock.ts stands in for the notify stream.

import { isTauri } from "../lib/bridge";
import { signPersonalMessage } from "../wallet/sign";
import { deviceKey, linkRequest, markLinked, type DeviceKey } from "./sign";
import { FRIENDS_BASE, type GameRef, type GraphOp, type Message, type Overview, type Party, type Presence, type Privacy } from "./types";
import * as mock from "./mock";

export const useMock = !isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).get("friends") === "mock";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export type DeviceKeyLike = DeviceKey;

const ORIGIN = "https://rare.market";

export async function send<T>(k: DeviceKey | null, method: string, path: string, body?: unknown, ms = 10_000): Promise<T> {
  const raw = body === undefined ? "" : JSON.stringify(body);
  const headers: Record<string, string> = { ...(raw ? { "content-type": "application/json" } : {}), ...(k ? await k.headers(method, path, raw) : {}) };
  const ctl = new AbortController();
  const t = setTimeout(() => ctl.abort(), ms);
  try {
    const r = await fetch(`${ORIGIN}${path}`, { method, headers, body: raw || undefined, signal: ctl.signal });
    const text = await r.text();
    let j: unknown = null;
    try {
      j = text ? JSON.parse(text) : null;
    } catch {
      j = null;
    }
    if (!r.ok) {
      const e = (j as { error?: { code?: string; message?: string } } | null)?.error;
      throw new ApiError(r.status, e?.code ?? "error", e?.message ?? `rare.market ${r.status}`);
    }
    return j as T;
  } finally {
    clearTimeout(t);
  }
}

/**
 * Links this device to the account: Rare Wallet's review sheet shows the message and signs it.
 * Throws SignRejected when the player cancels.
 */
export async function link(address: string): Promise<DeviceKey> {
  const k = deviceKey(address);
  if (useMock) {
    markLinked(k, Date.now() + 90 * 86_400_000);
    return k;
  }
  const { message, expires } = linkRequest(k);
  const { signature } = await signPersonalMessage(new TextEncoder().encode(message), {
    account: k.address,
    origin: { name: "Rare friends", kind: "wallet" },
    title: "Turn on friends",
  });
  const r = await send<{ expiresAt: number }>(null, "POST", "/v1/friends/link", { address: k.address, message, signature });
  markLinked(k, Math.min(expires, r.expiresAt));
  return k;
}

export async function unlink(k: DeviceKey) {
  if (useMock) return;
  await send(k, "DELETE", "/v1/friends/link");
}

export const overview = (k: DeviceKey) => (useMock ? mock.overview() : send<Overview>(k, "GET", "/v1/friends"));

export const graph = (k: DeviceKey, op: GraphOp, address: string) =>
  useMock ? mock.graph(op, address) : send<{ state: string | null }>(k, "POST", `/v1/friends/${op}`, { address });

export const saveSettings = (k: DeviceKey, s: Partial<Privacy>) => (useMock ? mock.settings(s) : send<Privacy>(k, "PUT", "/v1/friends/settings", s));

export const makeCode = (k: DeviceKey) => (useMock ? mock.code() : send<{ code: string; url: string; expiresAt: number }>(k, "POST", "/v1/friends/codes"));

export const redeemCode = (k: DeviceKey, code: string) => (useMock ? mock.redeem(code) : send<{ address: string }>(k, "POST", "/v1/friends/codes/redeem", { code }));

export const heartbeat = (k: DeviceKey, b: { state: "online" | "away" | "offline"; game: GameRef | null; avatar?: { id: string; imageUrl: string } | null }) =>
  useMock ? mock.heartbeat(b) : send<Presence>(k, "POST", "/v1/presence", b);

export const createParty = (k: DeviceKey, game: GameRef | null, invite: string[]) =>
  useMock ? mock.createParty(game, invite) : send<Party>(k, "POST", "/v1/friends/parties", { game, invite });

export const inviteToParty = (k: DeviceKey, id: string, addresses: string[]) =>
  useMock ? mock.invite(id, addresses) : send<Party>(k, "POST", `/v1/friends/parties/${id}/invite`, { addresses });

export const joinParty = (k: DeviceKey, id: string) => (useMock ? mock.join(id) : send<Party>(k, "POST", `/v1/friends/parties/${id}/join`));

export const leaveParty = (k: DeviceKey, id: string) => (useMock ? mock.leave(id) : send<{ ok: true }>(k, "POST", `/v1/friends/parties/${id}/leave`));

export const setPartyGame = (k: DeviceKey, id: string, game: GameRef | null) =>
  useMock ? mock.setGame(id, game) : send<Party>(k, "POST", `/v1/friends/parties/${id}/game`, { game });

export const messages = (k: DeviceKey, id: string) =>
  useMock ? mock.messages(id) : send<{ data: Message[] }>(k, "GET", `/v1/friends/parties/${id}/messages`).then((r) => r.data);

export const postMessage = (k: DeviceKey, id: string, text: string) =>
  useMock ? mock.post(id, text) : send<Message>(k, "POST", `/v1/friends/parties/${id}/messages`, { text });

export { FRIENDS_BASE };
