// Uploading a share: one Walrus quilt through the same client, relay and two-approval flow as
// cloud saves (sui/saves/walrus.ts), paid from the player's wallet.

import { WalrusFile } from "@mysten/walrus";
import type { Network } from "../sui/chain";
import type { CloudSigner } from "../sui/signer";
import { estimate, payAndStore, walrusClient, AGGREGATOR, type CostEstimate, type UploadProgress } from "../sui/saves/walrus";
import { estimateQuiltBytes, mediaFile, mediaMime, shareAttributes, shareLink, type ShareJson } from "./format";
import type { CaptureKind } from "./native";
import type { SponsorFile } from "./sponsor";

export type QuiltFile = SponsorFile;

/** The quilt's files, identifiers exactly as rare.market reads them. */
export function shareFiles(json: ShareJson, media: Uint8Array, poster: Uint8Array | null): QuiltFile[] {
  const files: QuiltFile[] = [
    { identifier: "share.json", mime: "application/json", bytes: new TextEncoder().encode(JSON.stringify(json)) },
    { identifier: mediaFile(json.kind), mime: mediaMime(json.kind), bytes: media },
  ];
  if (json.poster && poster) files.push({ identifier: "poster.jpg", mime: "image/jpeg", bytes: poster });
  return files;
}

/** Size of the encoded quilt (what Walrus charges for), from the SDK's encoder; estimated if that fails. */
export async function quiltSize(network: Network, files: QuiltFile[]): Promise<number> {
  try {
    const { quilt } = await walrusClient(network).walrus.encodeQuilt({ blobs: files.map((f) => ({ contents: f.bytes, identifier: f.identifier })) });
    return quilt.length;
  } catch {
    return estimateQuiltBytes(files.map((f) => f.bytes.length));
  }
}

export async function shareEstimate(network: Network, size: number, days: number): Promise<CostEstimate> {
  return estimate(size, days, network);
}

export interface ShareResult {
  quiltId: string;
  blobObjectId: string | null;
  link: string;
}

/** Stores the quilt, paid by `signer` (two approvals: "Share screenshot" and "Confirm share"). */
export async function uploadShare(
  network: Network,
  signer: CloudSigner,
  files: QuiltFile[],
  o: { kind: CaptureKind; epochs: number; onStep?: (s: UploadProgress) => void },
): Promise<ShareResult> {
  const flow = walrusClient(network).walrus.writeFilesFlow({ files: files.map((f) => WalrusFile.from({ contents: f.bytes, identifier: f.identifier })) });
  const r = await payAndStore(network, signer, flow, {
    epochs: o.epochs,
    // Permanent for its epochs: rare.market finds the player from the blob's certify event, which
    // deletable blobs don't carry.
    deletable: false,
    attributes: shareAttributes(o.kind),
    titles: { register: o.kind === "clip" ? "Share clip" : "Share screenshot", certify: "Confirm share" },
    onStep: o.onStep,
  });
  return { quiltId: r.blobId, blobObjectId: r.blobObjectId, link: shareLink(r.blobId, network) };
}

/** A file of a share, from the public aggregator. */
export function quiltFileUrl(network: Network, quiltId: string, identifier: string): string {
  return `${AGGREGATOR[network]}/v1/blobs/by-quilt-id/${encodeURIComponent(quiltId)}/${encodeURIComponent(identifier)}`;
}
