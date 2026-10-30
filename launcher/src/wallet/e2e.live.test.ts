// End to end through the real signing API, simulate only, never submitted:
//   LIVE=1 npx vitest run src/wallet/e2e.live.test.ts
// Creates a throwaway phrase account and imports a throwaway private key in a fresh keystore,
// queues a transaction with signAndExecute({ dryRun: true }), approves it the way the review
// sheet does, verifies the signature, and lets a mainnet fullnode simulate it over gRPC.
// The throwaway keys hold nothing. Gas comes from a real funded address as sponsor in the
// simulation only (simulation checks no signatures), so nothing can move.

import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { Secp256r1Keypair } from "@mysten/sui/keypairs/secp256r1";
import { Transaction } from "@mysten/sui/transactions";
import { fromBase64, toBase64 } from "@mysten/sui/utils";
import { verifyTransactionSignature } from "@mysten/sui/verify";
import { beforeAll, describe, expect, it } from "vitest";

const SPONSOR = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

// The launcher keeps public account data in localStorage; give node one.
const mem = new Map<string, string>();
(globalThis as { localStorage?: unknown }).localStorage = {
  getItem: (k: string) => mem.get(k) ?? null,
  setItem: (k: string, v: string) => void mem.set(k, String(v)),
  removeItem: (k: string) => void mem.delete(k),
  clear: () => mem.clear(),
};

describe.skipIf(!process.env.LIVE)("mainnet: create, import, sign, simulate (never submitted)", () => {
  let w: typeof import("./index");
  let sign: typeof import("./sign");
  beforeAll(async () => {
    w = await import("./index");
    sign = await import("./sign");
  });

  /** A no-op transaction for `sender`, gas sponsored by a funded address so the simulation runs. */
  async function noop(sender: string) {
    const tx = new Transaction();
    tx.moveCall({ target: "0x1::option::none", typeArguments: ["u64"] });
    tx.setSender(sender);
    tx.setGasOwner(SPONSOR);
    const coins = await w.grpcClient.core.listCoins({ owner: SPONSOR, coinType: "0x2::sui::SUI", limit: 1 });
    const c = coins.objects[0];
    tx.setGasPayment([{ objectId: c.objectId, version: c.version, digest: c.digest }]);
    tx.setGasBudget(10_000_000n);
    return tx;
  }

  /** What the review sheet does on Approve: review, unlock (already open here), approve with the reviewed bytes. */
  async function approveTop(sender: string) {
    const r = sign.useSignRequests.getState().requests[0];
    expect(r.account).toBe(sender);
    const { reviewTransaction } = await import("./review");
    const review = await reviewTransaction(await sign.requestBytes(r), sender);
    expect(review.ok).toBe(true);
    return sign.approveRequest(r.id, review.bytes!);
  }

  it("phrase account: create, sign through the review queue, simulate", async () => {
    const acc = await w.keystore.addPhrase(undefined, { pin: { secret: "482916", kind: "pin" } });
    await w.keystore.select(acc.address);
    const pending = sign.signAndExecute(await noop(acc.address), { dryRun: true, title: "Test" });
    await new Promise((r) => setTimeout(r, 0));
    const res = await approveTop(acc.address);
    expect(await pending).toEqual(res);
    expect(res.dryRun).toBe(true);
    expect(res.status).toBe("success");
    const pk = await verifyTransactionSignature(fromBase64(res.bytes), res.signature);
    expect(pk.toSuiAddress()).toBe(acc.address);
    console.log(`phrase account ${acc.address}: simulated ${res.digest} (${res.status}), signature verified`);
  });

  it("imported private key: sign through the queue, simulate", async () => {
    const kp = new Ed25519Keypair();
    const acc = await w.keystore.addPrivateKey(kp.getSecretKey(), { label: "Imported" });
    expect(acc.address).toBe(kp.toSuiAddress());
    const pending = sign.signAndExecute(await noop(acc.address), { account: acc.address, dryRun: true });
    await new Promise((r) => setTimeout(r, 0));
    const res = await approveTop(acc.address);
    await pending;
    expect(res.status).toBe("success");
    expect((await verifyTransactionSignature(fromBase64(res.bytes), res.signature)).toSuiAddress()).toBe(acc.address);
    console.log(`imported key ${acc.address}: simulated ${res.digest}, signature verified`);
  });

  it("passkey account (1-of-2 multisig): device key signs, multisig signature verifies for the address", async () => {
    const passkey = new Secp256r1Keypair();
    const device = new Ed25519Keypair();
    const acc = await w.keystore.addPasskey({
      credentialId: toBase64(new Uint8Array([7, 7, 7])),
      publicKey: toBase64(passkey.getPublicKey().toRawBytes()),
      deviceKey: toBase64(device.getPublicKey().toRawBytes()),
      deviceSecret: device.getSecretKey(),
    });
    const pending = sign.signAndExecute(await noop(acc.address), { account: acc.address, dryRun: true });
    await new Promise((r) => setTimeout(r, 0));
    const res = await approveTop(acc.address);
    await pending;
    expect(res.status).toBe("success");
    expect((await verifyTransactionSignature(fromBase64(res.bytes), res.signature)).toSuiAddress()).toBe(acc.address);
    console.log(`passkey multisig ${acc.address}: simulated ${res.digest}, multisig signature verified`);
  });

  it("a declined request rejects and leaves nothing queued", async () => {
    const acc = (await w.keystore.listAccounts())[0];
    const pending = sign.signAndExecute(await noop(acc.address), { account: acc.address, dryRun: true });
    await new Promise((r) => setTimeout(r, 0));
    sign.rejectRequest(sign.useSignRequests.getState().requests[0].id);
    await expect(pending).rejects.toBeInstanceOf(sign.SignRejected);
    expect(sign.useSignRequests.getState().requests).toHaveLength(0);
  });
});
