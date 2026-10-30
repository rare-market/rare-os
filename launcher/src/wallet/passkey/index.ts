// Passkey accounts for Rare Wallet: what the passkey screens call.
//
// A passkey account is a 1-of-2 Sui multisig of the person's passkey (secp256r1, on a phone or a
// security key, rp rare.market) and an Ed25519 device key sealed in the PIN vault on this SuiPlay
// (see core/passkey/multisig.ts). The passkey's user handle is 0x01 || the device public key, so a
// new device can rebuild the address from one sign-in: the user handle gives the device key, and
// the passkey public key is recovered from the assertion signature.
//
// The ceremony itself runs in Rust (CTAP2 over hybrid/caBLE or USB, see native.ts); in a browser
// build native.ts answers from the dev-only soft authenticator in ./soft.ts.

import { create } from "zustand";
import { p256 } from "@noble/curves/nist.js";
import { sha256 } from "@noble/hashes/sha2.js";
import { randomBytes } from "@noble/hashes/utils.js";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { findCommonPublicKey, PasskeyKeypair, PasskeyPublicKey, type PasskeyProvider } from "@mysten/sui/keypairs/passkey";
import { fromBase64, toBase64 } from "@mysten/sui/utils";
import { grpcClient, keystore } from "../index";
import { passkeyNative, type PasskeyAssertion, type PasskeyEvent, type PasskeyNative, type PasskeyVia } from "../native";
import { LockedError, NeedsPasswordError, type Keystore, type PinOptions, type SignerProvider } from "../core/keys/keystore";
import { MultisigMemberSigner, passkeyMultisig } from "../core/passkey/multisig";
import type { RareAccount } from "../core/types";

export const RP_ID = "rare.market";
/** First byte of the user handle: layout version (0x01 = device Ed25519 public key follows). */
const USER_HANDLE_V1 = 0x01;

// ---------------------------------------------------------------- prefs: phone or USB key

const VIA_KEY = "rare.passkey.via"; // same key the passkey page reads

function loadVia(): PasskeyVia {
  try {
    return localStorage.getItem(VIA_KEY) === "usb" ? "usb" : "phone";
  } catch {
    return "phone";
  }
}

/** How passkey signatures are asked for: "Scan with your phone" or "Touch your security key". */
export const usePasskeyPrefs = create<{ via: PasskeyVia; setVia(v: PasskeyVia): void }>((set) => ({
  via: loadVia(),
  setVia(via) {
    try {
      localStorage.setItem(VIA_KEY, via);
    } catch {
      /* private mode */
    }
    set({ via });
  },
}));

// ---------------------------------------------------------------- live ceremony state for the UI

export type CeremonyState =
  | { kind: "idle" }
  | { kind: "qr"; url: string }
  | { kind: "status"; stage: Extract<PasskeyEvent, { kind: "status" }>["stage"]; message: string }
  | { kind: "pin"; attemptsLeft: number | null }
  | { kind: "error"; code: string; message: string };

interface CeremonyStore {
  state: CeremonyState;
  /** the last QR of this ceremony; kept while later status events arrive */
  qrUrl: string | null;
  reset(): void;
}

export const ceremonyStore = create<CeremonyStore>((set) => ({
  state: { kind: "idle" },
  qrUrl: null,
  reset: () => set({ state: { kind: "idle" }, qrUrl: null }),
}));

let listening = false;
function listen(native: PasskeyNative) {
  if (listening) return;
  listening = true;
  void native.onEvent((e) => ceremonyStore.setState((s) => ({ state: e, qrUrl: e.kind === "qr" ? e.url : s.qrUrl })));
}

/** Current passkey ceremony (QR, status, PIN prompt, error) for the screens. */
export function usePasskeyCeremony(): CeremonyStore {
  listen(passkeyNative);
  return ceremonyStore();
}

// ---------------------------------------------------------------- helpers

const b64url = (b: Uint8Array) => toBase64(b).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const eq = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((v, i) => v === b[i]);

/** user.id for a new passkey: 0x01 || the 32-byte Ed25519 device public key. */
export function userHandleFor(devicePublicKey: Uint8Array): Uint8Array {
  const u = new Uint8Array(33);
  u[0] = USER_HANDLE_V1;
  u.set(devicePublicKey, 1);
  return u;
}

/** The device public key inside a Rare user handle, or null for a passkey Rare did not make. */
export function deviceKeyFromUserHandle(userHandle: string | null): Uint8Array | null {
  if (!userHandle) return null;
  const u = fromBase64(userHandle);
  return u.length === 33 && u[0] === USER_HANDLE_V1 ? u.slice(1) : null;
}

/**
 * Checks an assertion is ours (rare.market, our challenge, a get) and returns every public key
 * its signature could have come from (P-256 recovery: usually two).
 */
