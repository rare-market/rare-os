// Rare Wallet's door to the Rust side. Every secret operation (the encrypted vault) and every
// hardware wallet transport (Ledger over USB HID and Bluetooth, Keystone over USB) is a Tauri
// command. In a plain browser (UI development, screenshots, tests) the same API answers from a
// local stand-in: a WebCrypto vault in localStorage and no hardware unless `?mockhw` is set.
//
// Errors from Rust come back as "code|message" strings and are rethrown as NativeError, so the
// UI can branch on `code` (e.g. "udev" shows the permission fix, "wrong" counts PIN attempts).

import { webVault } from "./webvault";

/** True in the real app. Release builds fix it to true (lib/mockflag.ts) so no browser stand-in can run or ship. */
export const isTauri: boolean = __RARE_NATIVE__;

export class NativeError extends Error {
  constructor(
    public code: string,
    message: string,
    /** extra data some errors carry (attempts left, retry delay, the udev fix command) */
    public data?: Record<string, unknown>,
  ) {
    super(message);
    this.name = "NativeError";
  }
}

/** "code|message" or "code|message|{json}" from Rust; anything else becomes code "error". */
export function parseNativeError(e: unknown): NativeError {
  if (e instanceof NativeError) return e;
  const s = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  const i = s.indexOf("|");
  if (i <= 0 || i > 24) return new NativeError("error", s);
  const code = s.slice(0, i);
  const rest = s.slice(i + 1);
  const j = rest.indexOf("|{");
  if (j >= 0) {
    try {
      return new NativeError(code, rest.slice(0, j), JSON.parse(rest.slice(j + 1)));
    } catch {
      /* fall through */
    }
  }
  return new NativeError(code, rest);
}

export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw parseNativeError(e);
  }
}

// ---------------------------------------------------------------- vault

export type SecretKind = "pin" | "passphrase";

export interface VaultStatus {
  exists: boolean;
  unlocked: boolean;
  /** where the encrypted blob lives: the OS keyring (Secret Service / KWallet) or a file */
  backend: "keyring" | "file" | null;
  kind: SecretKind | null;
  /** wrong attempts in a row */
  failures: number;
  /** ms until the next attempt is allowed (0 = now) */
  retryAfterMs: number;
}

/**
 * The encrypted key vault. The payload is the wallet's secret JSON (recovery phrases, imported
 * keys). Rust encrypts it with AES-256-GCM under a key from Argon2id(PIN or passphrase) and keeps
 * that key in memory only while unlocked.
 *
 * Errors: "wrong" (data.failures, data.retryAfterMs), "throttled" (data.retryAfterMs),
 * "locked", "exists", "missing", "keyring" (keyring present at create but unavailable now).
 */
export interface VaultPort {
  status(): Promise<VaultStatus>;
  create(secret: string, kind: SecretKind, payload: string): Promise<VaultStatus>;
  /** Unlocks and returns the payload. */
  unlock(secret: string): Promise<string>;
  /** The payload while unlocked (after a UI reload), else null. */
  read(): Promise<string | null>;
  /** Re-encrypts a new payload. Needs the vault unlocked. */
  save(payload: string): Promise<void>;
  lock(): Promise<void>;
  /** Checks the PIN without changing the lock state and returns the payload (reveal phrase). */
  verify(secret: string): Promise<string>;
  changeSecret(current: string, next: string, kind: SecretKind): Promise<void>;
  /** Deletes the vault for good (Settings > Remove wallet). */
  reset(): Promise<void>;
}

const rustVault: VaultPort = {
  status: () => call("vault_status"),
  create: (secret, kind, payload) => call("vault_create", { secret, kind, payload }),
  unlock: (secret) => call("vault_unlock", { secret }),
  read: () => call("vault_read"),
  save: (payload) => call("vault_save", { payload }),
  lock: () => call("vault_lock"),
  verify: (secret) => call("vault_verify", { secret }),
  changeSecret: (current, next, kind) => call("vault_change_secret", { current, next, kind }),
  reset: () => call("vault_reset"),
};

