// One finished transaction over gRPC getTransaction, reduced to what the activity detail screen
// shows from this wallet's point of view.

import type { SuiClientTypes } from "@mysten/sui/client";
import { normalizeSuiAddress } from "@mysten/sui/utils";
import { grpcClient, SUI_TYPE } from "../grpc";
import { isCoinObject } from "../review/packages";

export interface TxDetail {
  digest: string;
  status: "success" | "failure";
  error?: string;
  timestampMs: number | null;
  sender: string;
  /** signed base units for THIS wallet, network fee already taken out of SUI */
  flows: { coinType: string; amount: string }[];
  /** net SUI the gas cost (MIST); negative = refund */
  feeMist: string;
  /** false when someone else (a sponsor) paid */
  feePaid: boolean;
  /** the other side of a transfer, when it is one address */
  counterparty?: string;
  calls: { package: string; module: string; function: string }[];
  objects: { id: string; type: string | null; action: "created" | "received" | "sent" | "deleted"; to?: string }[];
}

type Include = { effects: true; balanceChanges: true; events: true; objectTypes: true; transaction: true };

/** The slice of the gRPC client the loader uses (stubbed in tests). */
export interface TxClient {
  getTransaction(o: { digest: string; include: Include }): Promise<SuiClientTypes.TransactionResult<Include>>;
}

const norm = (a: string | null | undefined) => {
  if (!a) return "";
  try {
    return normalizeSuiAddress(a);
  } catch {
    return a.toLowerCase();
  }
};

const ownerAddr = (o: SuiClientTypes.ObjectOwner | null | undefined): string | null =>
  !o ? null : o.$kind === "AddressOwner" ? o.AddressOwner : o.$kind === "ConsensusAddressOwner" ? o.ConsensusAddressOwner.owner : null;

/** True when the node no longer has (or never had) the transaction. */
export function isPruned(e: unknown): boolean {
  const m = String((e as { message?: string })?.message ?? e) + String((e as { code?: string })?.code ?? "");
  return /not.?found|pruned|does not exist|NOT_FOUND|404/i.test(m);
}

/** Load one past transaction for wallet `me`. Null when the node has pruned it. */
export async function activityDetail(digest: string, me: string, client: TxClient = grpcClient.core as unknown as TxClient): Promise<TxDetail | null> {
  let r;
  try {
    r = await client.getTransaction({ digest, include: { effects: true, balanceChanges: true, events: true, objectTypes: true, transaction: true } });
  } catch (e) {
    if (isPruned(e)) return null;
    throw e;
  }
  const t = r.Transaction ?? r.FailedTransaction;
  if (!t) return null;
  return parseTx(t, digest, me);
}

type Tx = SuiClientTypes.Transaction<Include>;

/** Pure: SDK transaction -> TxDetail for wallet `me` (exported for tests). */
export function parseTx(t: Tx, digest: string, me: string): TxDetail {
  const mine = norm(me);
  const sender = t.transaction?.sender ?? "";
  const g = t.effects?.gasUsed;
  const net = g ? BigInt(g.computationCost) + BigInt(g.storageCost) - BigInt(g.storageRebate) : 0n;
  const gasOwner = ownerAddr(t.effects?.gasObject?.inputOwner) ?? sender;
  const feePaid = norm(gasOwner) === mine;

  // balance changes for this wallet, merged per coin; the fee is shown on its own line
  const sums = new Map<string, bigint>();
  for (const b of t.balanceChanges ?? []) {
    if (norm(b.address) !== mine) continue;
    sums.set(b.coinType, (sums.get(b.coinType) ?? 0n) + BigInt(b.amount));
  }
  if (feePaid) {
    const key = [...sums.keys()].find((k) => k.endsWith("::sui::SUI") && /^0x0*2::/.test(k)) ?? SUI_TYPE;
    sums.set(key, (sums.get(key) ?? 0n) + net);
  }
  const flows = [...sums].filter(([, v]) => v !== 0n).map(([coinType, v]) => ({ coinType, amount: v.toString() }));

  // the other address: whoever got what we sent, or whoever sent to us
  let counterparty: string | undefined;
  const sentAny = flows.some((f) => f.amount.startsWith("-"));
  if (sentAny) {
    counterparty = (t.balanceChanges ?? []).find((b) => norm(b.address) !== mine && !b.amount.startsWith("-") && b.amount !== "0")?.address;
  } else if (sender && norm(sender) !== mine) counterparty = sender;

  const calls: TxDetail["calls"] = [];
  for (const c of t.transaction?.commands ?? []) {
    const m = (c as { MoveCall?: { package: string; module: string; function: string } }).MoveCall;
    if (m) calls.push({ package: m.package, module: m.module, function: m.function });
  }

  const objects: TxDetail["objects"] = [];
  const gasId = t.effects?.gasObject?.objectId;
  for (const o of t.effects?.changedObjects ?? []) {
    if (o.objectId === gasId || o.outputState === "PackageWrite" || o.outputState === "AccumulatorWriteV1") continue;
    const type = t.objectTypes?.[o.objectId] ?? null;
    if (isCoinObject(type)) continue;
    const from = ownerAddr(o.inputOwner);
    const to = ownerAddr(o.outputOwner);
    const wasMine = norm(from) === mine;
    const isMine = norm(to) === mine;
    const gone = o.outputState === "DoesNotExist";
    if (o.idOperation === "Deleted" || (gone && wasMine)) {
      if (wasMine) objects.push({ id: o.objectId, type, action: "deleted" });
    } else if (o.idOperation === "Created" && isMine) objects.push({ id: o.objectId, type, action: "created" });
    else if (wasMine && !isMine && to) objects.push({ id: o.objectId, type, action: "sent", to });
    else if (!wasMine && isMine) objects.push({ id: o.objectId, type, action: "received" });
  }

  return {
    digest,
    status: t.status.success ? "success" : "failure",
    ...(t.status.success ? {} : { error: t.status.error?.message ?? "Transaction failed" }),
    timestampMs: t.timestampMs,
    sender,
    flows,
    feeMist: net.toString(),
    feePaid,
    ...(counterparty ? { counterparty } : {}),
    calls,
    objects,
  };
}
