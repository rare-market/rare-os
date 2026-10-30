// DEV ONLY: a software passkey authenticator for the browser build (`npm run dev:ui`, screenshots,
// tests). It stands in for the Rust CTAP2 side (native.ts uses it whenever the app is not running
// in Tauri) and follows the same contract: base64 in and out, browser-shaped clientDataJSON, a real
// P-256 key and DER signatures, and the same "passkey-event"s (a fake FIDO:/ QR, then status).
//
// PRF works like a real authenticator's hmac-secret: HMAC-SHA-256(credential secret, salt), with
// salt = SHA-256("WebAuthn PRF" || 0x00 || input), the same convention as browsers and the Rust side.
//
// The private keys sit in plain localStorage. This is NOT a real authenticator and never runs in
// the launcher on the device.

import { p256 } from "@noble/curves/nist.js";
import { hmac } from "@noble/hashes/hmac.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { randomBytes } from "@noble/hashes/utils.js";
import { fromBase64, toBase64 } from "@mysten/sui/utils";
import { NativeError, type PasskeyAssertion, type PasskeyEvent, type PasskeyRegistration, type PasskeyVia } from "../native";

export const SOFT_RP_ID = "rare.market";
export const SOFT_ORIGIN = "https://rare.market";
const KEY = "rare.dev.softPasskeys";

export interface SoftStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

interface SoftCred {
  id: string; // base64 credential id
  sk: string; // base64 P-256 secret key (dev only)
  userId: string; // base64 user handle
  userName: string;
  counter: number;
  prfKey?: string; // base64 32-byte credential secret for PRF (dev only)
}

const b64url = (b: Uint8Array) => toBase64(b).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");

/** The salt an authenticator HMACs for WebAuthn PRF input `input`. */
export function prfSaltFor(input: Uint8Array): Uint8Array {
  return sha256(new Uint8Array([...new TextEncoder().encode("WebAuthn PRF"), 0, ...input]));
}

/** clientDataJSON the way a browser writes it (same field order as the Rust side). */
export function clientDataJSON(type: "webauthn.create" | "webauthn.get", challenge: Uint8Array): Uint8Array {
  return new TextEncoder().encode(JSON.stringify({ type, challenge: b64url(challenge), origin: SOFT_ORIGIN, crossOrigin: false }));
}

function memoryStorage(): SoftStorage {
  const m = new Map<string, string>();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
}

function defaultStorage(): SoftStorage {
  try {
    if (typeof localStorage !== "undefined") return localStorage;
  } catch {
    /* blocked storage */
  }
  return memoryStorage();
}

export interface SoftAuthenticator {
  register(o: { userId: string; userName: string; displayName: string; challenge: string; via: PasskeyVia }): Promise<PasskeyRegistration>;
  assert(o: { challenge: string; credentialIds: string[]; via: PasskeyVia; prfSalt?: string }): Promise<PasskeyAssertion>;
  prf(credentialIds: string[], salt: string, via: PasskeyVia): Promise<string>;
  events(cb: (e: PasskeyEvent) => void): Promise<() => void>;
}

export function createSoftAuthenticator(o: { storage?: SoftStorage; delayMs?: number } = {}): SoftAuthenticator {
  const storage = o.storage ?? memoryStorage();
  const delay = o.delayMs ?? 0;
  const listeners = new Set<(e: PasskeyEvent) => void>();
  const emit = (e: PasskeyEvent) => listeners.forEach((l) => l(e));
  const wait = () => (delay > 0 ? new Promise((r) => setTimeout(r, delay)) : Promise.resolve());

  const load = (): SoftCred[] => {
    try {
      return JSON.parse(storage.getItem(KEY) ?? "[]") as SoftCred[];
    } catch {
      return [];
    }
  };
  const save = (c: SoftCred[]) => storage.setItem(KEY, JSON.stringify(c));

  // The same progression the phone flow shows: QR, phone found, confirm, done.
  async function ceremony(via: PasskeyVia) {
    if (via === "phone") {
      const digits = Array.from(randomBytes(40), (b) => (b % 10).toString()).join("");
      emit({ kind: "qr", url: `FIDO:/${digits}` });
      await wait();
      emit({ kind: "status", stage: "connecting", message: "Phone found. Connecting" });
      await wait();
      emit({ kind: "status", stage: "touch", message: "Confirm on your phone" });
    } else {
      emit({ kind: "status", stage: "touch", message: "Touch your security key" });
    }
    await wait();
    emit({ kind: "status", stage: "processing", message: "Working" });
  }

  const api: SoftAuthenticator = {
    async register(r) {
      await ceremony(r.via);
      const sk = p256.utils.randomSecretKey();
      const id = randomBytes(16);
      save([...load(), { id: toBase64(id), sk: toBase64(sk), userId: r.userId, userName: r.userName, counter: 0, prfKey: toBase64(randomBytes(32)) }]);
      return { credentialId: toBase64(id), publicKey: toBase64(p256.getPublicKey(sk, true)), authenticator: r.via === "usb" ? "security-key" : "phone", prf: true };
    },

    async prf(credentialIds, salt, via) {
      const a = await api.assert({ challenge: toBase64(randomBytes(32)), credentialIds, via, prfSalt: salt });
      if (!a.prf) throw new NativeError("no-prf", "This passkey cannot protect saved data.");
      return a.prf;
    },

    async assert(a) {
      await ceremony(a.via);
      const creds = load();
      // discoverable: the newest passkey (a phone would let the person pick)
      const cred = a.credentialIds.length ? creds.find((c) => a.credentialIds.includes(c.id)) : creds[creds.length - 1];
      if (!cred) {
        const e = new NativeError("notfound", "No Rare passkey on this phone or key");
        emit({ kind: "error", code: e.code, message: e.message });
        throw e;
      }
      cred.counter++;
      cred.prfKey ??= toBase64(randomBytes(32)); // credentials saved before PRF existed
      save(creds);
      const authData = new Uint8Array(37);
      authData.set(sha256(new TextEncoder().encode(SOFT_RP_ID)), 0);
      authData[32] = 0x05; // user present + user verified
      new DataView(authData.buffer).setUint32(33, cred.counter);
      const cdj = clientDataJSON("webauthn.get", fromBase64(a.challenge));
      const msg = new Uint8Array([...authData, ...sha256(cdj)]);
      const sig = p256.sign(msg, fromBase64(cred.sk), { format: "der" }); // prehash: sha256(msg)
      return {
        credentialId: cred.id,
        authenticatorData: toBase64(authData),
        clientDataJSON: toBase64(cdj),
        signature: toBase64(sig),
        userHandle: a.credentialIds.length ? null : cred.userId,
        prf: a.prfSalt ? toBase64(hmac(sha256, fromBase64(cred.prfKey), prfSaltFor(fromBase64(a.prfSalt)))) : null,
      };
    },

    async events(cb) {
      listeners.add(cb);
      return () => void listeners.delete(cb);
    },
  };
  return api;
}

// The browser build's single instance, kept in localStorage across reloads.
let dev: SoftAuthenticator | null = null;
const devAuth = () => (dev ??= createSoftAuthenticator({ storage: defaultStorage(), delayMs: 700 }));

export const softRegister: SoftAuthenticator["register"] = (o) => devAuth().register(o);
export const softAssert: SoftAuthenticator["assert"] = (o) => devAuth().assert(o);
export const softPrf: SoftAuthenticator["prf"] = (ids, salt, via) => devAuth().prf(ids, salt, via);
export const softEvents: SoftAuthenticator["events"] = (cb) => devAuth().events(cb);
