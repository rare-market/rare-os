//! Listing, bid, kiosk and policy shapes. Ported from launcher/src/marketplace/venues/model.ts
//! (rare.market packages/venues/src/model.ts and the API's Listing).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// One open listing as the builder sees it. Amounts are MIST as decimal strings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveListing {
    /// "tradeport", "drip", "kiosk" or "market" (rare's own contract).
    pub venue: String,
    /// Where the listing lives inside the venue; the builders switch on it.
    pub source: String,
    /// The venue's own key for the listing (orderKey in the API).
    pub listing_id: String,
    pub object_id: Option<String>,
    pub nft_id: String,
    pub nft_type: Option<String>,
    pub seller: Option<String>,
    pub kiosk_id: Option<String>,
    /// Price as the venue records it.
    pub price: String,
    /// Marketplace fee the buyer pays on top of `price`.
    pub fee: String,
    /// Marketplace cut taken out of `price` (legacy commission).
    pub commission: String,
    pub coin_type: String,
    /// Venue details the builder needs (listingType, inOrderbook, ...).
    pub extra: Map<String, Value>,
}

/// One open bid or offer as the builder sees it. Amounts are MIST as decimal strings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActiveBid {
    pub venue: String,
    pub source: String,
    pub bid_id: String,
    /// "collection" or "item".
    pub kind: String,
    pub nft_type: Option<String>,
    /// Item bids and offers only.
    pub nft_id: Option<String>,
    pub bidder: String,
    /// What the seller is offered before venue cuts.
    pub price: String,
    pub fee: String,
    pub royalty: String,
    pub commission: String,
    pub coin_type: String,
    pub pool_id: Option<String>,
    pub expire_at: Option<String>,
    /// False when the bid exists but cannot be filled (pool short, frozen store).
    pub fillable: bool,
    pub extra: Map<String, Value>,
}

/// GET /v1/accounts/:address/kiosks
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OwnedKioskRef {
    pub kiosk_id: String,
    /// KioskOwnerCap id. For a personal kiosk the cap is wrapped in the PersonalKioskCap.
    pub owner_cap_id: String,
    pub personal_cap_id: Option<String>,
    pub is_personal: bool,
}

/// GET /v1/policies/:type, one rule.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PolicyRule {
    #[serde(rename = "type")]
    pub kind: String,
    /// Full Move type of the rule, e.g. 0x434b...::royalty_rule::Rule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransferPolicy {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_shared: Option<bool>,
    pub rules: Vec<PolicyRule>,
}

/// Policies by NFT type, as loaded for a trade.
pub type PoliciesByType = HashMap<String, Vec<TransferPolicy>>;
