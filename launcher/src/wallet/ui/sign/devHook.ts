// Browser-only hook (window.__rareWalletDev) for screenshots and manual checks: starts sign
// requests, a PIN prompt, a web-game request, and forces a device waiting panel. Never installed
// inside Tauri.

import { keystore } from "../../index";
import type { WebGameRequest } from "../../native";
import { signAndExecute, signPersonalMessage, signTransaction } from "../../sign";
import { buildCoinSend } from "../../tx";
import { walletNfts } from "../../data/nfts";
import { setLedgerStage } from "../../ledger/prompt";
import { useWalletNav } from "../store";
import { requestUnlock, useUnlock } from "../unlock";
import { devSetKeystone, devSetPasskey, useDevHw, type CeremonyState } from "./prompts";

const OTHER = "0x" + "ab".repeat(32);

export function installDevHook(gameRequest: (r: WebGameRequest) => void) {
  const dev = {
    signAndExecute,
    signTransaction,
    signPersonalMessage,
    /** a real simulated send of `mist` SUI from the current account to another address */
    async sendDemo(mist = 100_000_000n, to = OTHER) {
      const me = await keystore.current();
      if (!me) throw new Error("no account");
      const tx = await buildCoinSend({ sender: me.address, to, coinType: "0x2::sui::SUI", amount: BigInt(mist) });
      return signAndExecute(tx, { title: "Send 0.1 SUI" }).catch(() => null);
    },
    async message(text = "Sign in to Rare Arcade\nNonce: 7f3a91c2") {
      return signPersonalMessage(new TextEncoder().encode(text), { origin: { name: "Rare Arcade", url: "https://arcade.rare.market", kind: "game" } }).catch(() => null);
    },
    unlock: (reason?: string) => requestUnlock(reason),
    /** shows the PIN sheet even when the vault is already open */
    unlockSheet: (reason = "Enter your PIN to approve") => useUnlock.setState({ pending: { reason, resolve: () => {} } }),
    openSend: (to?: string, coinType?: string) => useWalletNav.getState().replace({ kind: "send", to, coinType }),
    /** opens Send collectibles with the first few items the current account holds */
    async openNfts(n = 3) {
      const me = await keystore.current();
      if (!me) return;
      const page = await walletNfts(me.address, { limit: n });
      useWalletNav.getState().replace({ kind: "send-nfts", ids: page.data.slice(0, n).map((c) => c.id) });
    },
    ledger: (stage: Parameters<typeof setLedgerStage>[0], device?: string) => setLedgerStage(stage, device),
    gameRequest,
    hw: (kind: "ledger" | "keystone" | "passkey" | null) => useDevHw.setState({ force: kind }),
    keystone: (s: Parameters<typeof devSetKeystone>[0]) => devSetKeystone(s),
    passkey: (s: CeremonyState) => devSetPasskey(s),
  };
  (window as unknown as { __rareWalletDev: typeof dev }).__rareWalletDev = dev;
}