export function recoverPasskeyKeys(a: PasskeyAssertion, challenge: Uint8Array): PasskeyPublicKey[] {
  const authData = fromBase64(a.authenticatorData);
  const cdjBytes = fromBase64(a.clientDataJSON);
  const cdj = JSON.parse(new TextDecoder().decode(cdjBytes)) as { type?: string; challenge?: string };
  if (cdj.type !== "webauthn.get" || cdj.challenge !== b64url(challenge)) throw new Error("The passkey answered a different request");
  if (authData.length < 37 || !eq(authData.slice(0, 32), sha256(new TextEncoder().encode(RP_ID)))) throw new Error("This passkey is not a Rare passkey");
  const msgHash = sha256(new Uint8Array([...authData, ...sha256(cdjBytes)]));
  const sig = p256.Signature.fromBytes(fromBase64(a.signature), "der");
  const out: PasskeyPublicKey[] = [];
  for (let bit = 0; bit < 4; bit++) {
    try {
      const pk = new PasskeyPublicKey(sig.addRecoveryBit(bit).recoverPublicKey(msgHash).toBytes(true));
      if (!out.some((k) => k.equals(pk))) out.push(pk);
    } catch {
      /* not a valid point for this bit */
    }
  }
  if (!out.length) throw new Error("Could not read the passkey signature");
  return out;
}

/** The gRPC calls used to tell candidate addresses apart (anything on chain = the real one). */
export interface HistoryClient {
  core: {
    listBalances(o: { owner: string; limit?: number }): Promise<{ balances: { balance: string | bigint }[] }>;
    listOwnedObjects(o: { owner: string; limit?: number }): Promise<{ objects: unknown[] }>;
  };
}

async function hasHistory(grpc: HistoryClient, address: string): Promise<boolean> {
  const [bal, objs] = await Promise.all([
    grpc.core.listBalances({ owner: address, limit: 5 }).catch(() => null),
    grpc.core.listOwnedObjects({ owner: address, limit: 1 }).catch(() => null),
  ]);
  return !!bal?.balances.some((b) => BigInt(b.balance) > 0n) || (objs?.objects.length ?? 0) > 0;
}

// ---------------------------------------------------------------- the service

export interface PasskeyDeps {
  keystore: Keystore;
  native: PasskeyNative;
  grpc: HistoryClient;
  /** phone or USB key for signatures; defaults to the remembered choice */
  via?: () => PasskeyVia;
}

export interface PasskeyService {
  createPasskeyAccount(o: { via: PasskeyVia; label?: string; pin?: PinOptions }): Promise<RareAccount>;
  signInWithPasskey(o: { via: PasskeyVia; label?: string }): Promise<RareAccount>;
  passkeyPrf(account: RareAccount, input32: Uint8Array): Promise<Uint8Array>;
  signerProvider: SignerProvider;
}

const authenticatorName = (via: PasskeyVia) => (via === "usb" ? "security-key" : "phone");

