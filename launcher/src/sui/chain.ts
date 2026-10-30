// Sui chain reads for the Sui-native features (profile, ownership, live strip, cloud saves).
// Everything goes over gRPC-web against the public fullnodes, which allow CORS. Zero GraphQL.

import { SuiGrpcClient } from "@mysten/sui/grpc";
import { normalizeStructTag, normalizeSuiAddress } from "@mysten/sui/utils";

import { currentNetwork, rpcUrl, type Network } from "../lib/network";

export type { Network };

// One client per endpoint, so the Developer setting's network switch and testnet RPC override apply
// at once. The default network is the launcher's current one (Settings > Developer).
const clients = new Map<string, SuiGrpcClient>();
export function grpc(network: Network = currentNetwork()): SuiGrpcClient {
  const baseUrl = rpcUrl(network);
  const key = `${network}|${baseUrl}`;
  let c = clients.get(key);
  if (!c) {
    c = new SuiGrpcClient({ network, baseUrl });
    clients.set(key, c);
  }
  return c;
}

export const SUI_TYPE = "0x2::sui::SUI";

/** The address's default SuiNS name, or null. */
export async function defaultName(address: string): Promise<string | null> {
  const r = await grpc().core.defaultNameServiceName({ address });
  return r.data.name ?? null;
}

/** Reference gas price in MIST. */
export async function gasPrice(network: Network = currentNetwork()): Promise<number> {
  const r = await grpc(network).core.getReferenceGasPrice();
  return Number(r.referenceGasPrice);
}

/** Total SUI held, in MIST. */
export async function suiBalance(address: string, network: Network = currentNetwork()): Promise<bigint> {
  const r = await grpc(network).core.getBalance({ owner: address, coinType: SUI_TYPE });
  return BigInt(r.balance.balance);
}

export interface OwnedObject {
  objectId: string;
  type: string;
  display: Record<string, unknown> | null;
}

/** Objects of one Move type owned directly by the address (not inside kiosks), up to `max`. */
export async function ownedOfType(address: string, type: string, max = 50, network: Network = currentNetwork()): Promise<OwnedObject[]> {
  const out: OwnedObject[] = [];
  let cursor: string | null = null;
  do {
    const r: { objects: Array<{ objectId: string; type: string; display?: unknown }>; hasNextPage: boolean; cursor: string | null } = await grpc(network).core.listOwnedObjects({
      owner: address,
      type,
      limit: Math.min(50, max - out.length),
      cursor,
      include: { display: true },
    });
    for (const o of r.objects) {
      const d = (o.display ?? null) as { output?: Record<string, unknown> } | Record<string, unknown> | null;
      out.push({ objectId: o.objectId, type: o.type, display: d && "output" in d && d.output ? (d.output as Record<string, unknown>) : (d as Record<string, unknown> | null) });
    }
    cursor = r.hasNextPage ? r.cursor : null;
  } while (cursor && out.length < max);
  return out;
}

/** Canonical form of a Move type tag so types from different sources compare equal. */
export function canonType(t: string): string {
  try {
    return normalizeStructTag(t);
  } catch {
    return t.trim();
  }
}

export const canonAddr = (a: string) => normalizeSuiAddress(a);
