// Friends state for the selected Rare Wallet account: the friend list with live presence, requests,
// parties and their chat. Loaded from /v1/friends, kept live by the notify stream's friend events
// (notify/stream.ts dispatches them as `rare:social` window events), refreshed on any graph change.

import { create } from "zustand";
import { keystore } from "../wallet";
import { SignRejected } from "../wallet/sign";
import { devSign } from "../wallet/ui/dev";
import * as api from "./api";
import { nameOf } from "./display";
import { deviceKey, forgetKey, type DeviceKey } from "./sign";
import type { Friend, GameRef, GraphOp, Message, Overview, Party, Pending, Person, Presence, Privacy, SocialEvent } from "./types";
import { kept, persist } from "../lib/state";

export type FriendsStatus =
  /** no Rare Wallet account */
  | "none"
  /** a watch-only account: it cannot sign, so no friends */
  | "watch"
  /** a signing account that has not turned friends on yet on this device */
  | "unlinked"
  | "linking"
  | "loading"
  | "ready"
  | "error";

interface FriendsState {
  status: FriendsStatus;
  error: string | null;
  address: string | null;
  me: Overview["me"] | null;
  friends: Friend[];
  received: Pending[];
  sent: Pending[];
  blocked: Pending[];
  parties: Party[];
  messages: Record<string, Message[]>;
  unread: Record<string, number>;
  /** The party whose chat is on screen (no unread count, no notices for it). */
  openParty: string | null;
  busy: boolean;
  at: number;
}

export const useFriends = create<FriendsState>(() => ({
  status: "none",
  error: null,
  address: null,
  me: null,
  friends: [],
  received: [],
  sent: [],
  blocked: [],
  parties: [],
  messages: {},
  unread: {},
  openParty: null,
  busy: false,
  at: 0,
}));

const set = useFriends.setState;
const get = useFriends.getState;

let key: DeviceKey | null = null;
/** The linked device key for the current account, or null. */
export const currentKey = () => (key?.linked ? key : null);

