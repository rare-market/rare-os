// Rareling perks for the current account: one ownership check (cached five minutes), the theme it
// picked, and a light check for other wallets (the "Rareling" label in Top traders).

import { create } from "zustand";
import { RARELING_NETWORK, rarelingType } from "./config";
import { checkRareling, grpcRarelingReader, type RarelingReader, type RarelingResult } from "./detect";
import { applyTheme, readTheme, saveTheme } from "./themes";

const TTL = 5 * 60_000;
let reader: RarelingReader | null = null;
const read = () => (reader ??= grpcRarelingReader(RARELING_NETWORK));
/** Tests swap the chain reads. */
export function setPerksReader(r: RarelingReader | null) {
  reader = r;
}

interface PerksState {
  address: string | null;
  result: RarelingResult | null;
  at: number;
  theme: string;
  check: (address: string | null, force?: boolean) => Promise<void>;
  setTheme: (id: string) => void;
}

export const usePerks = create<PerksState>((set, get) => ({
  address: null,
  result: null,
  at: 0,
  theme: "rare",
  check: async (address, force = false) => {
    const a = address?.toLowerCase() ?? null;
    const s = get();
    if (!a) {
      set({ address: null, result: { status: rarelingType() ? "none" : "unavailable" }, theme: "rare" });
      applyTheme(null);
      return;
    }
    if (!force && s.address === a && s.result && Date.now() - s.at < TTL) return;
    if (s.address !== a) set({ address: a, result: null, theme: readTheme(a) });
    const result = await checkRareling(a, rarelingType(), read()).catch((): RarelingResult => ({ status: "none" }));
    if (get().address !== a) return;
    set({ result, at: Date.now() });
    // A theme is a perk: it shows only while the account holds a Rareling.
    applyTheme(result.status === "owned" ? get().theme : null);
  },
  setTheme: (id) => {
    const { address, result } = get();
    if (!address || result?.status !== "owned") return;
    saveTheme(address, id);
    set({ theme: id });
    applyTheme(id);
  },
}));

export const holdsRareling = (r: RarelingResult | null) => r?.status === "owned";

// ---------- other wallets (trader list label) ----------

const others = new Map<string, { at: number; owned: boolean }>();
const pending = new Map<string, Promise<boolean>>();

/** Whether a wallet holds a Rareling; false when the type is not set yet. Cached ten minutes. */
export function walletHoldsRareling(address: string): Promise<boolean> {
  const type = rarelingType();
  if (!type) return Promise.resolve(false);
  const a = address.toLowerCase();
  const hit = others.get(a);
  if (hit && Date.now() - hit.at < 2 * TTL) return Promise.resolve(hit.owned);
  let p = pending.get(a);
  if (!p) {
    p = checkRareling(a, type, read())
      .then((r) => r.status === "owned")
      .catch(() => false)
      .then((owned) => {
        others.set(a, { at: Date.now(), owned });
        pending.delete(a);
        return owned;
      });
    pending.set(a, p);
  }
  return p;
}

/** Rareling holders among `addresses`, read a few at a time. */
export const useRarelingHolders = create<{ held: Set<string>; scan: (addresses: string[]) => Promise<void> }>((set, get) => ({
  held: new Set(),
  scan: async (addresses) => {
    if (!rarelingType()) return;
    const todo = [...new Set(addresses.map((a) => a.toLowerCase()))];
    for (let i = 0; i < todo.length; i += 4) {
      const chunk = todo.slice(i, i + 4);
      const res = await Promise.all(chunk.map((a) => walletHoldsRareling(a).then((o) => [a, o] as const)));
      const held = new Set(get().held);
      for (const [a, o] of res) o ? held.add(a) : held.delete(a);
      set({ held });
    }
  },
}));
