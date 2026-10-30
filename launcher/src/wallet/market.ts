// The signing door for Rare Marketplace. Buy (and any later market action) builds a transaction
// with the account as sender and hands it to the wallet, which shows the review sheet, asks for
// the PIN or the device as the account needs, signs, and submits.
//
//   import { signAndExecute, SignRejected, canSignWithWallet, currentSigningAccount } from "../wallet/market";
//
//   const account = await currentSigningAccount();          // null when there is nothing to sign with
//   if (!account) return showQrFallback();
//   const tx = buildBuyTx({ ...listing, buyer: account.address });  // tx.setSender(account.address)
//   try {
//     const { digest } = await signAndExecute(tx, {
//       account: account.address,
//       origin: { name: "Rare Marketplace", kind: "market" },
//       title: `Buy ${item.name}`,
//     });
//   } catch (e) {
//     if (e instanceof SignRejected) return; // the person cancelled: say nothing
//     throw e;                                // build or network error: show its message
//   }
//
// Resolves once the network has the transaction (digest). Nothing is signed without the review.

import { keystore, type RareAccount } from "./index";
import { devSign } from "./ui/dev";

export { signAndExecute, signTransaction, signPersonalMessage, SignRejected, type SignResult, type SignOptions } from "./sign";

/** Whether this account can approve a purchase here (watch-only accounts cannot). */
export function canSignWithWallet(account: Pick<RareAccount, "kind"> | null | undefined): boolean {
  return !!account && (account.kind !== "watch" || devSign);
}

/** The selected account when it can sign, else null. */
export async function currentSigningAccount(): Promise<RareAccount | null> {
  const a = await keystore.current();
  return canSignWithWallet(a) ? a : null;
}
