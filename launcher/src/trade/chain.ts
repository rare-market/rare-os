// Sui reads for the Trade tab, over gRPC only (gRPC-web against the public fullnode).

import { SuiGrpcClient } from "@mysten/sui/grpc";
import { Transaction } from "@mysten/sui/transactions";
import { normalizeStructTag, normalizeSuiAddress } from "@mysten/sui/utils";
import { normalizeType } from "./amount";

export const grpc = new SuiGrpcClient({ network: "mainnet", baseUrl: "https://fullnode.mainnet.sui.io:443" });

/** The wallet's whole balance of one coin, base units. */
export async function coinBalance(owner: string, coinType: string): Promise<bigint> {
  const r = await grpc.core.getBalance({ owner, coinType: normalizeType(coinType) });
  return BigInt(r.balance.balance);
}

export interface DryRun {
  ok: boolean;
  error: string | null;
  /** coin type -> signed base units for the sender */
  changes: Array<{ coinType: string; amount: bigint }>;
  gasMist: bigint | null;
  /** the chain's own words, for logs */
  raw?: string;
}

/**
 * Builds the transaction for `sender` (gas and coins resolved over gRPC) and simulates it. Nothing
 * is signed or sent: this is the check before handing the transaction to Rare Wallet, and what
 * the tests use.
 */
export async function dryRun(tx: Transaction | Uint8Array, sender: string, client: Pick<SuiGrpcClient, "core"> = grpc): Promise<DryRun> {
  let bytes: Uint8Array;
  try {
    if (tx instanceof Uint8Array) bytes = tx;
    else {
      tx.setSenderIfNotSet(sender);
      bytes = await tx.build({ client: client as SuiGrpcClient });
    }
  } catch (e) {
    return { ok: false, error: plainChainError(e), changes: [], gasMist: null, raw: e instanceof Error ? e.message : String(e) };
  }
  const r = await client.core.simulateTransaction({ transaction: bytes, include: { effects: true, balanceChanges: true } });
  const t = r.Transaction ?? r.FailedTransaction;
  const effects = t?.effects;
  const ok = !!effects?.status.success;
  const gas = effects?.gasUsed;
  const gasMist = gas ? BigInt(gas.computationCost) + BigInt(gas.storageCost) - BigInt(gas.storageRebate) : null;
  const me = normalizeSuiAddress(sender);
  const changes = (t?.balanceChanges ?? [])
    .filter((c) => normalizeSuiAddress(c.address) === me)
    .map((c) => ({ coinType: normalizeType(c.coinType), amount: BigInt(c.amount) }));
  const failure = effects && !effects.status.success ? effects.status.error : "The transaction would fail";
  return { ok, error: ok ? null : plainChainError(failure), changes, gasMist };
}

const SUI_COIN = normalizeStructTag("0x2::coin::Coin<0x2::sui::SUI>");

type Json = { [k: string]: unknown };
interface TxJson {
  gasData: { payment: unknown[] | null };
  inputs: Array<{ Object?: { ImmOrOwnedObject?: { objectId: string; version: string; digest: string } } }>;
  commands: unknown[];
}

/** Every {Input: n} in the commands, rewritten by `f` (deep: vectors, nested arguments). */
function mapInputs(node: unknown, f: (n: number) => unknown): unknown {
  if (Array.isArray(node)) return node.map((x) => mapInputs(x, f));
  if (!node || typeof node !== "object") return node;
  const o = node as Json;
  if (typeof o.Input === "number") return f(o.Input);
  const out: Json = {};
  for (const [k, v] of Object.entries(o)) out[k] = mapInputs(v, f);
  return out;
}

/**
 * Router transactions spend one of the wallet's own SUI coins as an input. A wallet that holds a
 * single SUI coin then has nothing left to pay gas with, so the build fails. Paying gas from that
 * same coin (it becomes the gas coin, and the trade splits from it) works for every wallet.
 */
export async function gasFromSpentSui(tx: Transaction, client: Pick<SuiGrpcClient, "core"> = grpc): Promise<Transaction> {
  const data = JSON.parse(await tx.toJSON()) as TxJson;
  if (data.gasData.payment?.length) return tx;
  const owned = data.inputs.flatMap((inp, i) => (inp.Object?.ImmOrOwnedObject ? [{ i, ref: inp.Object.ImmOrOwnedObject }] : []));
  if (!owned.length) return tx;
  const res = await client.core.getObjects({ objectIds: owned.map((o) => o.ref.objectId) });
  const first = owned.find((_, k) => {
    const ob = res.objects[k];
    return !(ob instanceof Error) && !!ob && normalizeStructTag(ob.type) === SUI_COIN;
  });
  if (!first) return tx;
  const at = first.i;
  data.commands = mapInputs(data.commands, (n) => (n === at ? { GasCoin: true } : { Input: n > at ? n - 1 : n })) as unknown[];
  data.inputs.splice(at, 1);
  data.gasData.payment = [{ objectId: first.ref.objectId, version: first.ref.version, digest: first.ref.digest }];
  return Transaction.from(JSON.stringify(data));
}

/** Short, plain words for chain errors. */
export function plainChainError(e: unknown): string {
  const raw = e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e);
  if (/insufficient SUI balance|InsufficientGas|gas selection/i.test(raw)) return "You need SUI for the network fee. Add SUI from Wallet, then Receive.";
  if (/InsufficientCoinBalance|insufficient.*balance/i.test(raw)) return "Not enough balance for this trade.";
  if (/abort code: 30\b/.test(raw)) return "Not enough margin for this order. Add funds or lower the size.";
  if (/abort code: 41\b/.test(raw)) return "Your position in this market changed while the order was being built. Try again.";
  if (/MoveAbort|abort/i.test(raw)) return "The trade would fail at current prices. Get a new quote.";
  return raw.length > 160 ? `${raw.slice(0, 157)}...` : raw;
}
