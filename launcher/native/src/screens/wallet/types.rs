//! The wallet data the screens read, shaped like the TS types they come from (wallet/core/types.ts
//! `RareAccount`, wallet/data/balances.ts `Portfolio`, wallet/data/activity.ts `ActivityItem`,
//! wallet/native.ts `VaultStatus`). rare-wallet and rare-sui fill them; fixtures until then.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HdInfo {
    pub index: u32,
    pub phrase_id: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PasskeyInfo {
    pub credential_id: String,
    /// where the passkey lives, for copy ("iPhone", "Security key")
    pub authenticator: Option<String>,
    /// a PIN account: this SuiPlay signs with its PIN
    pub pin: bool,
    /// every passkey's authenticator ("phone", "security-key"), in committee order
    pub backups: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceInfo {
    /// the key's BIP32 path on the device
    pub path: String,
    /// Keystone master fingerprint (hex)
    pub mfp: Option<String>,
    pub device: Option<String>,
}

/// `RareAccount`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RareAccount {
    pub address: String,
    pub label: String,
    /// "native" | "passkey" | "social" | "watch" | "ledger" | "keystone"
    pub kind: String,
    pub hd: Option<HdInfo>,
    pub passkey: Option<PasskeyInfo>,
    pub ledger: Option<DeviceInfo>,
    pub keystone: Option<DeviceInfo>,
    /// The key is in the device vault: it signs with A alone, no PIN.
    pub device: bool,
}

impl RareAccount {
    pub fn new(address: &str, label: &str, kind: &str) -> RareAccount {
        RareAccount { address: address.into(), label: label.into(), kind: kind.into(), ..Default::default() }
    }

    /// rare-wallet's account (the keystore's public data).
    pub fn from_wallet(a: &rare_wallet::keystore::RareAccount) -> RareAccount {
        RareAccount {
            address: a.address.clone(),
            label: a.label.clone(),
            kind: a.kind.as_str().into(),
            hd: a.hd.as_ref().map(|h| HdInfo { index: h.index, phrase_id: h.phrase_id.clone() }),
            passkey: a.passkey.as_ref().map(|p| PasskeyInfo {
                credential_id: p.credential_id.clone(),
                authenticator: p.authenticator.clone(),
                pin: p.is_pin(),
                backups: p.passkeys().into_iter().map(|m| m.authenticator.unwrap_or_default()).collect(),
            }),
            ledger: a.ledger.as_ref().map(|l| DeviceInfo { path: l.path.clone(), mfp: None, device: l.device.clone() }),
            keystone: a.keystone.as_ref().map(|k| DeviceInfo { path: k.path.clone(), mfp: Some(k.mfp.clone()), device: k.device.clone() }),
            device: rare_wallet::keystore::in_device_vault(a),
        }
    }
}

/// `PortfolioToken` (a `Balance` with prices).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PortfolioToken {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub icon_url: Option<String>,
    pub raw: i128,
    pub amount: f64,
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
    /// percent over 24h
    pub change_24h: Option<f64>,
    pub verified: bool,
}

/// `Portfolio`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Portfolio {
    /// None when no token has a price (the Trade API down)
    pub total_usd: Option<f64>,
    pub change_24h_usd: Option<f64>,
    pub tokens: Vec<PortfolioToken>,
    /// false when prices could not be loaded
    pub priced: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ActivityKind {
    Send,
    Receive,
    Swap,
    Other,
}

/// One coin change of an activity item (`ActivityChange`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActivityChange {
    pub coin_type: String,
    /// signed base units
    pub amount: String,
    pub symbol: Option<String>,
    pub decimals: Option<u32>,
}

/// `ActivityItem`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ActivityItem {
    pub digest: String,
    /// ms since epoch
    pub timestamp: i64,
    pub kind: ActivityKind,
    pub changes: Vec<ActivityChange>,
    /// "1.5 SUI", "215.195 SUI → 502.6154 MAGMA"
    pub summary: String,
    pub counterparty: Option<String>,
    pub failed: bool,
    pub count: Option<u32>,
}

/// `VaultStatus.kind`: how the vault opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretKind {
    Pin,
    Passphrase,
}

/// rare.market's `CollectionRef`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CollectionRef {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub verified: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rarity {
    pub rank: u32,
    pub tier: String,
}

/// rare.market's `Listing` (the fields the screens show).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub venue: String,
    pub price_mist: i128,
    pub all_in_mist: Option<i128>,
}

/// A staking contract (or an object the owner holds) keeps the item: it counts as theirs but
/// cannot be listed or sent until unstaked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Staked {
    pub protocol: Option<String>,
    pub name: Option<String>,
    /// "staked" | "nested"
    pub via: String,
}

/// rare.market's `ItemCard` (wallet/data/nfts.ts `ItemCard` is the same shape).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemCard {
    pub id: String,
    pub name: String,
    pub type_: String,
    pub image_url: Option<String>,
    pub collection: Option<CollectionRef>,
    pub rarity: Option<Rarity>,
    pub listing: Option<Listing>,
    pub last_sale_mist: Option<i128>,
    pub kiosk_id: Option<String>,
    pub staked: Option<Staked>,
}

/// rare.market's trait with how many items share it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attribute {
    pub type_: String,
    pub value: String,
    pub pct: Option<f64>,
}

/// Who holds an object (`ChainOwner`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerKind {
    Address,
    Object,
    Shared,
    Immutable,
    Unknown,
}

/// wallet/data/nfts.ts `NftDetail`.
#[derive(Clone, Debug, PartialEq)]
pub struct NftDetail {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub description: String,
    pub collection: Option<CollectionRef>,
    pub attributes: Vec<Attribute>,
    pub rarity: Option<Rarity>,
    pub listing: Option<Listing>,
    pub kiosk_id: Option<String>,
    pub owner: OwnerKind,
    pub in_kiosk: bool,
    pub can_send: bool,
    /// plain reason when can_send is false
    pub cant_send: Option<String>,
}
