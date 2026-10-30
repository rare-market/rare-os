// Signature auth for friends (server side: market api/src/friends/auth.ts). No passwords, no email.
//
// The launcher keeps one Ed25519 device key per Rare Wallet account. Rare Wallet signs one personal
// message (the review sheet shows it) that links the key to the account for 90 days; after that every
// request to /v1/friends and /v1/presence is signed by the device key, so a passkey or Ledger is not
// asked on every heartbeat. The device key cannot move funds: the server only accepts it here.

import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";

export const PROTO = "rare-friends/1";
const enc = new TextEncoder();

/** The exact text the wallet signs (auth.ts linkText; the server rebuilds it to compare). */
export function linkText(p: { address: string; key: string; issued: string; expires: string }): string {
  return [
    "Rare friends",
    "",
    "Let this device handle your friends, parties and presence in Rare.",
    "",
    `Address: ${p.address}`,
    `Device key: ${p.key}`,
    `Issued: ${p.issued}`,
    `Expires: ${p.expires}`,
    "",
    "This does not move funds, send a transaction or cost gas.",
  ].join("\n");
}

export function requestText(p: { method: string; path: string; address: string; key: string; ts: number; nonce: string; bodyHash: string }): string {
  return [PROTO, `${p.method.toUpperCase()} ${p.path}`, p.address, p.key, String(p.ts), p.nonce, p.bodyHash].join("\n");
}

export function streamText(p: { address: string; key: string; ts: number }): string {
  return [PROTO, "STREAM", p.address, p.key, String(p.ts)].join("\n");
}

export const bodyHash = (body: string) => bytesToHex(sha256(enc.encode(body)));

const nonce = () => {
  const b = new Uint8Array(12);
  crypto.getRandomValues(b);
  return btoa(String.fromCharCode(...b)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
};

// ---------- the device key, per account ----------

interface Stored {
  secret: string;
  /** ms; 0 until the wallet linked it */
  linkedUntil: number;
}

const K = (address: string) => `rare.friends.key.${address.toLowerCase()}`;

function read(address: string): Stored | null {
  try {
    const v = localStorage.getItem(K(address));
    return v ? (JSON.parse(v) as Stored) : null;
  } catch {
    return null;
  }
}

function write(address: string, s: Stored | null) {
  try {
    if (s) localStorage.setItem(K(address), JSON.stringify(s));
    else localStorage.removeItem(K(address));
  } catch {
    /* private mode: linking lasts for this session */
  }
}

export class DeviceKey {
  readonly address: string;
  readonly kp: Ed25519Keypair;
  readonly key: string;
  linkedUntil: number;
  constructor(address: string, kp: Ed25519Keypair, linkedUntil: number) {
    this.address = address.toLowerCase();
    this.kp = kp;
    this.key = kp.toSuiAddress();
    this.linkedUntil = linkedUntil;
  }

  /** Linked and more than a day left. */
  get linked() {
    return this.linkedUntil > Date.now() + 86_400_000;
  }

  /** Headers for one signed request. `path` includes the query. */
  async headers(method: string, path: string, body: string, now = Date.now()): Promise<Record<string, string>> {
    const n = nonce();
    const text = requestText({ method, path, address: this.address, key: this.key, ts: now, nonce: n, bodyHash: bodyHash(body) });
    const { signature } = await this.kp.signPersonalMessage(enc.encode(text));
    return { "x-rare-address": this.address, "x-rare-key": this.key, "x-rare-ts": String(now), "x-rare-nonce": n, "x-rare-signature": signature };
  }

  /** The notify stream's `social` token (valid 12 hours on the server). */
  async streamToken(now = Date.now()): Promise<string> {
    const { signature } = await this.kp.signPersonalMessage(enc.encode(streamText({ address: this.address, key: this.key, ts: now })));
    const json = JSON.stringify({ a: this.address, k: this.key, t: now, s: signature });
    return btoa(json).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  }
}

/** This device's key for the account, made on first use. */
export function deviceKey(address: string): DeviceKey {
  const s = read(address);
  if (s) {
    try {
      return new DeviceKey(address, Ed25519Keypair.fromSecretKey(s.secret), s.linkedUntil);
    } catch {
      /* corrupt: make a new one */
    }
  }
  const kp = Ed25519Keypair.generate();
  write(address, { secret: kp.getSecretKey(), linkedUntil: 0 });
  return new DeviceKey(address, kp, 0);
}

export function markLinked(k: DeviceKey, until: number) {
  k.linkedUntil = until;
  write(k.address, { secret: k.kp.getSecretKey(), linkedUntil: until });
}

/** Forget the key (sign out of friends on this device). */
export function forgetKey(address: string) {
  write(address, null);
}

/** The link request for the wallet to sign: 90 days from now. */
export function linkRequest(k: DeviceKey, now = Date.now()) {
  const issued = new Date(now).toISOString();
  const expires = new Date(now + 90 * 86_400_000).toISOString();
  return { message: linkText({ address: k.address, key: k.key, issued, expires }), expires: Date.parse(expires) };
}
