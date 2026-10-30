import type { Network } from "../chain";

/** Dev and tests: ?walrus=testnet points cloud saves and Share at Walrus testnet. */
export const DEFAULT_NETWORK: Network = (() => {
  try {
    return new URLSearchParams(location.search).get("walrus") === "testnet" ? "testnet" : "mainnet";
  } catch {
    return "mainnet";
  }
})();

/** Walrus epoch length per network, in days. */
export const EPOCH_DAYS: Record<Network, number> = { mainnet: 14, testnet: 1 };
/** The furthest ahead storage can be bought, in epochs. */
export const MAX_EPOCHS = 53;

export function epochsFor(days: number, network: Network): number {
  return Math.max(1, Math.min(MAX_EPOCHS, Math.ceil(days / EPOCH_DAYS[network])));
}