export const vault: VaultPort = isTauri ? rustVault : webVault;

// ---------------------------------------------------------------- Ledger transport

export type LedgerModel = "nanoS" | "nanoSP" | "nanoX" | "stax" | "flex" | "unknown";

export interface LedgerDevice {
  /** "usb:<hidraw path>" or "ble:<bluetooth address>" */
  id: string;
  transport: "usb" | "ble";
  name: string;
  model: LedgerModel;
  /** BLE only: BlueZ already has it paired */
  paired?: boolean;
}

/**
 * Raw APDU transport, the same shape @ledgerhq/hw-transport's `exchange` needs.
 * Errors: "udev" (data.fix = the one-line command), "notfound", "busy", "ble-off",
 * "ble-pairing", "disconnected", "timeout", "cancelled" (ledger_close while waiting).
 * Bluetooth pairing on Linux emits "ledger-pairing" {passkey} (see onLedgerPairing).
 */
export interface LedgerNative {
  /** USB devices now; with `ble`, also scans Bluetooth for `scanMs` (Nano X, Stax, Flex). */
  list(opts?: { ble?: boolean; scanMs?: number }): Promise<LedgerDevice[]>;
  open(id: string): Promise<LedgerDevice>;
  /** One APDU in, the response (data + 2-byte status word) out. */
  exchange(apdu: Uint8Array): Promise<Uint8Array>;
  close(): Promise<void>;
}

const b64 = (u: Uint8Array) => {
  let s = "";
  for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000));
  return btoa(s);
};
const unb64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

const rustLedger: LedgerNative = {
  list: (o) => call("ledger_list", { ble: o?.ble ?? false, scanMs: o?.scanMs ?? 3000 }),
  open: (id) => call("ledger_open", { id }),
  exchange: async (apdu) => unb64(await call<string>("ledger_exchange", { apdu: b64(apdu) })),
  close: () => call("ledger_close"),
};

const mockHw = !isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).has("mockhw");

/** Browser stand-in: no devices, unless ?mockhw, which plugs in a simulated Sui app (see ledger/mock.ts). */
const webLedger: LedgerNative = {
  list: async (o) => {
    if (!mockHw) return [];
    const usb: LedgerDevice[] = [{ id: "usb:mock", transport: "usb", name: "Nano S Plus", model: "nanoSP" }];
    return o?.ble ? [...usb, { id: "ble:mock", transport: "ble", name: "Nano X 4F2A", model: "nanoX", paired: true }] : usb;
  },
  open: async (id) => {
    if (!mockHw) throw new NativeError("notfound", "Connect your Ledger and unlock it");
    return id.startsWith("ble") ? { id, transport: "ble", name: "Nano X 4F2A", model: "nanoX" } : { id, transport: "usb", name: "Nano S Plus", model: "nanoSP" };
  },
  exchange: async (apdu) => {
    const { mockLedgerExchange } = await import("./ledger/mock");
    return mockLedgerExchange(apdu);
  },
  close: async () => {},
};

export const ledgerNative: LedgerNative = isTauri ? rustLedger : webLedger;

/** The 6-digit code BlueZ shows while pairing a Ledger over Bluetooth; the person compares it on the Ledger. */
export async function onLedgerPairing(cb: (passkey: string) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ passkey: string }>("ledger-pairing", (e) => cb(e.payload.passkey));
}

// ---------------------------------------------------------------- Keystone transport

export interface KeystoneDevice {
  id: string;
  name: string;
}

/**
 * Keystone 3 Pro over USB: the EAPDU frames (64-byte packets, 9-byte header) are built in Rust;
 * JS sends a command and a string (a UR) and gets the device's JSON answer text.
 * Errors: "udev", "notfound", "locked" (device locked), "rejected", "timeout", "usb-off",
 * "busy", "mismatch" (another Keystone wallet), "unsupported", "error".
 */
