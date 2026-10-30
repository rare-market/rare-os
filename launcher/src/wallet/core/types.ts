/* Rare Wallet: shared account type. */

/** An account Rare Wallet manages. Public data only: keys live in the vault or on a device. */
export interface RareAccount {
  address: string;
  label: string;
  /**
   * native: recovery phrase or imported key in the vault
   * passkey: a 1-of-2 Sui multisig of the person's passkey and a key kept on this device
   * watch: view only, no signing
   * ledger / keystone: the key stays on the hardware wallet
   * social: reserved (zkLogin is not offered)
   */
  kind: "native" | "passkey" | "social" | "watch" | "ledger" | "keystone";
  /** native phrase accounts: account index on m/44'/784'/i'/0'/0' and the id of the phrase it came from */
  hd?: { index: number; phraseId: string };
  /** passkey accounts (see wallet/passkey) */
  passkey?: {
    /** base64 WebAuthn credential id */
    credentialId: string;
    /** base64 compressed secp256r1 public key (33 bytes) */
    publicKey: string;
    /** base64 Ed25519 public key of the device member; its secret is in the vault when this device holds it */
    deviceKey: string;
    /** where the passkey lives, for copy ("iPhone", "Security key") */
    authenticator?: string;
  };
  /** set for legacy WebAuthn-in-browser passkey accounts; unused in the launcher */
  vaultId?: string;
  /** Ledger accounts: the key's BIP32 path on the device and the model it was added with. */
  ledger?: { path: string; device?: string };
  /** Keystone accounts: BIP32 path, master fingerprint (hex) and device name. */
  keystone?: { path: string; mfp: string; device?: string };
}
