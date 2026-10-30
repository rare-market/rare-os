// Walrus TESTNET helpers for the gated end-to-end tests (cloud saves and Share). Test code only:
// nothing in the app imports this. Keys are never printed or saved.
//
// The public testnet faucet refuses scripts, so RARE_WALRUS_E2E_KEYS can name a JSON file of
// testnet-only keys (["suiprivkey..."]); the first one pays. Without it a fresh key asks the faucet.

import { readFileSync } from "node:fs";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { Transaction } from "@mysten/sui/transactions";
import { getFaucetHost, requestSuiFromFaucetV2 } from "@mysten/sui/faucet";
import { TESTNET_WALRUS_PACKAGE_CONFIG } from "@mysten/walrus";
import { grpc } from "../chain";
import type { CloudSigner } from "../signer";

const client = grpc("testnet");

export function keypairSigner(kp: Ed25519Keypair): CloudSigner {
  return {
    address: kp.toSuiAddress(),
    signPersonalMessage: (m) => kp.signPersonalMessage(m),
    signAndExecute: async (tx) => {
      tx.setSenderIfNotSet(kp.toSuiAddress());
      const bytes = await tx.build({ client });
      const { signature } = await kp.signTransaction(bytes);
      const r = await client.core.executeTransaction({ transaction: bytes, signatures: [signature], include: { effects: true } });
      const t = r.$kind === "Transaction" ? r.Transaction : r.FailedTransaction;
      if (r.$kind !== "Transaction") throw new Error(`transaction failed: ${JSON.stringify(t.status)}`);
      await client.core.waitForTransaction({ digest: t.digest });
      return { digest: t.digest };
    },
  };
}

export function testKey(): Ed25519Keypair {
  const file = process.env.RARE_WALRUS_E2E_KEYS;
  if (!file) return new Ed25519Keypair();
  return Ed25519Keypair.fromSecretKey((JSON.parse(readFileSync(file, "utf8")) as string[])[0]);
}

/** SUI and testnet WAL held, in MIST and FROST. */
export async function balances(owner: string): Promise<{ sui: bigint; wal: bigint }> {
  const r = await client.core.listBalances({ owner, limit: 50 });
  const of = (re: RegExp) => r.balances.filter((b) => re.test(b.coinType)).reduce((a, b) => a + BigInt(b.balance), 0n);
  return { sui: of(/^0x0*2::sui::SUI$/), wal: of(/::wal::WAL$/) };
}

/** Swaps 0.5 testnet SUI for WAL at the public exchange's fixed rate. */
export async function swapForWal(kp: Ed25519Keypair, mist = 500_000_000) {
  const owner = kp.toSuiAddress();
  const exchangeId = TESTNET_WALRUS_PACKAGE_CONFIG.exchangeIds![0];
  const ex = await client.core.getObject({ objectId: exchangeId });
  const pkg = ex.object.type.split("::")[0];
  const tx = new Transaction();
  const [sui] = tx.splitCoins(tx.gas, [mist]);
  const wal = tx.moveCall({ target: `${pkg}::wal_exchange::exchange_all_for_wal`, arguments: [tx.object(exchangeId), sui] });
  tx.transferObjects([wal], owner);
  await keypairSigner(kp).signAndExecute(tx, "exchange");
}

/** Faucet SUI if low, then a fresh swap for WAL. */
export async function fundWithWal(kp: Ed25519Keypair) {
  const owner = kp.toSuiAddress();
  const suiBal = async () => (await balances(owner)).sui;
  if ((await suiBal()) < 600_000_000n) {
    await requestSuiFromFaucetV2({ host: getFaucetHost("testnet"), recipient: owner });
    for (let i = 0; i < 30 && (await suiBal()) === 0n; i++) await new Promise((r) => setTimeout(r, 1000));
  }
  await swapForWal(kp);
}
