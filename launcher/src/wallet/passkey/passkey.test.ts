import { describe, expect, it } from "vitest";
import { Ed25519PublicKey } from "@mysten/sui/keypairs/ed25519";
import { fromBase64, toBase64, toHex } from "@mysten/sui/utils";
import { sha256 } from "@noble/hashes/sha2.js";
import { verifyPersonalMessageSignature, verifyTransactionSignature } from "@mysten/sui/verify";
import { parseSerializedSignature } from "@mysten/sui/cryptography";
import { Keystore } from "../core/keys/keystore";
import { MemoryKV } from "../core/store/kv";
import { memoryVault } from "../testing/memoryVault";
import { passkeyMultisig } from "../core/passkey/multisig";
import type { PasskeyNative, PasskeyVia } from "../native";
import type { VaultPort } from "../native";
import { createSoftAuthenticator, prfSaltFor, type SoftAuthenticator, type SoftStorage } from "./soft";
import { deviceKeyFromUserHandle, passkeyService, recoverPasskeyKeys, type HistoryClient } from "./index";

const PIN = { secret: "123456", kind: "pin" as const };

function memStorage(): SoftStorage {
  const m = new Map<string, string>();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
}

/** The soft authenticator behind the PasskeyNative contract, counting assertions. */
function nativeFrom(soft: SoftAuthenticator) {
  const calls = { register: 0, assert: [] as string[][], prf: [] as { ids: string[]; via: PasskeyVia }[] };
  const native: PasskeyNative = {
    register: (o) => (calls.register++, soft.register(o)),
    assert: (o) => (calls.assert.push(o.credentialIds), soft.assert(o)),
    prf: (ids, salt, via) => (calls.prf.push({ ids, via }), soft.prf(ids, salt, via)),
    pin: async () => {},
    cancel: async () => {},
    onEvent: (cb) => soft.events(cb),
  };
  return { native, calls };
}

/** gRPC stub: only `funded` addresses have anything on chain. */
function chain(funded: string[] = []) {
  const asked: string[] = [];
  const grpc: HistoryClient = {
    core: {
      listBalances: async ({ owner }) => (asked.push(owner), { balances: funded.includes(owner) || funded.includes("*") ? [{ balance: "1000" }] : [] }),
      listOwnedObjects: async ({ owner }) => (asked.push(owner), { objects: [] }),
    },
  };
  return { grpc, asked };
}

function device(soft: SoftAuthenticator, o: { funded?: string[]; vault?: VaultPort; via?: PasskeyVia } = {}) {
  const ks = new Keystore({ disk: new MemoryKV(), vault: o.vault ?? memoryVault() });
  const { native, calls } = nativeFrom(soft);
  const { grpc, asked } = chain(o.funded);
  const svc = passkeyService({ keystore: ks, native, grpc, via: () => o.via ?? "phone" });
  return { ks, svc, calls, asked };
}

const msg = new TextEncoder().encode("rare passkey test");
const txBytes = new Uint8Array(Array.from({ length: 96 }, (_, i) => (i * 13) & 0xff));

