import type { RareAccount } from '../types'

type AccountLike = Pick<RareAccount, 'kind'> | null | undefined

/** True when the account can produce signatures. Screens use this to swap Send/Swap/Sign for an "Import to sign" CTA. */
export function canSign(account: AccountLike): boolean {
  return !!account && account.kind !== 'watch'
}

export const isWatch = (account: AccountLike): boolean => account?.kind === 'watch'

/** Keys live on a Ledger: every signature is confirmed on the device. */
export const isLedger = (account: AccountLike): boolean => account?.kind === 'ledger'

/** Shown instead of a dead grey button on blocked actions. */
export const IMPORT_TO_SIGN_CTA = 'Import to sign'
