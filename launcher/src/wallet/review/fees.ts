// Fee recipients Rare itself uses. Transfers to them are shown as a named fee line ("Rare fee
// 0.2%") instead of "Sends coins to 0x..., not you" plus a first-time-recipient warning, which
// reads like a scam alert on every swap. Unknown recipients keep the full warning.
//
// - the swap router's fee wallet: the 0.2% swap fee on Rare's Trade tab (the wallet the Trade
//   API's swap builds pay).
// rare.market's own marketplace fees go to its Config treasury through the contract, not as a
// plain coin transfer, so there is nothing to list for it until that changes.

import { normalizeSuiAddress } from '@mysten/sui/utils'
import { normType } from './analyze'
import type { Insights } from './analyze'

export interface KnownFeeRecipient {
  label: string
}

export const KNOWN_FEE_RECIPIENTS: Record<string, KnownFeeRecipient> = {
  [normalizeSuiAddress('0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06')]: { label: 'Rare fee' },
}

export const knownFee = (address: string): KnownFeeRecipient | null => KNOWN_FEE_RECIPIENTS[normalizeSuiAddress(address)] ?? null

export interface FeeLine {
  address: string
  label: string
  coinType: string
  amount: bigint
  /** the fee as a share of what you send of that coin, when known, e.g. "0.2%" */
  pct: string | null
}

/** Splits coin transfers to other addresses into Rare's own fee lines and everything else. */
export function splitFees(ins: Pick<Insights, 'othersReceive' | 'send'>): { fees: FeeLine[]; others: Insights['othersReceive'] } {
  const fees: FeeLine[] = []
  const others: Insights['othersReceive'] = []
  for (const o of ins.othersReceive) {
    const k = knownFee(o.address)
    if (!k) {
      others.push(o)
      continue
    }
    const sent = ins.send.find((s) => normType(s.coinType) === normType(o.coinType))?.amount ?? 0n
    // the fee is part of what leaves the wallet: share = fee / (amount sent)
    let pct: string | null = null
    if (sent > 0n) {
      const bps = Number((o.amount * 10_000n + sent / 2n) / sent) // basis points, rounded
      pct = `${(bps / 100).toFixed(bps % 10 === 0 ? 1 : 2).replace(/\.0$/, '')}%`
    }
    fees.push({ address: o.address, label: k.label, coinType: o.coinType, amount: o.amount, pct })
  }
  return { fees, others }
}
