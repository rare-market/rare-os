import { describe, expect, it } from "vitest";
import { bytesToHex } from "@noble/hashes/utils.js";
import { keypairFromMnemonic } from "./core/keys/derive";
import { Keystore } from "./core/keys/keystore";
import { MemoryKV } from "./core/store/kv";
import { memoryVault } from "./testing/memoryVault";
import { DERIVE_SALT, deriveInfo, deriveMessage, ed25519SignatureBytes, finish, prfInput } from "./secret";

const PHRASE = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const ADDR = "0x5e93a736d04fbb25737aa40bee40171ef79f65fae833749e3c089fe7cc2161f1";

// Pinned vectors: if any of these change, every key derived on a device changes with them
// (cloud saves would no longer open). Change only with a new DERIVE_SALT version.
const V = {
  prfInput: "e98ccacee8c68d9008a2fe288bdf97b5ffe3e2c855bc02de82f261e45060ecbe",
  phrase: "22d6b803437a60cca9d5566218a1c8464c29da5859ea69fb6eac7895976a076c",
  ledger: "e5c10994260a631eea86a4252e3984ac69de5c45e9c6c486d4f1db687b3e1bf3",
};

describe("deriveSecret building blocks", () => {
  it("PRF input is SHA-256 of rare/<context>", () => {
    expect(bytesToHex(prfInput("cloud-saves"))).toBe(V.prfInput);
  });

  it("phrase accounts: HKDF over the private key, same on any device, per context and account", async () => {
    const mk = async () => {
      const ks = new Keystore({ disk: new MemoryKV(), vault: memoryVault() });
      const acc = await ks.addPhrase(PHRASE, { pin: { secret: "123456", kind: "pin" } });
      return { ks, acc };
    };
    const a = await mk();
    const b = await mk();
    expect(a.acc.address).toBe(ADDR);
    const s1 = await a.ks.deriveFromKey(ADDR, DERIVE_SALT, deriveInfo("cloud-saves", ADDR));
    const s2 = await b.ks.deriveFromKey(ADDR, DERIVE_SALT, deriveInfo("cloud-saves", ADDR));
    expect(bytesToHex(s1)).toBe(bytesToHex(s2));
    expect(bytesToHex(s1)).toBe(V.phrase);
    const other = await a.ks.deriveFromKey(ADDR, DERIVE_SALT, deriveInfo("other", ADDR));
    expect(bytesToHex(other)).not.toBe(bytesToHex(s1));
    const next = await a.ks.deriveNext();
    const s3 = await a.ks.deriveFromKey(next.address, DERIVE_SALT, deriveInfo("cloud-saves", next.address));
    expect(bytesToHex(s3)).not.toBe(bytesToHex(s1));
  });

  it("refuses accounts without a local key and a locked vault", async () => {
    const ks = new Keystore({ disk: new MemoryKV(), vault: memoryVault() });
    const w = await ks.addWatch(ADDR);
    await expect(ks.deriveFromKey(w.address, DERIVE_SALT, deriveInfo("x", w.address))).rejects.toThrow();
  });

  it("Ledger/Keystone: the hashed Ed25519 signature over the fixed message is deterministic", async () => {
    // The device holds the same kind of key; Ed25519 (RFC 8032) signs deterministically.
    const kp = keypairFromMnemonic(PHRASE);
    const sign = async () => (await kp.signPersonalMessage(deriveMessage("cloud-saves", ADDR))).signature;
    const [x, y] = [await sign(), await sign()];
    const s = finish(ed25519SignatureBytes(x), "cloud-saves", ADDR);
    expect(bytesToHex(s)).toBe(bytesToHex(finish(ed25519SignatureBytes(y), "cloud-saves", ADDR)));
    expect(bytesToHex(s)).toBe(V.ledger);
  });

  it("refuses non-Ed25519 signatures for the device path", () => {
    expect(() => ed25519SignatureBytes(btoa(String.fromCharCode(0x02, ...new Uint8Array(96))))).toThrow();
  });
});
