// The one signing door the Sui features use. In the launcher it is Rare Wallet's signing API
// (src/wallet/sign.ts: every request goes through the wallet's review sheet, nothing is signed
// without the player approving it). Tests pass a plain keypair signer instead.

import type { Transaction } from "@mysten/sui/transactions";

export interface CloudSigner {
  address: string;
  /** base64 serialized signature over the message */
  signPersonalMessage: (message: Uint8Array, title: string) => Promise<{ signature: string }>;
  /** resolves once the network has the transaction */
  signAndExecute: (tx: Transaction, title: string) => Promise<{ digest: string }>;
}

const ORIGIN = { name: "Rare cloud saves", kind: "wallet" as const };

/** Rare Wallet as a CloudSigner for `address`. `origin` names the feature on the review sheet. */
export async function walletSigner(address: string, origin: { name: string; kind: "wallet" } = ORIGIN): Promise<CloudSigner> {
  const w = await import("../wallet/sign");
  return {
    address,
    signPersonalMessage: (message, title) => w.signPersonalMessage(message, { account: address, origin, title }),
    signAndExecute: async (tx, title) => {
      const r = await w.signAndExecute(tx, { account: address, origin, title });
      if (!r.digest) throw new Error("The network did not return a transaction");
      return { digest: r.digest };
    },
  };
}
