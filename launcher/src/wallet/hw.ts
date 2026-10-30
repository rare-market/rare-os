// Ledger and Keystone signers for the keystore, loaded on first use so the hardware wallet
// libraries stay out of the main bundle. Importing ./ledger or ./keystone registers the same
// providers directly.
import type { Keystore } from "./core/keys/keystore";

export function registerHardwareSigners(ks: Keystore) {
  ks.setProvider("ledger", async (account, publicKey) => (await import("./ledger")).ledgerSigner(account, publicKey));
  ks.setProvider("keystone", async (account, publicKey) => (await import("./keystone")).keystoneSigner(account, publicKey));
}
