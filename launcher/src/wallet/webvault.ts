// Browser stand-in for the Rust vault, used only outside Tauri (UI development, screenshots and
// tests). Same contract and error codes as src-tauri/src/wallet/vault.rs, with WebCrypto
// PBKDF2 + AES-GCM (core/keys/crypto.ts) instead of Argon2id. Never used on the device.

import { openWithPassword, seal, sealWithPassword, type VaultBlob } from "./core/keys/crypto";
import type { SecretKind, VaultPort, VaultStatus } from "./native";
import { NativeError } from "./native";

const KEY = "rare.webvault";
// Fewer rounds than production: this vault never holds real funds.
const ITER = 60_000;

interface Stored {
  blob: VaultBlob;
  kind: SecretKind;
  failures: number;
  until: number;
}

let mem: { key: CryptoKey; payload: string } | null = null;

const load = (): Stored | null => {
  try {
    const v = localStorage.getItem(KEY);
    return v ? (JSON.parse(v) as Stored) : null;
  } catch {
    return null;
  }
};
const store = (s: Stored | null) => (s ? localStorage.setItem(KEY, JSON.stringify(s)) : localStorage.removeItem(KEY));

/** Delay after `n` wrong attempts in a row: none for 4, then 30 s doubling to 1 h. Same as Rust. */
export const throttleMs = (n: number) => (n < 5 ? 0 : Math.min(3_600_000, 30_000 * 2 ** (n - 5)));

async function check(secret: string): Promise<{ s: Stored; key: CryptoKey; payload: string }> {
  const s = load();
  if (!s) throw new NativeError("missing", "No wallet on this device yet");
  const wait = s.until - Date.now();
  if (wait > 0) throw new NativeError("throttled", "Too many tries. Wait a moment.", { retryAfterMs: wait });
  try {
    const { key, value } = await openWithPassword<{ p: string }>(secret, s.blob);
    if (s.failures) store({ ...s, failures: 0, until: 0 });
    return { s, key, payload: value.p };
  } catch {
    const failures = s.failures + 1;
    const retryAfterMs = throttleMs(failures);
    store({ ...s, failures, until: Date.now() + retryAfterMs });
    throw new NativeError("wrong", s.kind === "pin" ? "Wrong PIN" : "Wrong passphrase", { failures, retryAfterMs });
  }
}

const status = async (): Promise<VaultStatus> => {
  const s = load();
  return {
    exists: !!s,
    unlocked: !!s && !!mem,
    backend: s ? "file" : null,
    kind: s?.kind ?? null,
    failures: s?.failures ?? 0,
    retryAfterMs: s ? Math.max(0, s.until - Date.now()) : 0,
  };
};

export const webVault: VaultPort = {
  status,
  async create(secret, kind, payload) {
    if (load()) throw new NativeError("exists", "A wallet already exists on this device");
    const { key, blob } = await sealWithPassword(secret, { p: payload }, ITER);
    store({ blob, kind, failures: 0, until: 0 });
    mem = { key, payload };
    return status();
  },
  async unlock(secret) {
    const { key, payload } = await check(secret);
    mem = { key, payload };
    return payload;
  },
  async read() {
    return mem?.payload ?? null;
  },
  async save(payload) {
    const s = load();
    if (!s || !mem) throw new NativeError("locked", "Unlock Rare Wallet first");
    store({ ...s, blob: await seal(mem.key, { p: payload }, { iter: s.blob.iter, salt: s.blob.salt }) });
    mem = { ...mem, payload };
  },
  async lock() {
    mem = null;
  },
  async verify(secret) {
    return (await check(secret)).payload;
  },
  async changeSecret(current, next, kind) {
    const { payload } = await check(current);
    const { key, blob } = await sealWithPassword(next, { p: payload }, ITER);
    store({ blob, kind, failures: 0, until: 0 });
    mem = { key, payload };
  },
  async reset() {
    store(null);
    mem = null;
  },
};

