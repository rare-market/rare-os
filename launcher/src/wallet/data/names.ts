// SuiNS over the fullnode's gRPC NameService (LookupName / ReverseLookupName), cached briefly.

import { normalizeSuiAddress } from "@mysten/sui/utils";
import { grpcClient } from "../grpc";

export interface NameClient {
  resolveNameServiceAddress(o: { name: string }): Promise<{ address: string | null }>;
  defaultNameServiceName(o: { address: string }): Promise<{ data: { name: string | null } }>;
}

const TTL = 5 * 60_000;
const forward = new Map<string, { at: number; v: Promise<string | null> }>();
const reverse = new Map<string, { at: number; v: Promise<string | null> }>();

function cached(map: typeof forward, key: string, load: () => Promise<string | null>): Promise<string | null> {
  const hit = map.get(key);
  if (hit && Date.now() - hit.at < TTL) return hit.v;
  const v = load();
  map.set(key, { at: Date.now(), v });
  // failures are not cached
  v.catch(() => map.delete(key));
  return v;
}

const core = () => grpcClient.core as unknown as NameClient;

/** `alice.sui` -> address, or null when the name has no target. Throws when the node can't be reached. */
export function nameToAddress(name: string, client: NameClient = core()): Promise<string | null> {
  const key = name.trim().toLowerCase();
  return cached(forward, key, async () => {
    const r = await client.resolveNameServiceAddress({ name: key });
    return r.address ? normalizeSuiAddress(r.address) : null;
  });
}

/** Reverse lookup: the address's default SuiNS name, or null. */
export function nameOf(address: string, client: NameClient = core()): Promise<string | null> {
  const key = normalizeSuiAddress(address);
  return cached(reverse, key, async () => (await client.defaultNameServiceName({ address: key })).data.name ?? null);
}
