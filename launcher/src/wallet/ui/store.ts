// UI state for Rare Wallet: accounts and lock state, the wallet tab's page stack (like the
// Marketplace's: pages underneath stay mounted, B goes back with focus restored), and the
// bottom-bar hints for the page on top.

import { create } from "zustand";
import { keystore, type RareAccount } from "../index";
import { vault, type VaultStatus } from "../native";

export type WalletView =
  | { kind: "home"; tab?: "coins" | "collectibles" | "activity" }
  | { kind: "coin"; coinType: string }
  | { kind: "nft"; id: string }
  | { kind: "activity"; digest: string }
  | { kind: "send"; coinType?: string; to?: string }
  | { kind: "send-nfts"; ids: string[]; to?: string }
  | { kind: "receive" }
  | { kind: "accounts" }
  | { kind: "settings" }
  | { kind: "add" } // add account menu: passkey, new phrase, import, Ledger, Keystone, watch
  | { kind: "passkey"; mode: "create" | "signin" }
  | { kind: "create-phrase" }
  | { kind: "import"; what: "phrase" | "key" }
  | { kind: "watch" }
  | { kind: "ledger" }
  | { kind: "keystone" }
  | { kind: "playtron" };

interface Frame {
  key: number;
  view: WalletView;
  focus: HTMLElement | null;
}

let seq = 0;

interface NavState {
  stack: Frame[];
  push: (v: WalletView) => void;
  /** Replaces the top page (finishing a flow lands on its result). */
  replace: (v: WalletView) => void;
  pop: () => boolean;
  reset: (views?: WalletView[]) => void;
}

const home = (): Frame => ({ key: ++seq, view: { kind: "home" }, focus: null });

export const useWalletNav = create<NavState>((set, get) => ({
  stack: [home()],
  push: (view) => {
    const stack = get().stack.slice();
    stack[stack.length - 1] = { ...stack[stack.length - 1], focus: document.activeElement as HTMLElement | null };
    stack.push({ key: ++seq, view, focus: null });
    while (stack.length > 8) stack.splice(1, 1);
    set({ stack });
  },
  replace: (view) => {
    const stack = get().stack.slice();
    stack[stack.length - 1] = { key: ++seq, view, focus: null };
    set({ stack });
  },
  pop: () => {
    const s = get().stack;
    if (s.length <= 1) return false;
    set({ stack: s.slice(0, -1) });
    return true;
  },
  reset: (views = []) => set({ stack: [home(), ...views.map((view) => ({ key: ++seq, view, focus: null }))] }),
}));

export { useWalletHints } from "./hints";

interface WalletState {
  /** undefined until first load */
  accounts: RareAccount[] | undefined;
  current: RareAccount | null;
  vault: VaultStatus | null;
  /** bumps after a send so screens refetch balances */
  epoch: number;
  refresh: () => Promise<void>;
  select: (address: string) => Promise<void>;
  bump: () => void;
}

export const useWallet = create<WalletState>((set, get) => ({
  accounts: undefined,
  current: null,
  vault: null,
  epoch: 0,
  refresh: async () => {
    const [accounts, current, st] = await Promise.all([keystore.listAccounts(), keystore.current(), vault.status().catch(() => null)]);
    set({ accounts, current, vault: st });
    // the top bar's account value follows the selected account
    void import("../../lib/live").then((l) => l.setLiveAccount(current?.address ?? null)).catch(() => {});
    // web games see the selected account
    const { native } = await import("../../lib/bridge");
    void native.walletSetSession({
      address: current?.address ?? null,
      label: current?.label ?? null,
      kind: current?.kind ?? null,
      publicKey: current ? ((await keystore.publicKey(current.address)) ?? null) : null,
    });
  },
  select: async (address) => {
    await keystore.select(address);
    await get().refresh();
  },
  bump: () => set((s) => ({ epoch: s.epoch + 1 })),
}));
