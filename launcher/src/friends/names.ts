// Rare usernames for addresses we meet outside friends (top traders, a trader's page): one cached
// lookup per address on rare.market's public /v1/players route, a few at a time, failures quiet.

import { useEffect, useState } from "react";
import { FRIENDS_BASE } from "./types";
import { shown, type Shown } from "./display";

type Entry = { username: string | null; suins: string | null };
const cache = new Map<string, Entry | null>();
const waiting = new Map<string, Promise<void>>();
const subs = new Set<() => void>();
let active = 0;
const queue: Array<() => void> = [];

function slot<T>(fn: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const run = () => {
      active++;
      fn()
        .then(resolve, reject)
        .finally(() => {
          active--;
          queue.shift()?.();
        });
    };
    if (active < 3) run();
    else queue.push(run);
  });
}

/** Loads one address into the cache (no-op when known). */
export function loadRareName(address: string): Promise<void> {
  const a = address.toLowerCase();
  if (cache.has(a)) return Promise.resolve();
  let p = waiting.get(a);
  if (!p) {
    p = slot(async () => {
      try {
        const r = await fetch(`${FRIENDS_BASE}/players/${a}`);
        if (!r.ok) throw new Error(String(r.status));
        const j = (await r.json()) as { username?: string | null; suins?: string | null };
        cache.set(a, { username: j.username ?? null, suins: j.suins ?? null });
      } catch {
        cache.set(a, null);
      }
    }).finally(() => {
      waiting.delete(a);
      subs.forEach((f) => f());
    });
    waiting.set(a, p);
  }
  return p;
}

/** The Rare username of an address, when it has one; null until known or when there is none. */
export function useRareName(address: string | null | undefined): Shown | null {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!address) return;
    const f = () => tick((n) => n + 1);
    subs.add(f);
    void loadRareName(address);
    return () => void subs.delete(f);
  }, [address]);
  const e = address ? cache.get(address.toLowerCase()) : null;
  return e?.username ? shown({ address: address!, username: e.username, suins: e.suins }) : null;
}