describe("passkey accounts", () => {
  it("create: address is multisig(passkey, device key); the device key signs without the passkey", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const { ks, svc, calls } = device(soft);
    const events: string[] = [];
    await soft.events((e) => events.push(e.kind === "status" ? `status:${e.stage}` : e.kind));

    const acc = await svc.createPasskeyAccount({ via: "phone", label: "Main", pin: PIN });
    expect(calls.register).toBe(1);
    expect(events[0]).toBe("qr");
    expect(events).toContain("status:touch");
    expect(acc.kind).toBe("passkey");
    expect(acc.label).toBe("Main");
    expect(acc.passkey?.authenticator).toBe("phone");
    const expected = passkeyMultisig(fromBase64(acc.passkey!.publicKey), fromBase64(acc.passkey!.deviceKey)).toSuiAddress();
    expect(acc.address).toBe(expected);
    expect(await ks.hasDeviceKey(acc.address)).toBe(true);
    expect((await ks.current())?.address).toBe(acc.address);

    // everyday signing: the device member, no assertion
    const signer = await ks.getSigner(acc.address);
    const pm = await signer.signPersonalMessage(msg);
    expect((await verifyPersonalMessageSignature(msg, pm.signature)).toSuiAddress()).toBe(acc.address);
    const tx = await signer.signTransaction(txBytes);
    expect((await verifyTransactionSignature(txBytes, tx)).toSuiAddress()).toBe(acc.address);
    expect(calls.assert.length).toBe(0);
  });

  it("refuses to start a ceremony it could not save (no PIN for a new vault)", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const { svc, calls } = device(soft);
    await expect(svc.createPasskeyAccount({ via: "usb" })).rejects.toThrow("PIN");
    expect(calls.register).toBe(0);
  });

  it("passkey member signature (through MultisigMemberSigner) verifies for the account", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const { ks, svc, calls } = device(soft);
    const acc = await svc.createPasskeyAccount({ via: "usb", pin: PIN });
    const signer = await svc.signerProvider(acc, "");
    expect(signer.address).toBe(acc.address);

    const pm = await signer.signPersonalMessage(msg);
    expect((await verifyPersonalMessageSignature(msg, pm.signature)).toSuiAddress()).toBe(acc.address);
    const tx = await signer.signTransaction(txBytes);
    expect((await verifyTransactionSignature(txBytes, tx)).toSuiAddress()).toBe(acc.address);
    // one assertion per signature, limited to this account's credential
    expect(calls.assert).toEqual([[acc.passkey!.credentialId], [acc.passkey!.credentialId]]);
    // the multisig carries a passkey member signature (flag 0x06), not the device key's
    const parsed = parseSerializedSignature(tx);
    expect(parsed.signatureScheme).toBe("MultiSig");
    expect(Object.keys((parsed as { multisig: { sigs: object[] } }).multisig.sigs[0]).filter((k) => k !== "$kind")).toEqual(["Passkey"]);
    void ks;
  });

  it("sign in on a new device: rebuilds the same address from the user handle and recovery", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const a = device(soft);
    const created = await a.svc.createPasskeyAccount({ via: "phone", pin: PIN });

    // a fresh SuiPlay: empty keystore, no vault, nothing on chain -> second assertion decides
    const b = device(soft);
    const acc = await b.svc.signInWithPasskey({ via: "phone" });
    expect(acc.address).toBe(created.address);
    expect(acc.passkey).toMatchObject({ credentialId: created.passkey!.credentialId, publicKey: created.passkey!.publicKey, deviceKey: created.passkey!.deviceKey });
    expect(b.calls.assert).toEqual([[], [created.passkey!.credentialId]]);
    expect(await b.ks.hasDeviceKey(acc.address)).toBe(false);

    // no device key here: getSigner signs with the passkey, and it verifies
    const signer = await b.ks.getSigner(acc.address);
    const pm = await signer.signPersonalMessage(msg);
    expect((await verifyPersonalMessageSignature(msg, pm.signature)).toSuiAddress()).toBe(created.address);
  });

  it("chain history picks the account and skips the second assertion", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const created = await device(soft).svc.createPasskeyAccount({ via: "usb", pin: PIN });
    const b = device(soft, { funded: [created.address] });
    const acc = await b.svc.signInWithPasskey({ via: "usb" });
    expect(acc.address).toBe(created.address);
    expect(acc.passkey?.authenticator).toBe("security-key");
    expect(b.calls.assert).toEqual([[]]);
    // every candidate was checked, and the real one was among them
    expect(b.asked).toContain(created.address);
    expect(new Set(b.asked).size).toBeGreaterThanOrEqual(1);
  });

  it("history on more than one candidate falls back to the second assertion", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const created = await device(soft).svc.createPasskeyAccount({ via: "phone", pin: PIN });
    const b = device(soft, { funded: ["*"] }); // every candidate looks used
    const acc = await b.svc.signInWithPasskey({ via: "phone" });
    expect(acc.address).toBe(created.address);
    const candidates = new Set(b.asked).size;
    // two candidates (the usual case) both with history: ask once more, same credential
    expect(b.calls.assert).toEqual(candidates > 1 ? [[], [created.passkey!.credentialId]] : [[]]);
  });

  it("returning user on this device: no recovery, no chain reads, same account", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const a = device(soft);
    const created = await a.svc.createPasskeyAccount({ via: "phone", pin: PIN });
    const again = await a.svc.signInWithPasskey({ via: "phone" });
    expect(again.address).toBe(created.address);
    expect(a.calls.assert).toEqual([[]]);
    expect(a.asked).toEqual([]);
    expect((await a.ks.listAccounts()).length).toBe(1);
  });

  it("account list lost but vault kept: the device key in the vault names the account", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const vault = memoryVault();
    const a = device(soft, { vault });
    const created = await a.svc.createPasskeyAccount({ via: "phone", pin: PIN });

    const b = device(soft, { vault }); // same unlocked vault, empty account list
    const acc = await b.svc.signInWithPasskey({ via: "phone" });
    expect(acc.address).toBe(created.address);
    expect(b.calls.assert).toEqual([[]]);
    expect(b.asked).toEqual([]);
    expect(await b.ks.hasDeviceKey(acc.address)).toBe(true);
    // and it signs with the device key again, without the passkey
    const s = await b.ks.getSigner(acc.address);
    await s.signPersonalMessage(msg);
    expect(b.calls.assert.length).toBe(1);
  });

  it("rejects passkeys Rare did not make and answers to other requests", async () => {
    expect(deviceKeyFromUserHandle(null)).toBeNull();
    expect(deviceKeyFromUserHandle(Buffer.from([2, ...new Uint8Array(32)]).toString("base64"))).toBeNull();
    const pk = new Uint8Array(32).fill(7);
    expect(deviceKeyFromUserHandle(Buffer.from([1, ...pk]).toString("base64"))).toEqual(pk);
    expect(new Ed25519PublicKey(pk).toRawBytes()).toEqual(pk);

    const soft = createSoftAuthenticator({ storage: memStorage() });
    await device(soft).svc.createPasskeyAccount({ via: "phone", pin: PIN });
    const { native } = nativeFrom(soft);
    const challenge = new Uint8Array(32).fill(1);
    const as = await native.assert({ challenge: Buffer.from(challenge).toString("base64"), credentialIds: [], via: "phone" });
    expect(() => recoverPasskeyKeys(as, new Uint8Array(32).fill(2))).toThrow("different request");
    expect(recoverPasskeyKeys(as, challenge).length).toBeGreaterThanOrEqual(1);
  });

  it("PRF: same input, same output on any device; different inputs differ; follows the soft salt convention", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const a = device(soft, { via: "usb" });
    const acc = await a.svc.createPasskeyAccount({ via: "phone", pin: PIN });
    const input = sha256(new TextEncoder().encode("rare/cloud-save"));
    const events: string[] = [];
    await soft.events((e) => events.push(e.kind));

    const one = await a.svc.passkeyPrf(acc, input);
    expect(one.length).toBe(32);
    expect(a.calls.prf).toEqual([{ ids: [acc.passkey!.credentialId], via: "usb" }]);
    expect(events).toContain("status");

    // another SuiPlay signs in with the same passkey and gets the same secret
    const b = device(soft, { via: "phone" });
    const again = await b.svc.signInWithPasskey({ via: "phone" });
    expect(again.address).toBe(acc.address);
    const two = await b.svc.passkeyPrf(again, input);
    expect(toHex(two)).toBe(toHex(one));

    const other = await b.svc.passkeyPrf(again, sha256(new TextEncoder().encode("rare/other")));
    expect(toHex(other)).not.toBe(toHex(one));

    await expect(a.svc.passkeyPrf(acc, new Uint8Array(16))).rejects.toThrow("32 bytes");

    // the assertion carries the PRF output too, as HMAC(credential secret, SHA-256("WebAuthn PRF\0" || input))
    const { native } = nativeFrom(soft);
    const as = await native.assert({ challenge: toBase64(new Uint8Array(32)), credentialIds: [acc.passkey!.credentialId], via: "usb", prfSalt: toBase64(input) });
    expect(as.prf && toHex(fromBase64(as.prf))).toBe(toHex(one));
    // the same salt as the Rust side and python-fido2 (vector in src-tauri/src/wallet/passkey/tests.rs)
    expect(toHex(prfSaltFor(input))).toBe("7f8501bdda5b0bbaeb9ca1bcebfdef8e2a5570d4e3c30f4239b442b73d2ac48c");
    const noPrf = await native.assert({ challenge: toBase64(new Uint8Array(32)), credentialIds: [], via: "usb" });
    expect(noPrf.prf).toBeNull();
  });

  it("PRF needs a passkey account", async () => {
    const soft = createSoftAuthenticator({ storage: memStorage() });
    const { svc } = device(soft);
    await expect(svc.passkeyPrf({ address: "0x1", label: "w", kind: "watch" }, new Uint8Array(32))).rejects.toThrow("Not a passkey account");
  });
});
