// Walrus storage for Rare cloud saves, through the official @mysten/walrus SDK and Mysten's upload
// relay (one HTTPS upload from the device instead of a request per storage node). The player's
// wallet pays: WAL for storage and SUI for gas and the relay's small tip.
//
// The backup index lives on chain, not on a Rare server: every backup is a Walrus Blob object owned
// by the player, tagged with attributes { app: "rare-saves", v, key, meta } where `meta` is the
// encrypted description of the save set. A new device lists the player's Blob objects over gRPC,
// keeps the Rare ones, and downloads by blob id.

import { blobIdFromInt, walrus } from "@mysten/walrus";
import type { Transaction } from "@mysten/sui/transactions";
import walrusWasm from "@mysten/walrus-wasm/web/walrus_wasm_bg.wasm?url";
import { grpc, type Network } from "../chain";
import type { CloudSigner } from "../signer";

export const RELAY: Record<Network, string> = {
  mainnet: "https://upload-relay.mainnet.walrus.space",
  testnet: "https://upload-relay.testnet.walrus.space",
};

export const AGGREGATOR: Record<Network, string> = {
  mainnet: "https://aggregator.walrus-mainnet.walrus.space",
  testnet: "https://aggregator.walrus-testnet.walrus.space",
};

import { EPOCH_DAYS } from "./network";
export { EPOCH_DAYS, MAX_EPOCHS, epochsFor } from "./network";

export const APP_TAG = "rare-saves";

export { DEFAULT_NETWORK } from "./network";

// The wasm encoder: in the webview it loads from the bundled file; in Node the SDK uses its own build.
const wasmUrl = typeof window !== "undefined" ? walrusWasm : undefined;

function make(network: Network) {
  return grpc(network).$extend(
    walrus({
      wasmUrl,
      // The relay asks for a tip in MIST (mainnet: about 40 MIST per KiB of encoded data). Cap it at 0.5 SUI.
      uploadRelay: { host: RELAY[network], sendTip: { max: 500_000_000 } },
      storageNodeClientOptions: { timeout: 60_000 },
    }),
  );
}
type WalrusSui = ReturnType<typeof make>;
const clients = new Map<Network, WalrusSui>();
export function walrusClient(network: Network): WalrusSui {
  let c = clients.get(network);
  if (!c) {
    c = make(network);
    clients.set(network, c);
  }
  return c;
}

export interface CostEstimate {
  epochs: number;
  days: number;
  /** storage plus write fee, in FROST (1 WAL = 1e9 FROST) */
  walFrost: bigint;
  /** upload relay tip, in MIST */
  tipMist: bigint;
  /** two transactions (register and certify), rough, in MIST */
  gasMist: bigint;
}

import { epochsFor } from "./network";

/** What storing `size` bytes for `days` costs. Reads Walrus prices from chain; spends nothing. */
export async function estimate(size: number, days: number, network: Network): Promise<CostEstimate> {
  const c = walrusClient(network).walrus;
  const epochs = epochsFor(days, network);
  const [cost, tip] = await Promise.all([c.storageCost(size, epochs), c.calculateUploadRelayTip({ size }).catch(() => 0n)]);
  return { epochs, days: epochs * EPOCH_DAYS[network], walFrost: cost.totalCost, tipMist: BigInt(tip), gasMist: 12_000_000n };
}

export interface UploadProgress {
  step: "encode" | "register" | "upload" | "certify" | "done";
}

/** The steps every SDK write flow has: writeBlobFlow (one blob) and writeFilesFlow (a quilt). */
interface PayFlow {
  encode: () => Promise<{ blobId: string }>;
  register: (o: { epochs: number; owner: string; deletable: boolean; attributes?: Record<string, string | null> }) => Transaction;
  upload: (o?: { digest?: string }) => Promise<{ blobId: string; blobObjectId: string }>;
  certify: () => Transaction;
}

/**
 * Runs a write flow paid by the player: encode, register (wallet approval one: pays WAL, tags the
 * blob), upload through the relay, certify (wallet approval two). Shared by cloud saves and Share.
 */
