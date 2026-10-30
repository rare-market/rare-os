// Which Sui network the launcher reads (Settings > Developer). Mainnet by default. The native side
// owns the same setting for the rare.market API and the gas sponsor (src-tauri/src/network.rs);
// this is the UI half: the Sui RPC the gRPC reads use. Applied from the saved config at start
// (main.tsx) and whenever Settings changes it.

import type { Config } from "./types";

export type Network = "mainnet" | "testnet";

export const DEFAULT_RPC: Record<Network, string> = {
  mainnet: "https://fullnode.mainnet.sui.io:443",
  testnet: "https://fullnode.testnet.sui.io:443",
};

let current: Network = "mainnet";
let testnetRpc: string | null = null;

/** https anywhere; plain http only on this machine (a local node under test). */
export function rpcOk(u: string): boolean {
  try {
    const p = new URL(u);
    return p.protocol === "https:" || (p.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(p.hostname));
  } catch {
    return false;
  }
}

export function applyNetworkConfig(c: Pick<Config, "network" | "testnetRpc"> | null | undefined): void {
  current = c?.network === "testnet" ? "testnet" : "mainnet";
  const r = c?.testnetRpc?.trim();
  testnetRpc = r && rpcOk(r) ? r.replace(/\/+$/, "") : null;
}

/** The network the launcher is on. */
export function currentNetwork(): Network {
  return current;
}

/** The gRPC-web endpoint for a network (the testnet override applies to testnet only). */
export function rpcUrl(network: Network): string {
  return network === "testnet" && testnetRpc ? testnetRpc : DEFAULT_RPC[network];
}
