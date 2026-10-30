//! rare.market notify API types (contract: /v1/notify on rare.market). The JSON shapes match the
//! web launcher's types.ts field for field, so notices stored by either read back in the other.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Where the notify API lives on mainnet. The client builds its URLs from the net's market origin
/// instead, so testnet works the same way.
pub const NOTIFY_BASE: &str = "https://rare.market/v1/notify";

/// The notify API under a rare.market origin.
pub fn notify_base(origin: &str) -> String {
    format!("{}/v1/notify", origin.trim_end_matches('/'))
}

macro_rules! notice_kinds {
    ($($v:ident => $s:literal),* $(,)?) => {
        /// What a notice is about. Chain kinds come from rare.market; `followed_trade` (Trade >
        /// Traders) and the friend kinds (from the stream's friend events) are made in the launcher
        /// with `notify_local`. A kind a newer server sends is kept as `Other` so it still shows.
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum NoticeKind {
            $($v,)*
            Other(String),
        }

        /// Every kind this launcher knows, in the contract's order.
        pub const NOTICE_KINDS: &[NoticeKind] = &[$(NoticeKind::$v),*];

        impl NoticeKind {
            /// The wire name ("sold", "offer_received", ...).
            pub fn as_str(&self) -> &str {
                match self {
                    $(NoticeKind::$v => $s,)*
                    NoticeKind::Other(s) => s,
                }
            }

            /// The kind for a wire name; unknown names become `Other`.
            pub fn parse(s: &str) -> NoticeKind {
                match s {
                    $($s => NoticeKind::$v,)*
                    other => NoticeKind::Other(other.to_string()),
                }
            }
        }
    };
}

notice_kinds! {
    Sold => "sold",
    Bought => "bought",
    OfferReceived => "offer_received",
    CoinReceived => "coin_received",
    NftReceived => "nft_received",
    Airdrop => "airdrop",
    PerpLiquidation => "perp_liquidation",
    PriceAlert => "price_alert",
    FollowedTrade => "followed_trade",
    FriendRequest => "friend_request",
    FriendOnline => "friend_online",
    PartyInvite => "party_invite",
    PartyMessage => "party_message",
    DirectMessage => "direct_message",
    OsUpdate => "os_update",
    DownloadDone => "download_done",
    EmuUpdate => "emu_update",
    Trophy => "trophy",
    Battery => "battery",
}

impl Default for NoticeKind {
    fn default() -> Self {
        NoticeKind::Other(String::new())
    }
}

impl std::fmt::Display for NoticeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for NoticeKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NoticeKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(NoticeKind::parse(&String::deserialize(d)?))
    }
}

/// What A does on a notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum NoticeAction {
    Item {
        item_id: String,
        slug: Option<String>,
    },
    Collection {
        slug: String,
    },
    Position {
        market_id: String,
        symbol: String,
    },
    Coin {
        coin_type: String,
    },
    Tx {
        digest: String,
    },
    Trader {
        address: String,
    },
    Friends {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        address: Option<String>,
    },
    Party {
        party_id: String,
    },
    OsUpdate,
    Downloads,
    Emulators,
    Trophies,
    Battery,
    /// An action kind from a newer server (or none at all): A falls back to the transaction.
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Amount {
    pub raw: String,
    pub decimals: u32,
    pub symbol: String,
    pub coin_type: String,
    pub usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CollectionRef {
    pub slug: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NoticeItem {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub collection: Option<CollectionRef>,
}

/// One notice. Ids are decimal strings that only grow (they can pass 2^53); times are epoch ms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Notice {
    pub id: String,
    pub kind: NoticeKind,
    pub addr: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub image_url: Option<String>,
    pub amount: Option<Amount>,
    pub item: Option<NoticeItem>,
    pub action: NoticeAction,
    pub digest: Option<String>,
    #[serde(deserialize_with = "opt_ms")]
    pub cp_ts: Option<i64>,
    #[serde(deserialize_with = "ms")]
    pub created_at: i64,
    #[serde(deserialize_with = "ms")]
    pub sent_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AlertDir {
    #[default]
    Above,
    Below,
}

impl AlertDir {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertDir::Above => "above",
            AlertDir::Below => "below",
        }
    }
}

/// A price alert, scoped to this launcher's device id on rare.market.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PriceAlert {
    pub id: String,
    pub coin_type: String,
    pub symbol: String,
    pub dir: AlertDir,
    pub target: f64,
    #[serde(deserialize_with = "ms")]
    pub created_at: i64,
    #[serde(deserialize_with = "opt_ms")]
    pub fired_at: Option<i64>,
    pub active: bool,
}

/// Epoch ms as JSON numbers, whole or not; a null or junk value is 0, as `n.createdAt || 0` reads it.
fn ms<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(opt_ms(d)?.unwrap_or(0))
}

fn opt_ms<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_f64().filter(|x| x.is_finite()).map(|x| x as i64))
}

/// The epoch time in ms by the system clock.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notices_read_the_contract_shape() {
        let v = json!({
            "id": "42", "kind": "sold", "addr": "0x1", "title": "t", "body": null, "imageUrl": "https://x/y.png",
            "amount": { "raw": "70", "decimals": 9, "symbol": "SUI", "coinType": "0x2::sui::SUI", "usd": 1.5 },
            "item": { "id": "0xa", "name": "A", "imageUrl": null, "collection": { "slug": "s", "name": "S" } },
            "action": { "kind": "item", "itemId": "0xa", "slug": "s" },
            "digest": "d", "cpTs": 1700000000000.0, "createdAt": 1700000000100u64, "sentAt": 1700000000200u64
        });
        let n: Notice = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(n.kind, NoticeKind::Sold);
        assert_eq!(n.action, NoticeAction::Item { item_id: "0xa".into(), slug: Some("s".into()) });
        assert_eq!(n.cp_ts, Some(1_700_000_000_000));
        let back = serde_json::to_value(&n).unwrap();
        assert_eq!(back["action"], v["action"]);
        assert_eq!(back["imageUrl"], v["imageUrl"]);
        assert_eq!(back["createdAt"], json!(1700000000100i64));
    }

    #[test]
    fn unknown_kinds_and_actions_survive() {
        let n: Notice = serde_json::from_value(json!({ "id": "1", "kind": "something_new", "title": "x", "action": { "kind": "warp" } })).unwrap();
        assert_eq!(n.kind.as_str(), "something_new");
        assert_eq!(n.action, NoticeAction::Unknown);
        assert_eq!(n.created_at, 0);
        let f: NoticeAction = serde_json::from_value(json!({ "kind": "friends" })).unwrap();
        assert_eq!(f, NoticeAction::Friends { address: None });
        assert_eq!(serde_json::to_value(NoticeAction::OsUpdate).unwrap(), json!({ "kind": "os_update" }));
        assert_eq!(NOTICE_KINDS.len(), 19);
    }
}