export function passkeyService(d: PasskeyDeps): PasskeyService {
  const ks = d.keystore;
  const viaNow = d.via ?? (() => usePasskeyPrefs.getState().via);

  async function assert(challenge: Uint8Array, credentialIds: string[], via: PasskeyVia) {
    ceremonyStore.getState().reset();
    return d.native.assert({ challenge: toBase64(challenge), credentialIds, via });
  }

  /** Make a passkey and the account. The vault must be unlocked, or `pin` creates it. */
  async function createPasskeyAccount(o: { via: PasskeyVia; label?: string; pin?: PinOptions }): Promise<RareAccount> {
    // check before the ceremony, so a phone never ends up with a passkey for an account we drop
    if (await ks.hasVault()) {
      if (!(await ks.isUnlocked())) throw new LockedError();
    } else if (!o.pin) throw new NeedsPasswordError();

    const device = new Ed25519Keypair();
    const devicePk = device.getPublicKey().toRawBytes();
    const name = o.label?.trim() || "Rare Wallet";
    ceremonyStore.getState().reset();
    const reg = await d.native.register({
      userId: toBase64(userHandleFor(devicePk)),
      userName: name,
      displayName: name,
      challenge: toBase64(randomBytes(32)),
      via: o.via,
    });
    const acc = await ks.addPasskey({
      credentialId: reg.credentialId,
      publicKey: reg.publicKey,
      deviceKey: toBase64(devicePk),
      deviceSecret: device.getSecretKey(),
      label: o.label,
      authenticator: reg.authenticator || authenticatorName(o.via),
      pin: o.pin,
    });
    await ks.select(acc.address);
    return acc;
  }

  /**
   * Sign in with an existing Rare passkey, on this device or a new one. One discoverable
   * assertion gives the device key (user handle) and two or so candidate passkey keys; the
   * account is the candidate already known here, or the one whose device key is in the vault,
   * or the only one with on-chain history. Only when none of that decides does the passkey sign
   * a second time (same credential), and the key common to both signatures wins.
   */
  async function signInWithPasskey(o: { via: PasskeyVia; label?: string }): Promise<RareAccount> {
    const c1 = randomBytes(32);
    const a1 = await assert(c1, [], o.via);
    const deviceKey = deviceKeyFromUserHandle(a1.userHandle);
    if (!deviceKey) throw new Error("This passkey was not made in Rare Wallet. Pick your Rare passkey.");
    const deviceKeyB64 = toBase64(deviceKey);

    const accounts = await ks.listAccounts();
    const same = accounts.find((a) => a.kind === "passkey" && a.passkey?.deviceKey === deviceKeyB64 && a.passkey.credentialId === a1.credentialId);
    if (same) {
      await ks.select(same.address);
      return same;
    }

    const candidates = recoverPasskeyKeys(a1, c1).map((pk) => ({ pk, address: passkeyMultisig(pk.toRawBytes(), deviceKey).toSuiAddress() }));
    const known = candidates.find((c) => accounts.some((a) => a.address === c.address));
    if (known) {
      await ks.select(known.address);
      return accounts.find((a) => a.address === known.address)!;
    }

    let pick = candidates.length === 1 ? candidates[0] : undefined;
    if (!pick) {
      // the vault outlived the account list (UI data cleared): its device key names the account
      const withKey = [];
      for (const c of candidates) if (await ks.hasDeviceKey(c.address).catch(() => false)) withKey.push(c);
      if (withKey.length === 1) pick = withKey[0];
    }
    if (!pick) {
      const used = await Promise.all(candidates.map((c) => hasHistory(d.grpc, c.address)));
      const withHistory = candidates.filter((_, i) => used[i]);
      if (withHistory.length === 1) pick = withHistory[0];
    }
    if (!pick) {
      const c2 = randomBytes(32);
      const a2 = await assert(c2, [a1.credentialId], o.via);
      const common = findCommonPublicKey(
        candidates.map((c) => c.pk),
        recoverPasskeyKeys(a2, c2),
      );
      pick = candidates.find((c) => eq(c.pk.toRawBytes(), common.toRawBytes()));
      if (!pick) throw new Error("Could not identify the passkey account");
    }

    const acc = await ks.addPasskey({
      credentialId: a1.credentialId,
      publicKey: toBase64(pick.pk.toRawBytes()),
      deviceKey: deviceKeyB64,
      label: o.label,
      authenticator: authenticatorName(o.via),
    });
    await ks.select(acc.address);
    return acc;
  }

  /** Signs as the account's passkey member (a phone tap or key touch per signature). */
  const signerProvider: SignerProvider = async (account) => {
    if (!account.passkey) throw new Error("Not a passkey account");
    const credentialId = account.passkey.credentialId;
    const provider: PasskeyProvider = {
      async create() {
        throw new Error("Passkeys are created with createPasskeyAccount");
      },
      async get(challenge) {
        const a = await assert(challenge, [credentialId], viaNow());
        return {
          response: {
            authenticatorData: fromBase64(a.authenticatorData),
            clientDataJSON: fromBase64(a.clientDataJSON),
            signature: fromBase64(a.signature),
          },
        } as unknown as Awaited<ReturnType<PasskeyProvider["get"]>>;
      },
    };
    const kp = new PasskeyKeypair(fromBase64(account.passkey.publicKey), provider, fromBase64(credentialId));
    return new MultisigMemberSigner(account, {
      signTransaction: async (b) => (await kp.signTransaction(b)).signature,
      signPersonalMessage: async (b) => (await kp.signPersonalMessage(b)).signature,
    });
  };

  /**
   * The passkey's PRF output for a 32-byte WebAuthn PRF input (Rare uses SHA-256("rare/" + context)).
   * Deterministic per passkey and input, on any SuiPlay and in a browser on rare.market. One
   * phone scan or key touch, shown through the same ceremony state. Error "no-prf" if unsupported.
   */
  async function passkeyPrf(account: RareAccount, input32: Uint8Array): Promise<Uint8Array> {
    if (!account.passkey) throw new Error("Not a passkey account");
    if (input32.length !== 32) throw new Error("The PRF input must be 32 bytes");
    ceremonyStore.getState().reset();
    const out = fromBase64(await d.native.prf([account.passkey.credentialId], toBase64(input32), viaNow()));
    if (out.length !== 32) throw new Error("The passkey sent a bad PRF output");
    return out;
  }

  // accounts without the device key here sign through the passkey
  ks.setProvider("passkey", signerProvider);
  return { createPasskeyAccount, signInWithPasskey, passkeyPrf, signerProvider };
}

// ---------------------------------------------------------------- the launcher's instance
// Importing this module registers the passkey signer on the launcher's keystore.

const live = passkeyService({ keystore, native: passkeyNative, grpc: grpcClient as unknown as HistoryClient });

export const createPasskeyAccount = live.createPasskeyAccount;
export const signInWithPasskey = live.signInWithPasskey;
export const passkeySignerProvider = live.signerProvider;
export const passkeyPrf = live.passkeyPrf;