export interface KeystoneNative {
  list(): Promise<KeystoneDevice[]>;
  /** command: 1 echo, 2 resolve UR, 3 lock status, 5 version. Returns the response's JSON text. */
  send(command: number, data: string, timeoutMs?: number): Promise<string>;
}

const rustKeystone: KeystoneNative = {
  list: () => call("keystone_list"),
  send: (command, data, timeoutMs = 180_000) => call("keystone_send", { command, data, timeoutMs }),
};

const webKeystone: KeystoneNative = {
  list: async () => (mockHw ? [{ id: "usb:mock", name: "Keystone 3 Pro" }] : []),
  send: async (command, data) => {
    const { mockKeystoneSend } = await import("./keystone/mock");
    return mockKeystoneSend(command, data);
  },
};

export const keystoneNative: KeystoneNative = isTauri ? rustKeystone : webKeystone;

// ---------------------------------------------------------------- web games (Wallet Standard)

/** A request a web game's injected Rare Wallet sent; the launcher answers with `reply`. */
export interface WebGameRequest {
  id: number;
  method: "connect" | "signTransaction" | "signAndExecuteTransaction" | "signPersonalMessage";
  /** transaction: the Transaction JSON (or base64 bytes); message: base64 */
  params: { transaction?: string; message?: string; chain?: string };
  origin: string;
  game: string;
  /** library id of the game window ("web:act"): its gas budget may pay (category game:<slug>) */
  gameId?: string;
}

export const webgameWallet = {
  /** Answers request `id` in the game window; `value` is JSON (or an error string when !ok). */
  reply: (id: number, ok: boolean, value: unknown): Promise<void> => (isTauri ? call("wallet_reply", { id, ok, value }) : Promise.resolve()),
  /** The launcher must show a sheet for request `id` that Rust expected to answer silently: bring it to the front. */
  focus: (id: number): Promise<void> => (isTauri ? call("wallet_focus", { id }) : Promise.resolve()),
  /** Origins the player approved; Rust lets them see the address and ask for signatures. */
  setApproved: (origins: string[]): Promise<void> => (isTauri ? call("wallet_set_approved", { origins }) : Promise.resolve()),
  onRequest: async (cb: (r: WebGameRequest) => void): Promise<() => void> => {
    if (!isTauri) return () => {};
    const { listen } = await import("@tauri-apps/api/event");
    return listen<WebGameRequest>("wallet-request", (e) => cb(e.payload));
  },
};

// ---------------------------------------------------------------- passkeys (CTAP2 from Rust)

/**
 * WebKitGTK has no platform authenticator on Linux, so the WebAuthn ceremony runs in Rust
 * (webauthn-authenticator-rs): a phone over hybrid/caBLE (QR + Bluetooth + tunnel), or a USB
 * security key over CTAP2 HID. The relying party is always rare.market.
 */
export type PasskeyVia = "phone" | "usb";

export interface PasskeyRegistration {
  /** base64 credential id */
  credentialId: string;
  /** base64 compressed secp256r1 public key (33 bytes), from the attested credential data */
  publicKey: string;
  /** what made it: "phone" or "security-key" */
  authenticator: string;
  /** the passkey can evaluate PRF (WebAuthn prf / CTAP hmac-secret): see `PasskeyNative.prf` */
  prf: boolean;
}

export interface PasskeyAssertion {
  credentialId: string;
  /** base64 authenticatorData */
  authenticatorData: string;
  /** base64 of the exact clientDataJSON bytes the authenticator signed over (sha256) */
  clientDataJSON: string;
  /** base64 DER ECDSA signature, as the authenticator returned it */
  signature: string;
  /** base64 user handle (discoverable credentials), or null */
  userHandle: string | null;
  /** base64 32-byte PRF output for `prfSalt`, or null (none asked, or the passkey has no PRF) */
  prf: string | null;
}

