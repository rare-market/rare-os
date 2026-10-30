// Rare covers the fees of Playtron rescue moves (rare.market's "rescue" sponsor category) when the
// old wallet holds too little SUI for them. Each batch is built with gas "sponsored" (no gas coin:
// SUI is sent like any other coin), rare.market returns the full transaction with its own wallet
// as gas owner, the Playtron wallet signs those exact bytes, and rare.market submits.
//
// The signing still has to happen on Playtron's own surface: Rare never holds Playtron
// credentials. `sign` is that surface. None can take raw transaction bytes from Rare today, so
// MoveAssets only offers this when one is passed in.

import { defaultSponsorDeps, offers, runSponsored, sponsorStatus, sponsorTransaction, type SponsorDeps, type SponsoredOutcome, type SponsorResult } from "../../wallet/sponsor";
import { buildBatch } from "./plan";
import type { MovePlan, RescueScan } from "./types";

/** Whether rare.market covers rescue fees for this Playtron address right now. */
export async function rescueCovered(address: string, d: SponsorDeps = defaultSponsorDeps): Promise<boolean> {
  return offers(await sponsorStatus(address, d), "rescue");
}

/** The sponsored transaction for batch `index` of a plan made with gas "sponsored". */
export function sponsorRescueBatch(scan: RescueScan, plan: MovePlan, index: number, d: SponsorDeps = defaultSponsorDeps): Promise<SponsorResult> {
  return sponsorTransaction(buildBatch(scan, plan, index, "sponsored"), scan.address, "rescue", d);
}

/** Sponsor, sign on the Playtron surface, submit through rare.market. */
export function moveBatchCovered(
  scan: RescueScan,
  plan: MovePlan,
  index: number,
  sign: (bytes: Uint8Array) => Promise<string>,
  d: SponsorDeps = defaultSponsorDeps,
): Promise<SponsoredOutcome> {
  return runSponsored({ tx: buildBatch(scan, plan, index, "sponsored"), sender: scan.address, category: "rescue", sign }, d);
}
