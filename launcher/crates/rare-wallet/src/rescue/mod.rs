//! Moving assets out of an old Playtron (GameOS) wallet: a read-only scan of the old address and
//! the plan to move it, in signing order. Port of src/onboarding/rescue (types.ts, scan.ts,
//! plan.ts). Rare never signs for the old wallet: the plan is carried out in that wallet.

pub mod plan;
pub mod scan;

pub use plan::{format_amount, plan_moves};
pub use scan::{scan_playtron, stuck_reason, SUI_TYPE};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueCoin {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub icon_url: Option<String>,
    /// total in base units: coin objects plus address balance
    pub raw: u128,
    pub coin_raw: u128,
    pub address_raw: u128,
    /// Coin<T> objects, largest first
    pub object_ids: Vec<String>,
    /// balance of each entry of `object_ids`, same order
    pub object_balances: Vec<u128>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueItem {
    pub object_id: String,
    pub type_: String,
    pub name: String,
    pub image: Option<String>,
    pub collection: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueKioskItem {
    pub item: RescueItem,
    /// listed for sale, price in MIST
    pub listed: Option<u128>,
    /// locked by the collection's transfer policy
    pub locked: bool,
    /// the object type has `store`
    pub transferable: bool,
    /// listed through a marketplace's purchase cap: only that marketplace can cancel it
    pub exclusive: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueKiosk {
    pub kiosk_id: String,
    pub cap_id: String,
    pub personal: bool,
    pub personal_package: Option<String>,
    pub items: Vec<RescueKioskItem>,
    /// sale proceeds held in the kiosk, MIST
    pub profits: u128,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueStake {
    pub object_id: String,
    pub principal: u128,
    pub pool_id: String,
    pub activation_epoch: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueName {
    pub object_id: String,
    pub domain: String,
    pub expires_ms: u64,
    pub subname: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RescueScan {
    pub address: String,
    pub default_name: Option<String>,
    pub coins: Vec<RescueCoin>,
    pub nfts: Vec<RescueItem>,
    pub kiosks: Vec<RescueKiosk>,
    pub staked: Vec<RescueStake>,
    pub names: Vec<RescueName>,
    /// `key` without `store`: only their own package can move them
    pub soulbound: Vec<RescueItem>,
    /// owned objects the scan didn't page through (more than the cap)
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GasMode {
    Own,
    Sponsored,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveLine {
    /// kiosk, stake, name, nft or coin
    pub kind: &'static str,
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveBatch {
    /// 1-based, in the order to sign
    pub index: u32,
    pub title: String,
    pub lines: Vec<MoveLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StuckLine {
    pub id: String,
    pub label: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MovePlan {
    pub from: String,
    pub to: String,
    pub batches: Vec<MoveBatch>,
    pub stuck: Vec<StuckLine>,
    pub empty: bool,
    pub fee_note: Option<String>,
}