/** Progress of a ceremony, emitted as Tauri event "passkey-event". */
export type PasskeyEvent =
  | { kind: "qr"; url: string } // FIDO:/... for the phone to scan
  | { kind: "status"; stage: "waiting" | "bluetooth" | "connecting" | "touch" | "processing"; message: string }
  | { kind: "pin"; attemptsLeft: number | null } // a security key wants its PIN: answer with passkeyNative.pin()
  | { kind: "error"; code: string; message: string };

export interface PasskeyNative {
  /**
   * Errors: "cancelled", "timeout", "ble-off", "notfound" (no security key, or no Rare passkey on it),
   * "udev" (data.fix), "unsupported", "network" (caBLE tunnel unreachable), "wrong-pin" (security key PIN),
   * "busy" (another ceremony is open), "no-prf" (prf(): the passkey cannot evaluate PRF), "error".
   */
  register(o: { userId: string; userName: string; displayName: string; challenge: string; via: PasskeyVia }): Promise<PasskeyRegistration>;
  /**
   * `credentialIds` empty: discoverable credential (returning user on a new device).
   * `prfSalt`: base64 32-byte WebAuthn PRF input (`prf.eval.first`), evaluated along with the
   * assertion. Rust hashes it into the CTAP salt like browsers do, SHA-256("WebAuthn PRF" || 0x00 || input).
   */
  assert(o: { challenge: string; credentialIds: string[]; via: PasskeyVia; prfSalt?: string }): Promise<PasskeyAssertion>;
  /**
   * One assertion (random challenge) only to evaluate PRF: `salt` is the base64 32-byte WebAuthn
   * PRF input; returns the base64 32-byte output. The same passkey and input always give the
   * same output, on any device and in a browser on rare.market. Error "no-prf" when unsupported.
   */
  prf(credentialIds: string[], salt: string, via: PasskeyVia): Promise<string>;
  /** PIN for a security key that asked for one (event kind "pin"); null cancels. */
  pin(pin: string | null): Promise<void>;
  cancel(): Promise<void>;
  onEvent(cb: (e: PasskeyEvent) => void): Promise<() => void>;
}

const rustPasskey: PasskeyNative = {
  register: (o) => call("passkey_register", o),
  assert: (o) => call("passkey_assert", o),
  prf: (credentialIds, salt, via) => call("passkey_prf", { credentialIds, salt, via }),
  pin: (pin) => call("passkey_pin", { pin }),
  cancel: () => call("passkey_cancel"),
  onEvent: async (cb) => {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<PasskeyEvent>("passkey-event", (e) => cb(e.payload));
  },
};

/** Browser stand-in (dev only): a software authenticator with keys in localStorage, see passkey/soft.ts. */
const webPasskey: PasskeyNative = {
  register: async (o) => (await import("./passkey/soft")).softRegister(o),
  assert: async (o) => (await import("./passkey/soft")).softAssert(o),
  prf: async (credentialIds, salt, via) => (await import("./passkey/soft")).softPrf(credentialIds, salt, via),
  pin: async () => {},
  cancel: async () => {},
  onEvent: async (cb) => (await import("./passkey/soft")).softEvents(cb),
};

export const passkeyNative: PasskeyNative = isTauri ? rustPasskey : webPasskey;

// ---------------------------------------------------------------- Bluetooth

export interface BluetoothState {
  present: boolean;
  powered: boolean;
  blocked: boolean;
}

/**
 * The adapter the phone-passkey (hybrid/caBLE) and Ledger BLE flows need. On the SuiPlay it often
 * starts powered off; powerOn turns it on over BlueZ without sudo. Errors (NativeError.code):
 * "no-adapter", "bt-blocked" (hardware switch), "bt-denied" (needs Settings), "bt-failed".
 */
export const bluetooth = {
  state: (): Promise<BluetoothState> => (isTauri ? call("bluetooth_state") : Promise.resolve({ present: true, powered: true, blocked: false })),
  powerOn: (): Promise<BluetoothState> => (isTauri ? call("bluetooth_power_on") : Promise.resolve({ present: true, powered: true, blocked: false })),
};
