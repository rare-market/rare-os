// End-to-end cloud saves on Walrus TESTNET with a throwaway key: faucet SUI, swap a little for
// testnet WAL, back up an encrypted archive through the upload relay, find it again from chain
// alone (as a new device would), download, decrypt, compare. Never touches mainnet or real funds.
//
//   RARE_WALRUS_E2E=1 npx vitest run src/sui/saves/walrus.e2e.test.ts
//
// The public testnet faucet refuses scripts, so RARE_WALRUS_E2E_KEYS can name a JSON file of
// testnet-only keys (["suiprivkey..."]); the first one pays. Without it a fresh key asks the faucet.
// Keys are never printed or saved.

import { describe, expect, it } from "vitest";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { decodeSuiPrivateKey } from "@mysten/sui/cryptography";
import { deriveSaveKey, SAVES_CONTEXT } from "./crypto";
import { finish } from "../../wallet/secret";
import { backUpSet, fetchBackup, listBackups } from "./cloud";
import { estimate } from "./walrus";
import { fundWithWal, keypairSigner, testKey } from "./testnet";

const run = process.env.RARE_WALRUS_E2E === "1";

describe.skipIf(!run)("Walrus testnet cloud saves", () => {
  it(
    "backs up, lists from chain and restores an encrypted save",
    async () => {
      const kp = testKey();
      const signer = keypairSigner(kp);
      await fundWithWal(kp);

      const archive = crypto.getRandomValues(new Uint8Array(48_000));
      const est = await estimate(archive.length, 2, "testnet");
      expect(est.walFrost).toBeGreaterThan(0n);

      // What deriveSecret("cloud-saves") gives this key's account.
      const secretOf = (k: Ed25519Keypair) => finish(decodeSuiPrivateKey(k.getSecretKey()).secretKey, SAVES_CONTEXT, signer.address);
      const key = await deriveSaveKey(secretOf(kp), signer.address);
      const set = { id: "retroarch-saves", emulator: "RetroArch", label: "Save files", kind: "emulator" as const, rel: ".config/retroarch/saves", path: "", files: 3, bytes: archive.length, modified: null };
      const steps: string[] = [];
      const made = await backUpSet({ network: "testnet", signer, key, set, days: 2, pack: async () => archive, onStep: (s) => steps.push(s.step) });
      expect(steps).toEqual(["encode", "register", "upload", "certify", "done"]);

      // A new device: same account, fresh key derivation, nothing but the chain to go on.
      const again = Ed25519Keypair.fromSecretKey(kp.getSecretKey());
      const key2 = await deriveSaveKey(secretOf(again), signer.address);
      const found = await listBackups("testnet", signer.address, key2);
      const b = found.find((x) => x.blobId === made.blobId);
      expect(b?.meta).toMatchObject({ id: "retroarch-saves", rel: ".config/retroarch/saves", bytes: archive.length });

      let restored: Uint8Array | null = null;
      for (let i = 0; i < 10 && !restored; i++) {
        restored = await fetchBackup("testnet", key2, b!).catch(() => null);
        if (!restored) await new Promise((r) => setTimeout(r, 3000));
      }
      expect(restored && Buffer.from(restored).equals(Buffer.from(archive))).toBe(true);
    },
    300_000,
  );
});
