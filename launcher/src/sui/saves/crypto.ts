// Encryption for Rare cloud saves. Everything is encrypted on the device before it leaves; Walrus
// only ever stores ciphertext.
//
// The key comes from the player's own wallet: Rare Wallet's deriveSecret("cloud-saves") gives 32
// bytes that are the same for an account on any device (passkey PRF, the phrase or imported key,
// or a Ledger/Keystone signature, see src/wallet/secret.ts), and HKDF-SHA256 over them gives the
// AES-256-GCM key. That is what makes restore on a new device work, for every account kind.
//
// Format of an encrypted blob: "RSV1" | 12-byte nonce | AES-GCM ciphertext+tag, with "RSV1"+keyId
// as additional data so a blob can't be opened with another account's key by accident.

import { hkdf } from "@noble/hashes/hkdf.js";
import { sha256 } from "@noble/hashes/sha2.js";

const MAGIC = new TextEncoder().encode("RSV1");
const enc = new TextEncoder();

export class WrongKey extends Error {
  constructor() {
    super("This backup was made with a different account");
    this.name = "WrongKey";
  }
}

export interface SaveKey {
  key: CryptoKey;
  /** 16 hex chars: identifies the key without revealing it (stored next to each backup) */
  id: string;
}

const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, "0")).join("");

/** The deriveSecret context for cloud saves. Changing it changes every key. */
export const SAVES_CONTEXT = "cloud-saves";

/** Derives the save key from the account's deriveSecret(SAVES_CONTEXT) bytes. */
export async function deriveSaveKey(secret: Uint8Array, address: string): Promise<SaveKey> {
  if (secret.length !== 32) throw new Error("Expected a 32-byte secret");
  const okm = hkdf(sha256, secret, enc.encode("rare-saves-v1"), enc.encode(address.toLowerCase()), 64);
  const key = await crypto.subtle.importKey("raw", okm.slice(0, 32), { name: "AES-GCM" }, false, ["encrypt", "decrypt"]);
  return { key, id: hex(sha256(okm.slice(32)).slice(0, 8)) };
}

function aad(id: string): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(MAGIC.length + 16);
  out.set(MAGIC);
  out.set(enc.encode(id.padEnd(16, "0").slice(0, 16)), MAGIC.length);
  return out;
}

export async function seal(k: SaveKey, plain: Uint8Array): Promise<Uint8Array> {
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: aad(k.id) }, k.key, plain as Uint8Array<ArrayBuffer>));
  const out = new Uint8Array(MAGIC.length + nonce.length + ct.length);
  out.set(MAGIC);
  out.set(nonce, MAGIC.length);
  out.set(ct, MAGIC.length + nonce.length);
  return out;
}

export async function open(k: SaveKey, data: Uint8Array): Promise<Uint8Array> {
  if (data.length < MAGIC.length + 12 + 16 || MAGIC.some((b, i) => data[i] !== b)) throw new Error("Not a Rare cloud save");
  const nonce = data.slice(MAGIC.length, MAGIC.length + 12);
  try {
    return new Uint8Array(await crypto.subtle.decrypt({ name: "AES-GCM", iv: nonce, additionalData: aad(k.id) }, k.key, data.slice(MAGIC.length + 12)));
  } catch {
    throw new WrongKey();
  }
}

/** Overhead added by seal(), for cost estimates. */
export const SEAL_OVERHEAD = 4 + 12 + 16;
