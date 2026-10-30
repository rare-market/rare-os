// Where Rare Trophies lives on chain. deployment.json is filled by the deploy kit; while both
// networks are empty the trophy case still shows local Steam achievements and Claim stays off.

import deployment from "./deployment.json";
import type { Network } from "../sui/chain";

export interface TrophyConfig {
  network: Network;
  package: string;
  registry: string;
}

type Deployment = Record<Network, { package: string; registry: string }>;

/** Mainnet when it is deployed there, else testnet, else null (claiming not open yet). */
export function trophyConfig(d: Deployment = deployment as Deployment): TrophyConfig | null {
  for (const network of ["mainnet", "testnet"] as const) {
    const e = d[network];
    if (e?.package && e?.registry) return { network, package: e.package, registry: e.registry };
  }
  return null;
}

export const trophyType = (pkg: string) => `${pkg}::trophy::Trophy`;