export async function payAndStore(
  network: Network,
  signer: CloudSigner,
  flow: PayFlow,
  opts: {
    epochs: number;
    attributes: Record<string, string>;
    titles: { register: string; certify: string };
    onStep?: (s: UploadProgress) => void;
    tagBlobId?: boolean;
    /** Cloud saves: true (old backups can be deleted for a rebate). Shares: false, so the blob is permanent for its paid epochs. */
    deletable?: boolean;
  },
): Promise<{ blobId: string; blobObjectId: string }> {
  opts.onStep?.({ step: "encode" });
  const { blobId } = await flow.encode();
  opts.onStep?.({ step: "register" });
  const reg = flow.register({ epochs: opts.epochs, owner: signer.address, deletable: opts.deletable ?? true, attributes: opts.tagBlobId ? { ...opts.attributes, blob: blobId } : opts.attributes });
  reg.setSenderIfNotSet(signer.address);
  const { digest } = await signer.signAndExecute(reg, opts.titles.register);
  await grpc(network).core.waitForTransaction({ digest });
  opts.onStep?.({ step: "upload" });
  const up = await flow.upload({ digest });
  opts.onStep?.({ step: "certify" });
  const cert = flow.certify();
  cert.setSenderIfNotSet(signer.address);
  const done = await signer.signAndExecute(cert, opts.titles.certify);
  await grpc(network).core.waitForTransaction({ digest: done.digest });
  opts.onStep?.({ step: "done" });
  return { blobId, blobObjectId: up.blobObjectId };
}

/**
 * Stores an already encrypted blob. Two wallet approvals: register (pays WAL, tags the blob) and
 * certify. Returns the blob id and the Blob object id.
 */
export async function storeBlob(
  network: Network,
  signer: CloudSigner,
  blob: Uint8Array,
  opts: { epochs: number; attributes: Record<string, string>; title: string; onStep?: (s: UploadProgress) => void },
): Promise<{ blobId: string; blobObjectId: string }> {
  const flow = walrusClient(network).walrus.writeBlobFlow({ blob });
  return payAndStore(network, signer, flow, {
    epochs: opts.epochs,
    attributes: opts.attributes,
    tagBlobId: true,
    titles: { register: `Back up ${opts.title}`, certify: `Confirm backup of ${opts.title}` },
    onStep: opts.onStep,
  });
}

export interface StoredBlob {
  blobObjectId: string;
  blobId: string;
  size: number;
  endEpoch: number | null;
  certified: boolean;
  attributes: Record<string, string>;
}

interface BlobJson {
  blob_id?: string;
  size?: string | number;
  certified_epoch?: unknown;
  storage?: { end_epoch?: number | string };
}

/** The player's Walrus Blob objects tagged as Rare cloud saves. Read only. */
export async function listTagged(network: Network, owner: string): Promise<StoredBlob[]> {
  const c = walrusClient(network);
  const type = await c.walrus.getBlobType();
  const objs: Array<{ objectId: string; json: BlobJson | null }> = [];
  let cursor: string | null = null;
  do {
    const r: { objects: Array<{ objectId: string; json?: unknown }>; hasNextPage: boolean; cursor: string | null } = await c.core.listOwnedObjects({ owner, type, limit: 50, cursor, include: { json: true } });
    for (const o of r.objects) objs.push({ objectId: o.objectId, json: (o.json as BlobJson | null) ?? null });
    cursor = r.hasNextPage ? r.cursor : null;
  } while (cursor && objs.length < 500);
  const out = await Promise.all(
    objs.map(async (o) => {
      const attributes = await c.walrus.readBlobAttributes({ blobObjectId: o.objectId }).catch(() => null);
      if (!attributes || attributes.app !== APP_TAG) return null;
      const certified = o.json?.certified_epoch != null && !(typeof o.json.certified_epoch === "object" && Object.keys(o.json.certified_epoch as object).length === 0);
      const blobId = attributes.blob || (o.json?.blob_id ? blobIdFromInt(BigInt(o.json.blob_id)) : "");
      return {
        blobObjectId: o.objectId,
        blobId,
        size: Number(o.json?.size ?? 0),
        endEpoch: o.json?.storage?.end_epoch != null ? Number(o.json.storage.end_epoch) : null,
        certified,
        attributes,
      } satisfies StoredBlob;
    }),
  );
  return out.filter((x): x is StoredBlob => !!x && !!x.blobId);
}

/** Downloads a blob: the public aggregator first (one request), the storage nodes directly if that fails. */
export async function readBlob(network: Network, blobId: string): Promise<Uint8Array> {
  try {
    const r = await fetch(`${AGGREGATOR[network]}/v1/blobs/${encodeURIComponent(blobId)}`);
    if (r.ok) return new Uint8Array(await r.arrayBuffer());
  } catch {
    /* fall through to the nodes */
  }
  return walrusClient(network).walrus.readBlob({ blobId });
}

/** The current Walrus epoch, for "kept until" dates. */
export async function currentEpoch(network: Network): Promise<number> {
  const s = await walrusClient(network).walrus.systemState();
  return Number(s.committee.epoch);
}

/** A transaction that deletes an old backup and returns its storage rebate (player-initiated only). */
export function deleteTx(network: Network, owner: string, blobObjectId: string): Transaction {
  const tx = walrusClient(network).walrus.deleteBlobTransaction({ blobObjectId, owner });
  tx.setSenderIfNotSet(owner);
  return tx;
}
