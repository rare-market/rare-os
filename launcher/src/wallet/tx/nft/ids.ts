/* On-chain ids the NFT send flow touches: the live Drip and Tradeport listing contracts (read
 * from mainnet, drip.market/docs/contracts) and the Mysten kiosk extension packages. */

/**
 * Drip marketplace (drip.market/docs/contracts). A Drip listing is a plain kiosk listing
 * (kiosk::list, not exclusive) plus a ListingRecord in the registry's `listings` bag, keyed by
 * the NFT id. marketplace::delist<T>(registry, kiosk, cap, id) clears both.
 */
export const DRIP_MARKETPLACE_PACKAGE = '0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a'
export const DRIP_LISTING_REGISTRY = '0x6c2f62f0ca9b2d2ae09b6fc53defff6e35a66b8c5412d00b8d3ff435a5ab7693'
/** ListingRegistry.listings (a Bag): dynamic fields ID -> ListingRecord */
export const DRIP_LISTINGS_BAG = '0x6b3e13b4fc2fe1cdea8dfb6458f7b38f66323f4568dc54c7648b4f5e32cf6408'

/**
 * Tradeport kiosk listings: an exclusive kiosk listing whose PurchaseCap sits in a SimpleListing,
 * a dynamic object field of the shared Store keyed by the NFT id. Orderbook listings also have an
 * entry in the orderbook Store keyed by the SimpleListing id and are cancelled with the _ob_ call.
 * Calls go to the latest package version (v5); the Store checks its version.
 */
export const TRADEPORT_LISTINGS_PACKAGE = '0x81e16ff388eacc2908f4e8248f54d140eb6d7834ff01dc7a86042a3b701b68f6'
export const TRADEPORT_STORE = '0xf96f9363ac5a64c058bf7140723226804d74c0dab2dd27516fb441a180cd763b'
export const TRADEPORT_ORDERBOOK_STORE = '0x3af0a94360253c80fbabe73f6832be0a49ddfc0b8772ccd5335f568cbc356da1'

/**
 * Mysten kiosk extensions (royalty_rule, kiosk_lock_rule, personal_kiosk, personal_kiosk_rule,
 * floor_price_rule). One upgradeable package; a PTB may not mix its versions, so every call
 * goes to v4 (which also has personal_kiosk::create_for).
 */
export const KIOSK_RULES_ORIGINAL_PACKAGE = '0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879'
export const KIOSK_EXT_PACKAGE = '0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3'
export const KIOSK_EXT_VERSIONS = [
  KIOSK_RULES_ORIGINAL_PACKAGE,
  '0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802',
  '0x34cc6762780f4f6f153c924c0680cfe2a1fb4601e7d33cc28a92297b62de1e0e',
  KIOSK_EXT_PACKAGE,
]

/** PersonalKioskCap as owned-object queries report it (type origin is the v2 package). */
export const PERSONAL_KIOSK_CAP_TYPE = '0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802::personal_kiosk::PersonalKioskCap'
/** OriginByte kiosks: the wallet holds an OwnerToken that names the kiosk. */
export const OB_OWNER_TOKEN_TYPE = '0x95a441d389b07437d00dd07e0b6f05f513d7659b13fd7c5d3923c7d9d847199b::ob_kiosk::OwnerToken'

export const SUI_TYPE = '0x2::sui::SUI'
export const KIOSK_TYPE = '0x2::kiosk::Kiosk'
export const KIOSK_OWNER_CAP_TYPE = '0x2::kiosk::KioskOwnerCap'

export const target = {
  dripMarketplace: (fn: string) => `${DRIP_MARKETPLACE_PACKAGE}::marketplace::${fn}`,
  tradeport: (fn: string) => `${TRADEPORT_LISTINGS_PACKAGE}::tradeport_listings::${fn}`,
  personalKiosk: (fn: string) => `${KIOSK_EXT_PACKAGE}::personal_kiosk::${fn}`,
}
