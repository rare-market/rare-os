//! rare.market /v1 for the native marketplace: the response shapes (market/api/src/routes:
//! discovery, collections, items, market, holders), the paths, and reads through the data cache.
//! Ported from launcher/src/marketplace/api.ts.
//!
//! Every read goes through `rare-state` ([`crate::net`]): display data answers from memory or disk
//! at once and is revalidated behind by the host's policy; data a transaction is built from (an
//! item's listing, offers, kiosks, policies) is always fetched. Screens that want to redraw when a
//! revalidation brings new data subscribe to the store's change listener for the key
//! ([`crate::net::market_key`]).

use crate::net::{self, Net};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub type Extra = Map<String, Value>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionRef {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub verified: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionRow {
    #[serde(flatten)]
    pub r#ref: CollectionRef,
    pub rank: u32,
    pub supply: u64,
    pub floor_mist: Option<String>,
    pub floor_change_pct: Option<f64>,
    pub top_offer_mist: Option<String>,
    pub volume_mist: String,
    pub volume_change_pct: Option<f64>,
    pub sales: u64,
    pub listed_count: u64,
    pub owners: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Featured {
    /// "new" or "featured".
    pub kind: String,
    pub title: String,
    pub image_url: Option<String>,
    pub href: String,
    pub floor_mist: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mover {
    #[serde(flatten)]
    pub r#ref: CollectionRef,
    pub floor_mist: Option<String>,
    pub volume_change_pct: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HomeFeed {
    pub featured: Vec<Featured>,
    pub movers: Vec<Mover>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionStats {
    pub floor_mist: Option<String>,
    pub floor_change24h_pct: Option<f64>,
    pub top_offer_mist: Option<String>,
    pub volume7d_mist: Option<String>,
    pub sales7d: u64,
    pub listed_count: u64,
    pub supply: u64,
    pub owners: u64,
    pub unique_owner_pct: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TraitType {
    #[serde(rename = "type")]
    pub kind: String,
    pub value_count: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionDetail {
    #[serde(flatten)]
    pub r#ref: CollectionRef,
    pub description: Option<String>,
    pub banner_url: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub royalty_bps: Option<f64>,
    pub stats: Option<CollectionStats>,
    pub trait_types: Vec<TraitType>,
    pub venues: Vec<String>,
}

/// A listing as the API sends it, on an item card or GET /v1/items/:id. The builder fields
/// (`source`, `orderKey`, `objectId`, `nftType`, `extra`) are there on the item endpoint.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Listing {
    pub venue: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
    pub price_mist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_in_mist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_mist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commission_mist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seller: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nft_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kiosk_id: Option<String>,
    /// `royaltyMist`, `feeBps`, and the venue details the builder needs (`listingType`, `inOrderbook`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<Extra>,
}

impl Listing {
    pub fn extra_str(&self, k: &str) -> Option<String> {
        self.extra.as_ref().and_then(|e| e.get(k)).and_then(json_num_str)
    }
}

/// A JSON string or number as its decimal text (amounts arrive either way).
pub fn json_num_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rarity {
    pub rank: u64,
    pub tier: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Staked {
    pub protocol: Option<String>,
    pub name: Option<String>,
    /// "staked" or "nested".
    pub via: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemCard {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub image_url: Option<String>,
    pub collection: Option<CollectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rarity: Option<Rarity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<Listing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sale_mist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_offer_mist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kiosk_id: Option<String>,
    /// Set when a staking contract (or an object the owner holds) keeps the item for its owner: it
    /// counts as theirs but cannot be listed or sent until unstaked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staked: Option<Staked>,
    /// Inventory only: the collection's floor (cheapest all-in listing), MIST.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floor_mist: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attribute {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
    pub pct: Option<f64>,
    pub floor_mist: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemDetail {
    #[serde(flatten)]
    pub card: ItemCard,
    pub description: Option<String>,
    pub attributes: Option<Vec<Attribute>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OfferItem {
    pub id: String,
    pub name: String,
    pub image_url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OfferTrait {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
}

/// An offer or bid (GET /v1/offers), rare.market api/src/routes/market.ts toOffers. Amounts are MIST
/// as decimal strings. With `?item=` the API returns offers on that item plus collection offers it
/// can fill, open and fillable only, highest price first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Offer {
    pub id: String,
    pub venue: String,
    /// Where the bid lives, e.g. "tradeport/bids", "tradeport/kiosk-bids", "drip/v2-bids".
    pub source: String,
    /// Move type the bid is for (None for a few legacy bids).
    pub nft_type: Option<String>,
    pub fee_mist: String,
    pub royalty_mist: String,
    pub commission_mist: String,
    pub pool_id: Option<String>,
    /// False when the bid exists but cannot be filled (pool short, frozen store).
    pub fillable: bool,
    pub extra: Extra,
    /// "collection", "item" or "trait".
    pub kind: String,
    pub collection: CollectionRef,
    pub item: Option<OfferItem>,
    #[serde(rename = "trait")]
    pub trait_: Option<OfferTrait>,
    pub price_mist: String,
    pub quantity: Option<u64>,
    pub filled: Option<u64>,
    pub bidder: String,
    /// Kiosk the bidder receives items in (rare's own offers).
    pub dest_kiosk_id: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    /// "open", "filled", "cancelled" or "expired".
    pub status: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventItem {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub rarity: Option<Rarity>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketEvent {
    pub id: String,
    /// "sale", "list", "delist", "offer", "offer_cancel", "transfer" or "mint".
    pub kind: String,
    pub venue: Option<String>,
    pub item: Option<EventItem>,
    pub collection: Option<CollectionRef>,
    pub price_mist: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub from_name: Option<String>,
    pub to_name: Option<String>,
    pub at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Holder {
    pub rank: u64,
    pub address: String,
    pub name: Option<String>,
    pub count: u64,
    pub pct_of_supply: f64,
    pub listed: u64,
    pub staked: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NamedCount {
    pub name: String,
    pub count: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HoldersHeader {
    pub owners: u64,
    pub unique_owners_pct: f64,
    pub top10_pct: f64,
    pub top100_pct: f64,
    pub items_held: u64,
    pub supply: u64,
    pub staked_or_locked: Vec<NamedCount>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HoldersPage {
    pub data: Vec<Holder>,
    pub next_cursor: Option<String>,
    pub total: u64,
    pub header: HoldersHeader,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TraitValue {
    #[serde(rename = "type")]
    pub kind: String,
    pub value: String,
    pub count: u64,
    pub pct: f64,
    pub floor_mist: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchCollection {
    #[serde(flatten)]
    pub r#ref: CollectionRef,
    pub floor_mist: Option<String>,
    pub volume_mist: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchAccount {
    pub address: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchResult {
    pub collections: Vec<SearchCollection>,
    pub accounts: Vec<SearchAccount>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Status {
    pub sui_usd: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    PriceAsc,
    PriceDesc,
    RarityAsc,
    ListedDesc,
}

impl Sort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Sort::PriceAsc => "price_asc",
            Sort::PriceDesc => "price_desc",
            Sort::RarityAsc => "rarity_asc",
            Sort::ListedDesc => "listed_desc",
        }
    }
}

/// `encodeURIComponent`.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Event filters for [`paths::events`].
#[derive(Clone, Debug, Default)]
pub struct EventQuery<'a> {
    pub collection: Option<&'a str>,
    pub item: Option<&'a str>,
    pub account: Option<&'a str>,
}

/// The rare.market paths the marketplace reads (api.ts `paths`).
pub mod paths {
    use super::*;
    pub fn status() -> String {
        "/v1/status".into()
    }
    pub fn home() -> String {
        "/v1/home".into()
    }
    pub fn collections(window: &str, sort: &str, limit: u32) -> String {
        format!("/v1/collections?window={window}&sort={sort}&limit={limit}")
    }
    /// `collections(window)` with the TS defaults (volume, 20).
    pub fn collections_default(window: &str) -> String {
        collections(window, "volume", 20)
    }
    pub fn collection(slug: &str) -> String {
        format!("/v1/collections/{}", enc(slug))
    }
    pub fn items(slug: &str, sort: Sort, traits: &str) -> String {
        let t = if traits.is_empty() { String::new() } else { format!("&traits={}", enc(traits)) };
        format!("/v1/collections/{}/items?sort={}&limit=36{t}", enc(slug), sort.as_str())
    }
    pub fn traits(slug: &str) -> String {
        format!("/v1/collections/{}/traits", enc(slug))
    }
    pub fn holders(slug: &str) -> String {
        format!("/v1/collections/{}/holders?limit=25", enc(slug))
    }
    pub fn item(id: &str) -> String {
        format!("/v1/items/{}", enc(id))
    }
    pub fn events(q: &EventQuery, limit: u32) -> String {
        let mut s = format!("/v1/events?limit={limit}");
        if let Some(c) = q.collection.filter(|c| !c.is_empty()) {
            s.push_str(&format!("&collection={}", enc(c)));
        }
        if let Some(i) = q.item.filter(|c| !c.is_empty()) {
            s.push_str(&format!("&item={}", enc(i)));
        }
        if let Some(a) = q.account.filter(|c| !c.is_empty()) {
            s.push_str(&format!("&account={}", enc(a)));
        }
        s
    }
    pub fn account_items(address: &str) -> String {
        format!("/v1/accounts/{}/items?limit=36", enc(address))
    }
    pub fn search(q: &str) -> String {
        format!("/v1/search?q={}", enc(q))
    }
    pub fn offers_for_item(id: &str, limit: u32) -> String {
        format!("/v1/offers?item={}&limit={limit}", enc(id))
    }
    pub fn kiosks(address: &str) -> String {
        format!("/v1/accounts/{}/kiosks", enc(address))
    }
    pub fn policies(nft_type: &str) -> String {
        format!("/v1/policies/{}", enc(nft_type))
    }
}

/// Reads `path` through the cache and parses it.
pub fn get<T: serde::de::DeserializeOwned>(n: &dyn Net, path: &str) -> crate::Result<T> {
    Ok(net::get_market(n, path)?)
}

/// What the cache holds for `path` (the seed a screen draws its first frame from), without fetching.
pub fn cached<T: serde::de::DeserializeOwned>(n: &dyn Net, path: &str) -> Option<T> {
    net::peek_market(n, path)
}

// ---------- paged lists ----------

/// Items with an id, for de-duplicating appended pages.
pub trait Keyed {
    fn key(&self) -> Option<&str>;
}

impl Keyed for ItemCard {
    fn key(&self) -> Option<&str> {
        Some(&self.id)
    }
}
impl Keyed for MarketEvent {
    fn key(&self) -> Option<&str> {
        Some(&self.id)
    }
}
impl Keyed for Holder {
    fn key(&self) -> Option<&str> {
        None
    }
}
impl Keyed for Value {
    fn key(&self) -> Option<&str> {
        self.get("id").and_then(|v| v.as_str())
    }
}

/// The path of a cursor page of `base` (`usePaged`'s join): `base` alone for the first page.
pub fn page_path(base: &str, cursor: Option<&str>) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    let s = format!("{base}{sep}{}", cursor.map(|c| format!("cursor={}", enc(c))).unwrap_or_default());
    s.trim_end_matches(['?', '&']).to_string()
}

/// A cursor-paged list (items grid, activity), as `usePaged` keeps it: the first page revalidates
/// like any read, [`Paged::more`] appends the next page without duplicates.
#[derive(Clone, Debug, PartialEq)]
pub struct Paged<T> {
    pub base: String,
    pub items: Vec<T>,
    pub cursor: Option<String>,
    pub total: Option<u64>,
    pub done: bool,
    /// When the first page was fetched (ms), for the screen's own TTL.
    pub at_ms: u64,
}

impl<T: Keyed + Clone + serde::de::DeserializeOwned> Paged<T> {
    pub fn from_page(base: &str, p: Page<T>, at_ms: u64) -> Self {
        Paged { base: base.to_string(), done: p.next_cursor.is_none(), items: p.data, cursor: p.next_cursor, total: p.total, at_ms }
    }

    /// The first page from the cache only (drawn in the first frame).
    pub fn seeded(n: &dyn Net, base: &str) -> Option<Self> {
        cached::<Page<T>>(n, &page_path(base, None)).map(|p| Self::from_page(base, p, 0))
    }

    /// The first page through the cache.
    pub fn load(n: &dyn Net, base: &str, now_ms: u64) -> crate::Result<Self> {
        Ok(Self::from_page(base, get::<Page<T>>(n, &page_path(base, None))?, now_ms))
    }

    /// The next page's path, or None when there is none.
    pub fn next_path(&self) -> Option<String> {
        if self.done {
            return None;
        }
        self.cursor.as_deref().map(|c| page_path(&self.base, Some(c)))
    }

    /// Appends a fetched next page, skipping items already shown.
    pub fn append(&mut self, p: Page<T>) {
        let seen: std::collections::HashSet<String> = self.items.iter().filter_map(|x| x.key().map(String::from)).collect();
        self.items.extend(p.data.into_iter().filter(|x| x.key().is_none_or(|k| !seen.contains(k))));
        self.done = p.next_cursor.is_none();
        self.cursor = p.next_cursor;
        self.total = p.total.or(self.total);
    }

    /// Fetches and appends the next page; false when there was none.
    pub fn more(&mut self, n: &dyn Net) -> crate::Result<bool> {
        let Some(path) = self.next_path() else { return Ok(false) };
        let p = get::<Page<T>>(n, &path)?;
        self.append(p);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::MockNet;
    use serde_json::json;

    #[test]
    fn paths_match_the_web() {
        assert_eq!(paths::collections_default("24h"), "/v1/collections?window=24h&sort=volume&limit=20");
        assert_eq!(paths::items("retro sui", Sort::PriceAsc, "Hat:Red"), "/v1/collections/retro%20sui/items?sort=price_asc&limit=36&traits=Hat%3ARed");
        assert_eq!(paths::items("x", Sort::RarityAsc, ""), "/v1/collections/x/items?sort=rarity_asc&limit=36");
        assert_eq!(paths::events(&EventQuery { item: Some("0x1"), ..Default::default() }, 30), "/v1/events?limit=30&item=0x1");
        assert_eq!(paths::policies("0x2::a::B"), "/v1/policies/0x2%3A%3Aa%3A%3AB");
        assert_eq!(enc("a b&c/é"), "a%20b%26c%2F%C3%A9");
    }

    #[test]
    fn pages_join_and_append_without_duplicates() {
        assert_eq!(page_path("/v1/events?limit=30", None), "/v1/events?limit=30");
        assert_eq!(page_path("/v1/x", Some("c 1")), "/v1/x?cursor=c%201");
        let n = MockNet::new();
        n.on_read_json("/v1/accounts/0x1/items?limit=36", json!({"data": [{"id": "a", "name": "A", "type": "T"}], "nextCursor": "c1"}));
        n.on_read_json("/v1/accounts/0x1/items?limit=36&cursor=c1", json!({"data": [{"id": "a"}, {"id": "b"}], "nextCursor": null, "total": 2}));
        let mut p = Paged::<ItemCard>::load(&n, &paths::account_items("0x1"), 1).unwrap();
        assert!(!p.done);
        assert!(p.more(&n).unwrap());
        assert_eq!(p.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert!(p.done);
        assert_eq!(p.total, Some(2));
        assert!(!p.more(&n).unwrap());
    }

    #[test]
    fn shapes_parse_from_live_like_json() {
        let c: ItemCard = serde_json::from_value(json!({
            "id": "0x1", "name": "RetroSui #1", "type": "0x2::a::B", "imageUrl": null,
            "collection": {"slug": "retrosui", "name": "RetroSui", "imageUrl": null, "verified": true},
            "listing": {"venue": "kiosk", "priceMist": "12", "extra": {"royaltyMist": 3}},
            "staked": {"protocol": null, "name": "Pool", "via": "staked"}
        }))
        .unwrap();
        assert_eq!(c.listing.as_ref().unwrap().extra_str("royaltyMist").as_deref(), Some("3"));
        assert_eq!(c.collection.unwrap().slug, "retrosui");
        assert!(c.staked.is_some());
    }
}