const listeners = new Set<() => void>();
/** Called when the linked key changes (the notify stream reconnects with a new token). */
export function onKeyChange(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
const keyChanged = () => listeners.forEach((f) => f());

/** Picks up the selected account: linked already, needs linking, or cannot sign. */
export async function syncAccount() {
  const a = await keystore.current().catch(() => null);
  const address = a?.address?.toLowerCase() ?? null;
  if (address === get().address && get().status !== "none") return;
  key = null;
  set({ address, me: null, friends: [], received: [], sent: [], blocked: [], parties: [], messages: {}, unread: {}, error: null });
  if (!a || !address) {
    set({ status: "none" });
    keyChanged();
    return;
  }
  if (a.kind === "watch" && !api.useMock && !devSign) {
    set({ status: "watch" });
    keyChanged();
    return;
  }
  let k = deviceKey(address);
  if (api.useMock && !k.linked) k = await api.link(address);
  if (!k.linked) {
    set({ status: "unlinked" });
    keyChanged();
    return;
  }
  key = k;
  // The last overview from Rust's cache (`ui:friends:<address>`) is on screen at once; refresh() updates
  // it (`at` 1: shown from cache, so a failed refresh keeps it on screen rather than an error).
  const last = kept<Partial<FriendsState>>(`ui:friends:${address}`);
  set(last ? { ...last, status: "ready", at: 1 } : { status: "loading" });
  keyChanged();
  await refresh();
}

/** Turns friends on for this account on this device: one wallet signature. */
export async function turnOn(): Promise<boolean> {
  const address = get().address;
  if (!address) return false;
  set({ status: "linking", error: null });
  try {
    key = await api.link(address);
    set({ status: "loading" });
    keyChanged();
    await refresh();
    return true;
  } catch (e) {
    set({ status: "unlinked", error: e instanceof SignRejected ? null : messageOf(e) });
    return false;
  }
}

/** Signs this device out of friends (the key is revoked on the server and forgotten here). */
export async function turnOff() {
  const k = currentKey();
  const address = get().address;
  if (k) await api.unlink(k).catch(() => undefined);
  if (address) forgetKey(address);
  key = null;
  set({ status: "unlinked", me: null, friends: [], received: [], sent: [], blocked: [], parties: [] });
  keyChanged();
}

export const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

let refreshing: Promise<void> | null = null;
export function refresh(): Promise<void> {
  const k = currentKey();
  if (!k) return Promise.resolve();
  if (refreshing) return refreshing;
  refreshing = (async () => {
    try {
      const o = await api.overview(k);
      if (currentKey() !== k) return;
      set({ ...o, status: "ready", error: null, at: Date.now() });
      persist(`ui:friends:${k.address.toLowerCase()}`, o);
    } catch (e) {
      if (e instanceof api.ApiError && e.status === 401 && /not linked/.test(e.message)) {
        // The server no longer knows this key (revoked or expired): link again.
        forgetKey(k.address);
        key = null;
        set({ status: "unlinked" });
        keyChanged();
        return;
      }
      set({ status: get().at ? "ready" : "error", error: messageOf(e) });
    } finally {
      refreshing = null;
    }
  })();
  return refreshing;
}

async function run<T>(fn: (k: DeviceKey) => Promise<T>): Promise<T> {
  const k = currentKey();
  if (!k) throw new Error("Turn on friends first.");
  set({ busy: true });
  try {
    return await fn(k);
  } finally {
    set({ busy: false });
  }
}

export async function graph(op: GraphOp, address: string) {
  await run((k) => api.graph(k, op, address));
  await refresh();
}

export async function savePrivacy(s: Partial<Privacy>) {
  const me = get().me;
  if (me) set({ me: { ...me, settings: { ...me.settings, ...s } } });
  const v = await run((k) => api.saveSettings(k, s));
  const cur = get().me;
  if (cur) set({ me: { ...cur, settings: v } });
}

export const makeCode = () => run((k) => api.makeCode(k));

export async function redeemCode(code: string) {
  const r = await run((k) => api.redeemCode(k, code));
  await refresh();
  return r;
}

// ---------- parties ----------

function upsertParty(p: Party) {
  const rest = get().parties.filter((x) => x.id !== p.id);
  set({ parties: [p, ...rest] });
}

function dropParty(id: string) {
  const { [id]: _m, ...messages } = get().messages;
  const { [id]: _u, ...unread } = get().unread;
  set({ parties: get().parties.filter((x) => x.id !== id), messages, unread });
}

/** The party the player is in (joined), if any. */
export const activeParty = (s: Pick<FriendsState, "parties" | "address">) => s.parties.find((p) => p.members.some((m) => m.address === s.address && m.state === "joined")) ?? null;
/** Invites waiting for an answer. */
export const partyInvites = (s: Pick<FriendsState, "parties" | "address">) => s.parties.filter((p) => p.members.some((m) => m.address === s.address && m.state === "invited"));

export async function createParty(game: GameRef | null, invite: string[]) {
  const p = await run((k) => api.createParty(k, game, invite));
  set({ parties: [p, ...get().parties.filter((x) => !x.members.some((m) => m.address === get().address && m.state === "joined"))] });
  return p;
}

export async function inviteToParty(id: string, addresses: string[]) {
  upsertParty(await run((k) => api.inviteToParty(k, id, addresses)));
}

export async function joinParty(id: string) {
  const p = await run((k) => api.joinParty(k, id));
  // Joining one party leaves any other.
  set({ parties: get().parties.filter((x) => x.id === id || !x.members.some((m) => m.address === get().address && m.state === "joined")) });
  upsertParty(p);
  void loadMessages(id);
  return p;
}

export async function leaveParty(id: string) {
  await run((k) => api.leaveParty(k, id));
  dropParty(id);
}

export async function setPartyGame(id: string, game: GameRef | null) {
  upsertParty(await run((k) => api.setPartyGame(k, id, game)));
}

export async function loadMessages(id: string) {
  const k = currentKey();
  if (!k) return;
  const list = await api.messages(k, id).catch(() => null);
  if (list) set({ messages: { ...get().messages, [id]: list } });
}

export async function sendMessage(id: string, text: string) {
  const m = await run((k) => api.postMessage(k, id, text));
  addMessage(m);
}

function addMessage(m: Message) {
  const cur = get().messages[m.partyId] ?? [];
  if (cur.some((x) => x.id === m.id)) return false;
  set({ messages: { ...get().messages, [m.partyId]: [...cur, m].slice(-200) } });
  return true;
}

export function openChat(id: string | null) {
  set({ openParty: id });
  if (id) set({ unread: { ...get().unread, [id]: 0 } });
}

// ---------- people ----------

/** Name and picture of anyone we know about (friends, requests, party members). */
export function personOf(address: string): Person | null {
  const s = get();
  const a = address.toLowerCase();
  return (
    s.friends.find((f) => f.address === a) ??
    s.received.find((f) => f.address === a) ??
    s.sent.find((f) => f.address === a) ??
    s.parties.flatMap((p) => p.members).find((m) => m.address === a) ??
    null
  );
}

/** Username first, else SuiNS name, else a short address (display.ts). */
export const label = (p: Pick<Person, "name" | "address" | "username" | "suins"> | null | undefined, fallback = "Someone") => nameOf(p, fallback);

// ---------- live events ----------

export type Applied =
  | { kind: "online"; friend: Friend; game: GameRef | null }
  | { kind: "request"; from: Person | null; address: string }
  | { kind: "accepted"; who: Person | null; address: string }
  | { kind: "invite"; party: Party; from: string | null }
  | { kind: "message"; message: Message; party: Party | null }
  | null;

const setPresence = (address: string, presence: Presence) => {
  const s = get();
  const friends = s.friends.map((f) => (f.address === address ? { ...f, presence } : f));
  const parties = s.parties.map((p) => (p.members.some((m) => m.address === address) ? { ...p, members: p.members.map((m) => (m.address === address ? { ...m, presence } : m)) } : p));
  set({ friends, parties });
};

/** Applies one stream event to the store; returns what a notice could be made from. */
export async function apply(e: SocialEvent): Promise<Applied> {
  const s = get();
  if (s.status !== "ready") return null;
  switch (e.event) {
    case "presence": {
      const before = s.friends.find((f) => f.address === e.data.address);
      setPresence(e.data.address, e.data.presence);
      const after = get().friends.find((f) => f.address === e.data.address);
      if (before && after && before.presence.state === "offline" && e.data.presence.state !== "offline") return { kind: "online", friend: after, game: e.data.presence.game };
      return null;
    }
    case "friend": {
      await refresh();
      const who = personOf(e.data.address);
      if (e.data.change === "request") return { kind: "request", from: who, address: e.data.address };
      if (e.data.change === "accepted") return { kind: "accepted", who, address: e.data.address };
      return null;
    }
    case "party": {
      const d = e.data;
      if ((d.change === "closed" || d.change === "left") && d.id) {
        dropParty(d.id);
        return null;
      }
      if (!d.party) return null;
      // A party I am no longer in (left from another device).
      if (!d.party.members.some((m) => m.address === s.address)) {
        dropParty(d.party.id);
        return null;
      }
      upsertParty(d.party);
      if (d.change === "invite" && d.party.members.some((m) => m.address === s.address && m.state === "invited")) return { kind: "invite", party: d.party, from: d.from ?? null };
      return null;
    }
    case "chat": {
      const m = e.data;
      if (!addMessage(m)) return null;
      if (m.from === s.address || s.openParty === m.partyId) return null;
      set({ unread: { ...get().unread, [m.partyId]: (get().unread[m.partyId] ?? 0) + 1 } });
      return { kind: "message", message: m, party: get().parties.find((p) => p.id === m.partyId) ?? null };
    }
  }
  return null;
}
