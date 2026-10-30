// deriveSecret(context): 32 secret bytes, the same for a given account and context on any device.
// Features that encrypt the player's data (cloud saves) take their key from here, so a new
// handheld with the same account opens the same data and nobody else can.
//
// Per account kind:
// - phrase or imported key: HKDF-SHA256 over the private key (after the PIN unlocks the vault)
// - passkey: the WebAuthn PRF extension (CTAP2 hmac-secret) evaluated by the passkey on the phone
//   or security key. The PRF input is SHA-256("rare/" + context), so a browser on rare.market can
//   derive the same secret later with WebAuthn's prf extension and the same input.
// - Ledger / Keystone: the device signs a fixed message (Ed25519 signatures are deterministic) and
//   the signature is hashed. The message goes through the review sheet like any other signature.
// Every path then runs the same HKDF with the account address, so contexts and accounts never
// share a secret.

import { fromBase64 } from "@mysten/sui/utils";
import { hkdf } from "@noble/hashes/hkdf.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { keystore, type RareAccount } from "./index";

const enc = new TextEncoder();

/** HKDF salt shared by every derivation (bump the version to rotate everything). */
export const DERIVE_SALT = enc.encode("rare/derive/v1");

/** HKDF info: the context and the account, so secrets never repeat across either. */
export const deriveInfo = (context: string, address: string) => enc.encode(`rare/${context}\u0000${address.toLowerCase()}`);

/** The WebAuthn PRF input for a context (32 bytes). */
export const prfInput = (context: string) => sha256(enc.encode(`rare/${context}`));

/** The message a Ledger or Keystone signs to derive a secret. Never changes for a version. */
export function deriveMessage(context: string, address: string): Uint8Array {
  return enc.encode(
    `Rare key\n\nSign to unlock "${context}" on this device. This is not a transaction and costs nothing.\n\nAccount: ${address.toLowerCase()}\nVersion: 1`,
  );
}

/** Final step for every kind: 32 bytes from raw key material, bound to context and account. */
export const finish = (ikm: Uint8Array, context: string, address: string) => hkdf(sha256, ikm, DERIVE_SALT, deriveInfo(context, address), 32);

/** An Ed25519 Sui signature (flag 0x00 | sig 64 | pk 32) -> its 64 signature bytes; anything else is refused. */
export function ed25519SignatureBytes(serialized: string): Uint8Array {
  const b = fromBase64(serialized);
  if (b[0] !== 0x00 || b.length !== 97) throw new Error("This account's signatures can't derive a key");
  return b.slice(1, 65);
}

export class DeriveUnavailable extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DeriveUnavailable";
  }
}

export interface DeriveOptions {
  /** account address; the selected account by default */
  account?: string;
  /** title on the review sheet for Ledger/Keystone, e.g. "Unlock cloud saves" */
  title?: string;
}

async function accountFor(address?: string): Promise<RareAccount> {
  const all = await keystore.listAccounts();
  const acc = address ? all.find((a) => a.address === address) : await keystore.current();
  if (!acc) throw new DeriveUnavailable("Add a Rare Wallet account first");
  return acc;
}

/** 32 bytes, deterministic per account and context. Asks for the PIN, a passkey tap or a device confirm as the account needs. */
export async function deriveSecret(context: string, o: DeriveOptions = {}): Promise<Uint8Array> {
  if (!context || context.length > 64) throw new Error("Invalid context");
  const acc = await accountFor(o.account);
  switch (acc.kind) {
    case "watch":
      throw new DeriveUnavailable("This account is watch only. Use an account you can sign with.");
    case "native": {
      const { ensureUnlockedFor } = await import("./ui/unlock");
      if (!(await ensureUnlockedFor(acc.address, o.title ? `Enter your PIN to ${o.title.toLowerCase()}` : undefined))) throw new DeriveUnavailable("Cancelled");
      return keystore.deriveFromKey(acc.address, DERIVE_SALT, deriveInfo(context, acc.address));
    }
    case "passkey": {
      const { passkeyPrf } = await import("./passkey");
      return finish(await passkeyPrf(acc, prfInput(context)), context, acc.address);
    }
    case "ledger":
    case "keystone": {
      const { signPersonalMessage } = await import("./sign");
      const { signature } = await signPersonalMessage(deriveMessage(context, acc.address), {
        account: acc.address,
        origin: { name: "Rare Wallet", kind: "wallet" },
        title: o.title ?? "Unlock with your device",
      });
      return finish(ed25519SignatureBytes(signature), context, acc.address);
    }
    default:
      throw new DeriveUnavailable("This account can't derive a key");
  }
}
