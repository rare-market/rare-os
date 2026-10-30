//! Game ownership on Sui. Ports `src/sui/ownership.ts`.
//!
//! rare.market's games catalog (games.json) lists each Sui game's collections by Move type. A game
//! counts as owned when the address holds at least one object of one of those types:
//! rare.market's holdings (wallet and kiosks) first, then a direct gRPC read for the types it
//! doesn't index yet.

use std::collections::HashSet;
use std::sync::Mutex;

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::grpc::{owned_of_type, ChainRead};
use crate::ids::canon_type;
use crate::market::{self, HeldCollection, ItemCard, MarketApi};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameCollection {
    pub nft_type: String,
    pub name: String,
}

/// One Sui game in rare.market's catalog (lib/rare.ts `SuiGame`); the fields ownership needs,
/// the rest kept in `extra`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SuiGame {
    pub slug: String,
    pub name: String,
    pub collections: Vec<GameCollection>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedCollection {
    pub name: String,
    /// rare.market's slug when it has one (for "Your items").
    pub slug: Option<String>,
    #[serde(rename = "type")]
    pub collection_type: String,
    pub count: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameOwnership {
    pub slug: String,
    /// Objects held across the game's collections.
    pub count: f64,
    /// The first few items, for art.
    pub items: Vec<ItemCard>,
    pub collections: Vec<OwnedCollection>,
}

/// slug -> ownership, in catalog order (the TS Map's insertion order).
pub type OwnershipMap = Vec<(String, GameOwnership)>;

const MAX_ITEMS: usize = 8;

/// Whether an owned object's type belongs to a catalog collection type. A catalog type with type
/// arguments must match exactly; one without (`0x..::pawtato::LAND`) also matches any
/// instantiation of that struct.
pub fn type_matches(owned: &str, catalog: &str) -> bool {
    let o = canon_type(owned);
    let c = canon_type(catalog);
    if o == c {
        return true;
    }
    if c.contains('<') {
        return false;
    }
    o.split('<').next() == Some(c.as_str())
}

fn entry<'a>(m: &'a mut OwnershipMap, slug: &str) -> &'a mut GameOwnership {
    let i = match m.iter().position(|(s, _)| s == slug) {
        Some(i) => i,
        None => {
            m.push((slug.to_string(), GameOwnership { slug: slug.to_string(), count: 0.0, items: vec![], collections: vec![] }));
            m.len() - 1
        }
    };
    &mut m[i].1
}

/// Joins held collections with the catalog: only games with something held.
pub fn match_ownership(games: &[SuiGame], held: &[HeldCollection]) -> OwnershipMap {
    let mut out: OwnershipMap = vec![];
    for g in games {
        for col in &g.collections {
            for h in held {
                if !type_matches(&h.collection_type, &col.nft_type) {
                    continue;
                }
                let cur = entry(&mut out, &g.slug);
                cur.count += h.count;
                let room = MAX_ITEMS.saturating_sub(cur.items.len());
                cur.items.extend(h.items.iter().take(room).cloned());
                cur.collections.push(OwnedCollection {
                    name: if h.name.is_empty() { col.name.clone() } else { h.name.clone() },
                    slug: (!h.slug.is_empty()).then(|| h.slug.clone()),
                    collection_type: canon_type(&h.collection_type),
                    count: h.count,
                });
            }
        }
    }
    out
}

pub fn get<'a>(m: &'a OwnershipMap, slug: &str) -> Option<&'a GameOwnership> {
    m.iter().find(|(s, _)| s == slug).map(|(_, o)| o)
}

