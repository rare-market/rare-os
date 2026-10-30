// The one Sui client for Rare Wallet: gRPC-web against the public mainnet fullnode. Kept apart
// from index.ts so data/review/tx modules (and their tests) can import it without the keystore.

import { SuiGrpcClient } from "@mysten/sui/grpc";
import { currentNetwork, rpcUrl } from "../lib/network";

// The launcher's current network (Settings > Developer): mainnet unless switched to testnet. Reads
// the setting at each use, so a switch applies without every importer re-creating a client.
const made = new Map<string, SuiGrpcClient>();
function active(): SuiGrpcClient {
  const network = currentNetwork();
  const baseUrl = rpcUrl(network);
  const key = `${network}|${baseUrl}`;
  let c = made.get(key);
  if (!c) {
    c = new SuiGrpcClient({ network, baseUrl });
    made.set(key, c);
  }
  return c;
}

export const grpcClient: SuiGrpcClient = new Proxy({} as SuiGrpcClient, {
  get(_t, key) {
    const c = active();
    const v = Reflect.get(c, key, c);
    return typeof v === "function" ? v.bind(c) : v;
  },
});

export const SUI_TYPE = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
