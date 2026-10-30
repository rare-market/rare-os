//! On-chain ids the NFT send flow touches (`tx/nft/ids.ts`). The Drip, Tradeport and Mysten kiosk
//! extension ids live in `rare_sui::kiosk` (the reads use them too); this adds the call targets.

pub use rare_sui::kiosk::{
    DRIP_LISTINGS_BAG, DRIP_LISTING_REGISTRY, DRIP_MARKETPLACE_PACKAGE, KIOSK_EXT_PACKAGE, KIOSK_EXT_VERSIONS, KIOSK_RULES_ORIGINAL_PACKAGE, KIOSK_TYPE, OB_OWNER_TOKEN_TYPE,
    PERSONAL_KIOSK_CAP_TYPE, TRADEPORT_LISTINGS_PACKAGE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE,
};

/// SUI as the TS builders write it in type arguments (`coin::zero<0x2::sui::SUI>`).
pub const SUI_TYPE: &str = "0x2::sui::SUI";

pub fn drip_marketplace(f: &str) -> String {
    format!("{DRIP_MARKETPLACE_PACKAGE}::marketplace::{f}")
}

pub fn tradeport(f: &str) -> String {
    format!("{TRADEPORT_LISTINGS_PACKAGE}::tradeport_listings::{f}")
}

pub fn personal_kiosk(f: &str) -> String {
    format!("{KIOSK_EXT_PACKAGE}::personal_kiosk::{f}")
}
