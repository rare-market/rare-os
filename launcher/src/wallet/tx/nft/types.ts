/* Data shapes for sending NFTs. */

/** A kiosk the sender owns. */
export interface KioskRef {
  kioskId: string
  /** KioskOwnerCap id. For personal kiosks the cap is wrapped inside the PersonalKioskCap. */
  ownerCapId: string
  /** PersonalKioskCap id when the kiosk is personal. */
  personalCapId?: string | null
  isPersonal: boolean
}

export interface KioskItemRef {
  nftId: string
  nftType: string
  isLocked: boolean
  isListed?: boolean
  isExclusiveListing?: boolean
  /** Where it is listed, when a marketplace has to cancel the listing (not a plain kiosk::list). */
  market?: Market
}

/** Drip: kiosk listing + registry record. Tradeport: exclusive listing, cap held by Tradeport (orderbook or not). */
export type Market = { kind: 'drip' } | { kind: 'tradeport'; orderbook: boolean }

export interface PolicyRule {
  /** registry id ("royalty_rule") or the rule's struct name */
  type: string
  /** Full Move type of the rule, e.g. 0x434b...::royalty_rule::Rule */
  ruleType?: string
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  config?: any
}

export interface TransferPolicyInfo {
  id: string
  isShared?: boolean
  rules: PolicyRule[]
}

export interface SendItem {
  /** Kiosk the item is in, or null for an NFT held directly in the wallet. */
  from: KioskRef | null
  item: KioskItemRef
  policies?: TransferPolicyInfo[] | null
}