/// Ownership for every catalog game: rare.market first, gRPC for collection types it missed.
/// `on_first` gets rare.market's answer before the chain reads finish.
pub async fn read_ownership(api: &dyn MarketApi, chain: &dyn ChainRead, address: &str, games: &[SuiGame], on_first: Option<&(dyn Fn(&OwnershipMap) + Sync)>) -> OwnershipMap {
    let held = market::collections(api, address).await.unwrap_or_default();
    let mut map = match_ownership(games, &held);
    if let Some(f) = on_first {
        f(&map);
    }
    // Types rare.market had nothing for: ask the chain (wallet-held only; kiosks come from rare.market).
    let known: HashSet<String> = held.iter().map(|h| canon_type(&h.collection_type)).collect();
    let missing: Vec<&GameCollection> = games.iter().flat_map(|g| g.collections.iter().filter(|c| !known.iter().any(|k| type_matches(k, &c.nft_type)))).collect();
    let reads = join_all(missing.iter().map(|c| async move { (*c, owned_of_type(chain, address, &canon_type(&c.nft_type), 8).await.unwrap_or_default()) })).await;
    for (c, objs) in reads {
        let Some(first) = objs.first() else { continue };
        let h = HeldCollection {
            collection_type: first.object_type.clone(),
            count: objs.len() as f64,
            name: c.name.clone(),
            items: objs
                .iter()
                .map(|o| ItemCard {
                    id: o.object_id.clone(),
                    name: o.display_str("name").map(str::to_string).unwrap_or_else(|| c.name.clone()),
                    item_type: o.object_type.clone(),
                    image_url: o.display_str("image_url").map(str::to_string),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        for (slug, e) in match_ownership(games, std::slice::from_ref(&h)) {
            let cur = entry(&mut map, &slug);
            cur.count += e.count;
            cur.collections.extend(e.collections);
            let room = MAX_ITEMS.saturating_sub(cur.items.len());
            cur.items.extend(e.items.into_iter().take(room));
        }
    }
    map
}

/// Ownership for the current account, read once a minute at most (`useOwnership`).
#[derive(Default)]
pub struct OwnershipStore {
    inner: Mutex<OwnState>,
}

#[derive(Clone, Default)]
pub struct OwnState {
    pub address: Option<String>,
    pub by_slug: OwnershipMap,
    pub at_ms: f64,
    pub loading: bool,
}

impl OwnershipStore {
    pub fn snapshot(&self) -> OwnState {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Loads ownership for `address` unless it is fresh (under a minute) or already loading.
    pub async fn load(&self, api: &dyn MarketApi, chain: &dyn ChainRead, address: Option<&str>, games: &[SuiGame], now_ms: f64) {
        let Some(address) = address else {
            *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = OwnState::default();
            return;
        };
        {
            let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if s.loading || (s.address.as_deref() == Some(address) && now_ms - s.at_ms < 60_000.0) {
                return;
            }
            s.loading = true;
            if s.address.as_deref() != Some(address) {
                s.by_slug.clear();
                s.address = Some(address.to_string());
            }
        }
        let first = |m: &OwnershipMap| {
            let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if s.address.as_deref() == Some(address) {
                s.by_slug = m.clone();
            }
        };
        let all = read_ownership(api, chain, address, games, Some(&first)).await;
        let mut s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if s.address.as_deref() == Some(address) {
            s.by_slug = all;
            s.at_ms = now_ms;
        }
        s.loading = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::{Include, ObjectInfo};
    use crate::BoxFut;
    use serde_json::json;

    fn game(slug: &str, types: &[&str]) -> SuiGame {
        SuiGame { slug: slug.into(), name: slug.into(), collections: types.iter().map(|t| GameCollection { nft_type: t.to_string(), name: format!("{slug} items") }).collect(), extra: Map::new() }
    }

    fn held(t: &str, count: f64, slug: &str, name: &str) -> HeldCollection {
        HeldCollection { collection_type: t.into(), count, slug: slug.into(), name: name.into(), ..Default::default() }
    }

    // ownership.test.ts
    #[test]
    fn matches_the_same_type_written_two_ways() {
        assert!(type_matches(
            "0x09afb9a1c63a9bbaba650ef0a6b473b9874882cd63aab9570b99274e8f796f00::pawtato::LAND",
            "0x9afb9a1c63a9bbaba650ef0a6b473b9874882cd63aab9570b99274e8f796f00::pawtato::LAND"
        ));
    }

    #[test]
    fn a_catalog_type_without_type_arguments_matches_any_instantiation() {
        assert!(type_matches("0xee::suifrens::SuiFren<0x88::capy::Capy>", "0xee::suifrens::SuiFren"));
    }

    #[test]
    fn a_catalog_type_with_type_arguments_must_match_exactly() {
        assert!(!type_matches("0xbd::nonfungible::Nft<0xe2::a::A>", "0xbd::nonfungible::Nft<0xe3::b::B>"));
        assert!(type_matches("0xbd::nonfungible::Nft<0xe2::a::A>", "0xbd::nonfungible::Nft<0xe2::a::A>"));
    }

    #[test]
    fn different_modules_or_structs_never_match() {
        assert!(!type_matches("0x1::a::B", "0x1::a::C"));
        assert!(!type_matches("0x1::a::B", "0x2::a::B"));
    }

    #[test]
    fn joins_held_collections_to_games_and_sums_counts() {
        let games = [game("pawtato", &["0x1::pawtato::LAND", "0x2::heroes::HERO"]), game("other", &["0x9::x::Y"])];
        let held = [held("0x1::pawtato::LAND", 2.0, "pawtato-land", "Pawtato Land"), held("0x2::heroes::HERO", 3.0, "", "Heroes"), held("0x5::z::Z", 1.0, "z", "Z")];
        let m = match_ownership(&games, &held);
        assert_eq!(m.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["pawtato"]);
        let p = get(&m, "pawtato").unwrap();
        assert_eq!(p.count, 5.0);
        assert_eq!(p.collections.iter().map(|c| c.slug.clone()).collect::<Vec<_>>(), [Some("pawtato-land".to_string()), None]);
    }

    struct Market(Value);
    impl MarketApi for Market {
        fn get(&self, _p: &str) -> BoxFut<'_, Value> {
            let v = self.0.clone();
            Box::pin(async move { Ok(v) })
        }
    }
    struct Chain;
    impl ChainRead for Chain {
        fn owned_objects(&self, _o: &str, t: Option<&str>, _max: usize, _i: Include) -> BoxFut<'_, Vec<ObjectInfo>> {
            let t = t.unwrap_or("").to_string();
            Box::pin(async move {
                Ok(if t.ends_with("::heroes::HERO") {
                    vec![ObjectInfo { object_id: "0xh1".into(), object_type: t, display: Some(json!({ "name": "Hero #1", "image_url": "https://h/1.png" })), ..Default::default() }]
                } else {
                    vec![]
                })
            })
        }
    }

    #[tokio::test]
    async fn chain_fills_the_types_rare_market_missed() {
        let games = [game("pawtato", &["0x1::pawtato::LAND", "0x2::heroes::HERO"])];
        let api = Market(json!({ "data": [{ "type": "0x1::pawtato::LAND", "count": 2, "slug": "pawtato-land", "name": "Pawtato Land", "items": [] }] }));
        let firsts = Mutex::new(0usize);
        let first = |m: &OwnershipMap| {
            assert_eq!(get(m, "pawtato").unwrap().count, 2.0);
            *firsts.lock().unwrap() += 1;
        };
        let m = read_ownership(&api, &Chain, "0x1", &games, Some(&first)).await;
        assert_eq!(*firsts.lock().unwrap(), 1);
        let p = get(&m, "pawtato").unwrap();
        assert_eq!(p.count, 3.0);
        assert_eq!(p.items[0].name, "Hero #1");
        assert_eq!(p.items[0].image_url.as_deref(), Some("https://h/1.png"));
        assert_eq!(p.collections.len(), 2);
        assert_eq!(p.collections[1].slug, None);
    }

    #[tokio::test]
    async fn store_reads_at_most_once_a_minute() {
        let games = [game("pawtato", &["0x1::pawtato::LAND"])];
        let api = Market(json!({ "data": [{ "type": "0x1::pawtato::LAND", "count": 1 }] }));
        let s = OwnershipStore::default();
        s.load(&api, &Chain, Some("0x1"), &games, 1_000.0).await;
        assert_eq!(s.snapshot().at_ms, 1_000.0);
        s.load(&api, &Chain, Some("0x1"), &games, 30_000.0).await;
        assert_eq!(s.snapshot().at_ms, 1_000.0);
        s.load(&api, &Chain, Some("0x1"), &games, 70_000.0).await;
        assert_eq!(s.snapshot().at_ms, 70_000.0);
        s.load(&api, &Chain, None, &games, 80_000.0).await;
        assert!(s.snapshot().address.is_none());
    }
}
