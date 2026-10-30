// Followed traders, kept on this device for each Rare Wallet account (no account: one device list).
// Nothing leaves the device: the follow list is local storage, the feed polls public trades.

import { create } from "zustand";

export interface Followed {
  address: string;
  name: string;
  /** when the follow started; trades before it never notify */
  since: number;
}

/** The most traders one account follows, so the feed stays a handful of light reads. */
export const MAX_FOLLOWS = 30;

export interface KV {
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
}

const store = (): KV | null => {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
};

export const followKey = (account: string | null) => `rare.traders.follow.${account ? account.toLowerCase() : "device"}`;

export function readFollows(account: string | null, kv: KV | null = store()): Followed[] {
  try {
    const raw = kv?.getItem(followKey(account));
    const v = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(v) ? v.filter((f): f is Followed => !!f && typeof f.address === "string" && /^0x[0-9a-f]{64}$/.test(f.address)).slice(0, MAX_FOLLOWS) : [];
  } catch {
    return [];
  }
}

export function writeFollows(account: string | null, list: Followed[], kv: KV | null = store()) {
  try {
    kv?.setItem(followKey(account), JSON.stringify(list));
  } catch {
    /* private mode: the list lasts this session */
  }
}

/** Follows or unfollows; the newest follow first; never more than MAX_FOLLOWS. */
export function toggleFollow(list: Followed[], who: { address: string; name: string }, now = Date.now()): Followed[] {
  const a = who.address.toLowerCase();
  if (list.some((f) => f.address === a)) return list.filter((f) => f.address !== a);
  return [{ address: a, name: who.name, since: now }, ...list].slice(0, MAX_FOLLOWS);
}

interface FollowState {
  account: string | null;
  list: Followed[];
  /** reads the list for this account (the feed and the screens call it as the account changes) */
  use: (account: string | null) => void;
  toggle: (who: { address: string; name: string }) => void;
}

export const useFollows = create<FollowState>((set, get) => ({
  account: null,
  list: readFollows(null),
  use: (account) => {
    const a = account?.toLowerCase() ?? null;
    if (a === get().account) return;
    set({ account: a, list: readFollows(a) });
  },
  toggle: (who) => {
    const { account, list } = get();
    const next = toggleFollow(list, who);
    writeFollows(account, next);
    set({ list: next });
  },
}));

export const isFollowing = (list: Followed[], address: string) => list.some((f) => f.address === address.toLowerCase());
