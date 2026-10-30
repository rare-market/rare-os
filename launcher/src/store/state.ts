// Store state shared by the Store tab, the game page and the Library: the deployment config, the
// current Rare Wallet account's licences (rare.market /v1/store/licences), installed store games
// (Rust) and install progress.

import { useEffect } from "react";
import { create } from "zustand";
import { keystore, type RareAccount } from "../wallet";
import { api } from "./api";
import { onStoreProgress, storeNative } from "./native";
import type { Installed, Licence, Progress, StoreConfig } from "./types";

interface StoreState {
  config: StoreConfig | null;
  account: RareAccount | null;
  licences: Licence[];
  /** Address the licences were read for. */
  licencesFor: string | null;
  installed: Installed[];
  progress: Record<string, Progress>;
  /** What each slug is doing right now ("Getting ready", "Approve in Rare Wallet"). */
  busy: Record<string, string>;
  setBusy: (slug: string, what: string | null) => void;
  refresh: () => Promise<void>;
  refreshInstalled: () => Promise<void>;
}

export const useStore = create<StoreState>((set, get) => ({
  config: null,
  account: null,
  licences: [],
  licencesFor: null,
  installed: [],
  progress: {},
  busy: {},
  setBusy: (slug, what) => {
    const busy = { ...get().busy };
    if (what) busy[slug] = what;
    else delete busy[slug];
    set({ busy });
  },
  refresh: () => (inflight ??= refreshNow().finally(() => (inflight = null))),
  refreshInstalled: async () => set({ installed: await storeNative.installed().catch(() => get().installed) }),
}));

let inflight: Promise<void> | null = null;
async function refreshNow(): Promise<void> {
  const { setState: set, getState: get } = useStore;
  const [config, account] = await Promise.all([api.config().catch(() => get().config), keystore.current().catch(() => null)]);
  set({ config, account });
  void get().refreshInstalled();
  if (!account) {
    set({ licences: [], licencesFor: null });
    return;
  }
  const licences = await api.licences(account.address).catch(() => (get().licencesFor === account.address ? get().licences : []));
  set({ licences, licencesFor: account.address });
}

let started = false;
/** Once: progress events, and a first read. */
export function startStore() {
  if (started) return;
  started = true;
  void onStoreProgress((p) => {
    const progress = { ...useStore.getState().progress };
    if (p.phase === "done") delete progress[p.slug];
    else progress[p.slug] = p;
    useStore.setState({ progress });
  });
  void useStore.getState().refresh();
  // Back from the wallet or a browser: the account or its licences may have changed.
  window.addEventListener("focus", () => void useStore.getState().refresh());
  // Another account picked in Wallet.
  void import("../wallet/ui/store").then(({ useWallet }) =>
    useWallet.subscribe((s, prev) => {
      if (s.current?.address !== prev.current?.address) void useStore.getState().refresh();
    }),
  );
  // At boot the account can land a moment after the first read.
  let tries = 0;
  const again = () => {
    if (useStore.getState().account || ++tries > 3) return;
    void useStore.getState().refresh().then(() => setTimeout(again, 1500));
  };
  setTimeout(again, 1500);
}

/** Store screens read fresh licences when they open (the account can change in Wallet). */
export function useStoreFresh(enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    if (started) void useStore.getState().refresh();
    startStore();
  }, [enabled]);
}

export const licenceFor = (slug: string) => useStore.getState().licences.find((l) => l.slug === slug) ?? null;
