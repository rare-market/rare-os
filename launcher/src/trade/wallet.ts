// The Trade tab's only door to Rare Wallet. Every trade is built here as an unsigned transaction
// and handed to the wallet's signing API (src/wallet/market.ts), which shows its review sheet, asks
// for the PIN or the device as the account needs, signs and submits. Nothing here signs.

import type { Transaction } from "@mysten/sui/transactions";
import { useEffect, useState } from "react";
import { isTauri } from "../lib/bridge";
import { keystore, type RareAccount } from "../wallet";
import { SignRejected, canSignWithWallet, signAndExecute as walletSign, type SignResult } from "../wallet/market";
import { signAndExecuteSponsored } from "../wallet/sign";
import type { SponsorCategory, Sponsorship } from "../wallet/sponsor";

export type { SignResult };

/** The person declined in Rare Wallet's review sheet. Say nothing. */
export const isRejection = (e: unknown) => e instanceof SignRejected;

export const ORIGIN = { name: "Rare Trade", kind: "market" as const };

/** Review, sign and submit through Rare Wallet. Resolves once the network has it. */
export function signAndExecute(tx: Transaction, o: { account: string; title: string }): Promise<SignResult> {
  return walletSign(tx, { account: o.account, origin: ORIGIN, title: o.title });
}

/** As signAndExecute, with the fee covered by Rare (`sponsorship` from sponsorTransaction). */
export function signAndExecuteCovered(tx: Transaction, sponsorship: Sponsorship, category: SponsorCategory, o: { account: string; title: string }): Promise<SignResult> {
  return signAndExecuteSponsored(tx, sponsorship, category, { account: o.account, origin: ORIGIN, title: o.title });
}

/** Whether this account could approve a trade (watch-only accounts never can). */
export function canSign(account: RareAccount | null): boolean {
  return canSignWithWallet(account);
}

/** The selected Rare Wallet account, refreshed when the launcher comes back into focus. */
export function useTradeAccount(): { account: RareAccount | null; ready: boolean } {
  const [state, setState] = useState<{ account: RareAccount | null; ready: boolean }>({ account: null, ready: false });
  useEffect(() => {
    let live = true;
    const read = async () => {
      // Dev and screenshots: ?watch=0x... adds a watch account, as on the Wallet tab.
      const q = new URLSearchParams(location.search).get("watch");
      let a = await keystore.current().catch(() => null);
      if (!a && q && !isTauri && /^0x[0-9a-fA-F]{64}$/.test(q)) {
        a = await keystore.addWatch(q, "Demo wallet").catch(() => null);
        if (a) await keystore.select(a.address).catch(() => {});
      }
      if (live) setState({ account: a, ready: true });
    };
    void read();
    const onFocus = () => void read();
    window.addEventListener("focus", onFocus);
    return () => {
      live = false;
      window.removeEventListener("focus", onFocus);
    };
  }, []);
  return state;
}
