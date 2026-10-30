import { describe, expect, it } from "vitest";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { Secp256r1Keypair } from "@mysten/sui/keypairs/secp256r1";
import { decodeSuiPrivateKey } from "@mysten/sui/cryptography";
import { deriveSaveKey, open, SAVES_CONTEXT, seal, WrongKey } from "./crypto";
import { finish } from "../../wallet/secret";
import { latestBySet, openMeta, sealMeta, type Backup, type BackupMeta } from "./cloud";

/** What Rare Wallet's deriveSecret("cloud-saves") gives a phrase or imported-key account. */
async function keyFor(kp: Ed25519Keypair | Secp256r1Keypair) {
  const a = kp.toSuiAddress();
  return deriveSaveKey(finish(decodeSuiPrivateKey(kp.getSecretKey()).secretKey, SAVES_CONTEXT, a), a);
}

describe("save keys", () => {
  it("the same account derives the same key on any device", async () => {
    const kp = new Ed25519Keypair();
    const again = Ed25519Keypair.fromSecretKey(kp.getSecretKey());
    const [a, b] = await Promise.all([keyFor(kp), keyFor(again)]);
    expect(a.id).toBe(b.id);
    const sealed = await seal(a, new TextEncoder().encode("memory card"));
    expect(new TextDecoder().decode(await open(b, sealed))).toBe("memory card");
  });
  it("another account can't open it", async () => {
    const a = await keyFor(new Ed25519Keypair());
    const b = await keyFor(new Ed25519Keypair());
    expect(a.id).not.toBe(b.id);
    await expect(open(b, await seal(a, new Uint8Array([1, 2, 3])))).rejects.toBeInstanceOf(WrongKey);
  });
  it("works for every key scheme (no signature needed)", async () => {
    const kp = new Secp256r1Keypair();
    const [a, b] = await Promise.all([keyFor(kp), keyFor(Secp256r1Keypair.fromSecretKey(kp.getSecretKey()))]);
    expect(a.id).toBe(b.id);
  });
  it("refuses anything but a 32-byte secret", async () => {
    await expect(deriveSaveKey(new Uint8Array(16), "0x1")).rejects.toThrow("32-byte");
  });
  it("ciphertext never contains the plaintext and each seal is unique", async () => {
    const k = await keyFor(new Ed25519Keypair());
    const plain = new TextEncoder().encode("SAVEGAME-SAVEGAME-SAVEGAME");
    const [x, y] = [await seal(k, plain), await seal(k, plain)];
    expect(Buffer.from(x).includes(Buffer.from("SAVEGAME"))).toBe(false);
    expect(Buffer.from(x).equals(Buffer.from(y))).toBe(false);
  });
});

describe("backup descriptions", () => {
  const meta = (id: string, created: number): BackupMeta => ({ v: 1, id, rel: ".config/x", emulator: "X", label: "Saves", kind: "emulator", files: 1, bytes: 10, created });
  it("round-trip encrypted", async () => {
    const k = await keyFor(new Ed25519Keypair());
    const m = meta("rpcs3", 5);
    expect(await openMeta(k, await sealMeta(k, m))).toEqual(m);
    expect(await openMeta(await keyFor(new Ed25519Keypair()), await sealMeta(k, m))).toBeNull();
  });
  it("latest backup per save set", () => {
    const b = (id: string, created: number): Backup => ({ blobObjectId: `${id}${created}`, blobId: "x", size: 1, endEpoch: null, meta: meta(id, created) });
    const m = latestBySet([b("a", 1), b("a", 3), b("b", 2), b("a", 2)]);
    expect(m.get("a")!.meta.created).toBe(3);
    expect(m.get("b")!.meta.created).toBe(2);
  });
});
