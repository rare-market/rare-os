// Ported from rare.market: market/packages/venues/src/ids.ts (only the ids that
// buying a listing or selling into a bid touches). Calls go to each venue's latest package: versioned stores abort on
// older code. Keep in sync with the market repo when a venue upgrades.

export const CLOCK = "0x6";

/** Mysten kiosk-extension rules (royalty, kiosk lock, personal kiosk, floor price). Rule types name
 * the version that first defined them (origin); calls go to the latest version. */
export const KIOSK_EXT = {
  origin: "0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879",
  latest: "0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3",
  /** personal_kiosk and personal_kiosk_rule were added in v2. */
  personalOrigin: "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802",
  /** floor_price_rule was added in v3. */
  floorOrigin: "0x34cc6762780f4f6f153c924c0680cfe2a1fb4601e7d33cc28a92297b62de1e0e",
};
export const KIOSK_EXT_ALL = [KIOSK_EXT.origin, KIOSK_EXT.personalOrigin, KIOSK_EXT.floorOrigin, KIOSK_EXT.latest];

/** Tradeport, by family. */
export const TRADEPORT = {
  /** A. Legacy escrow listings and wallet-NFT bids. */
  escrow: {
    latest: "0xd85a131b82ef9f4aa5eb99a0ea758a3f209a8771eba7e9402e8181273734bf9a",
    listingsStore: "0x47cba0b6309a12ce39f9306e28b899ed4b3698bce4f4911fd0c58ff2329a2ff6",
    biddingsStore: "0x3d3b0c6e616fdc1a45b2b73d022bc085448e78bd729d28081c7a340abb33cba1",
  },
  /** B. Legacy kiosk listings and kiosk bids. */
  kiosk: {
    latest: "0xafc035fc8e321da96a2cb1bb88cec64ef6bf7c83483653c259184312b3c0bb96",
    listingsStore: "0xbff3161b047fb8b727883838c09b08afa9e0dd0f5488bde9e99e80643da9b9e0",
    biddingsStore: "0x1e5e7f38e3a61d1d189506d0fc6b7e47e935a292d9e1b23c0f3f1c0f94227772",
    escrowKiosk: "0x151fbe627d3f8de855f424c879ea2176d89282185a4b9226a7157172dd2919cc",
  },
  /** C. Early kiosk listings (2023). */
  kioskEarly: {
    latest: "0xf938f27ce9460838643ac04fac05254f73329caf59643b7843c883bb4fa7402f",
    listingsStore: "0x3a8e72ea422473a0f1ef06ab3b7f727eacf048308d4dbdfa669f87f87c487b28",
  },
  /** D. `tradeport_biddings`: single bids and bid pools. */
  bids: {
    latest: "0xe2680c552bd3a5540c2f2986e92927ef2ebb6308e41ae395a1cec24e51d2d951",
    store: "0x8aaed7e884343fb8b222c721d02eaac2c6ae2abbb4ddcdf16cb55cf8754ee860",
  },
  /** E. `tradeport_listings`: current listings. */
  simple: {
    latest: "0x81e16ff388eacc2908f4e8248f54d140eb6d7834ff01dc7a86042a3b701b68f6",
    store: "0xf96f9363ac5a64c058bf7140723226804d74c0dab2dd27516fb441a180cd763b",
  },
  /** F. Orderbook index for E (buy_ob_* needs membership). */
  orderbook: {
    store: "0x3af0a94360253c80fbabe73f6832be0a49ddfc0b8772ccd5335f568cbc356da1",
  },
} as const;

/** Drip v2: records over native kiosk listings, and bids. */
export const DRIP_V2 = {
  market: "0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a",
  registry: "0x6c2f62f0ca9b2d2ae09b6fc53defff6e35a66b8c5412d00b8d3ff435a5ab7693",
  bids: "0x7dcbc405496f4f6ecb70921b571ea9bfca09db7004efddd3dad33a524e7489e4",
} as const;
