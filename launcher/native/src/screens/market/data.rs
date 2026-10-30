//! What the Marketplace pages read: rare.market's answers by path (api.ts `useRare` / `usePaged`,
//! paths from `rare_market::api::paths`), the clock they are relative to, and the selected Rare
//! Wallet account. The app fills it from rare-market's client; the golden fixtures fill it from
//! answers captured off the web launcher, so native and web show the same data.

use rare_market::api::{page_path, paths, Page, Status};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;

/// The selected Rare Wallet account (keystore.current()): Your NFTs and the buy sheet read it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub address: String,
    pub label: String,
    /// Rare Wallet can sign with it (not a watch account): `canBuyInApp`.
    pub signer: bool,
}

/// What `Answers::rows` found.
#[derive(Clone, Debug, PartialEq)]
pub struct Rows<T> {
    pub items: Vec<T>,
    pub total: Option<u64>,
    /// The next page to fetch when the person nears the end.
    pub next: Option<String>,
    /// The first page has arrived.
    pub ready: bool,
}

/// `nearEnd`: focus is within the last two rows of a `cols`-wide list of `count`.
pub fn near_end(idx: Option<usize>, count: usize, cols: usize) -> bool {
    idx.map(|i| i + cols * 2 >= count).unwrap_or(false)
}

#[derive(Clone, Debug, Default)]
pub struct Answers {
    /// Wall clock of the answers in ms (relative times are against this).
    pub now_ms: f64,
    map: HashMap<String, Value>,
    /// Paths whose last fetch failed and have no answer yet (the pages say "Could not load").
    failed: std::collections::HashSet<String>,
    pub account: Option<Account>,
}

impl Answers {
    /// `{"now": ms, "api": {"/v1/...": body}}`, the fixture shape.
    pub fn from_json(s: &str) -> Answers {
        let v: Value = serde_json::from_str(s).expect("market fixture");
        let now_ms = v.get("now").and_then(Value::as_f64).unwrap_or(0.0);
        let map = v.get("api").and_then(Value::as_object).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
        Answers { now_ms, map, failed: Default::default(), account: None }
    }

    /// The answer for `path`, if it has arrived and parses.
    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        self.map.get(path).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn set(&mut self, path: impl Into<String>, body: Value) {
        self.map.insert(path.into(), body);
    }

    /// Every answer of `other` too (the fixture shots merge the pages' captures).
    pub fn absorb(&mut self, other: Answers) {
        self.map.extend(other.map);
    }

    pub fn fail(&mut self, path: impl Into<String>) {
        self.failed.insert(path.into());
    }

    pub fn unfail(&mut self, path: &str) {
        self.failed.remove(path);
    }

    /// The path failed to load and nothing arrived for it.
    pub fn failed(&self, path: &str) -> bool {
        self.failed.contains(path) && !self.map.contains_key(path)
    }

    pub fn has(&self, path: &str) -> bool {
        self.map.contains_key(path)
    }

    /// A page of rows (`usePaged`'s first page).
    pub fn page<T: DeserializeOwned>(&self, path: &str) -> Option<Page<T>> {
        self.get(path)
    }

    /// A cursor-paged list as `usePaged` keeps it: every page of `base` that has arrived, in cursor
    /// order, the total, and the path of the next page (None at the end or while one is missing).
    pub fn rows<T: DeserializeOwned>(&self, base: &str) -> Rows<T> {
        let mut out = Rows { items: Vec::new(), total: None, next: None, ready: false };
        let mut path = page_path(base, None);
        while let Some(p) = self.get::<Page<T>>(&path) {
            if !out.ready {
                out.total = p.total;
                out.ready = true;
            }
            out.items.extend(p.data);
            match p.next_cursor {
                Some(c) => path = page_path(base, Some(&c)),
                None => return out,
            }
        }
        if out.ready {
            out.next = Some(path);
        }
        out
    }

    /// SUI in USD from /v1/status (0 when unknown).
    pub fn sui_usd(&self) -> Option<f64> {
        self.get::<Status>(&paths::status()).map(|s| s.sui_usd).filter(|v| *v > 0.0)
    }

    /// Parses an ISO time ("2026-10-27T18:40:00.000Z") to ms, for `timeAgo`.
    pub fn ago(&self, iso: &str) -> String {
        rare_market::format::time_ago(Some(iso), self.now_ms as i64)
    }
}

pub fn home() -> Answers {
    Answers::from_json(include_str!("fixtures/home.json"))
}
pub fn collection() -> Answers {
    Answers::from_json(include_str!("fixtures/collection.json"))
}
pub fn item() -> Answers {
    Answers::from_json(include_str!("fixtures/item.json"))
}
pub fn search() -> Answers {
    Answers::from_json(include_str!("fixtures/search.json"))
}
pub fn account() -> Answers {
    Answers::from_json(include_str!("fixtures/account.json"))
}

/// The demo watch wallet the web shots use.
pub const DEMO: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
/// The listed Popkins item the item and buy shots open (scripts/shots.mjs ITEM).
pub const ITEM: &str = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::api::{CollectionDetail, CollectionRow, HoldersPage, ItemCard, ItemDetail, MarketEvent, SearchResult, Sort, TraitValue};

    #[test]
    fn fixtures_parse() {
        let h = home();
        assert!(h.sui_usd().is_some());
        let rows: Page<CollectionRow> = h.page(&paths::collections("24h", "volume", 25)).unwrap();
        assert_eq!(rows.data[0].r#ref.name, "Popkins");
        let c = collection();
        let d: CollectionDetail = c.get(&paths::collection("popkins")).unwrap();
        assert!(d.stats.is_some());
        assert!(c.page::<ItemCard>(&paths::items("popkins", Sort::PriceAsc, "")).unwrap().data.len() >= 12);
        assert!(c.get::<HoldersPage>(&paths::holders("popkins")).is_some());
        assert!(c.get::<Page<TraitValue>>(&paths::traits("popkins")).is_some());
        let ev: Page<MarketEvent> = c.page(&paths::events(&rare_market::api::EventQuery { collection: Some("popkins"), ..Default::default() }, 30)).unwrap();
        assert!(!ev.data.is_empty());
        let i = item();
        let it: ItemDetail = i.get(&paths::item(ITEM)).unwrap();
        assert!(it.card.listing.is_some());
        assert!(search().get::<SearchResult>(&paths::search("pop")).is_some());
        assert!(account().page::<ItemCard>(&paths::account_items(DEMO)).is_some());
    }

    #[test]
    fn rows_follow_the_cursor_and_name_the_next_page() {
        let mut a = Answers::default();
        let base = "/v1/collections/x/items?sort=price_asc&limit=36";
        assert!(!a.rows::<serde_json::Value>(base).ready);
        a.set(base, serde_json::json!({"data": [1, 2], "nextCursor": "c1", "total": 5}));
        let r = a.rows::<serde_json::Value>(base);
        assert_eq!((r.items.len(), r.total, r.next.as_deref()), (2, Some(5), Some("/v1/collections/x/items?sort=price_asc&limit=36&cursor=c1")));
        a.set("/v1/collections/x/items?sort=price_asc&limit=36&cursor=c1", serde_json::json!({"data": [3], "nextCursor": null}));
        let r = a.rows::<serde_json::Value>(base);
        assert_eq!((r.items.len(), r.next), (3, None));
        assert!(near_end(Some(1), 3, 1) && !near_end(Some(0), 3, 1) && !near_end(Some(0), 13, 6) && near_end(Some(1), 13, 6) && !near_end(None, 1, 1));
    }
}
