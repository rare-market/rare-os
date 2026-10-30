//! The Items and Market tabs' live data (overlay/data.ts loadItems, loadMarket, game.ts catalogFor):
//! which Sui game is under the menu, and its collections read from rare.market through the
//! launcher's market net. The menu starts one load when it opens and picks the answer up in `tick`.

use super::model::OverlayItem;
use crate::data::catalog::SuiGame;
use crate::data::model::Game;
use crate::screens::overlay::panels::data::{merge_market, sort_items, Item, Listing};
use crate::screens::overlay::panels::trade::norm_type;
use rare_market::api::{self, enc, CollectionDetail, ItemCard, Page};
use std::sync::mpsc::{channel, Receiver};

/// One of the game's collections: Move type and name.
pub type Coll = (String, String);

pub struct Loaded {
    pub items: Vec<Item>,
    pub market: Vec<Item>,
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// game.ts catalogFor: the catalog entry for a run id (a web slug, a library game by its site, a
/// Steam app id from the profiles, else by title).
pub fn catalog_for<'a>(rid: &str, lib: Option<&Game>, games: &'a [SuiGame]) -> Option<&'a SuiGame> {
    let by_slug = |slug: &str| games.iter().find(|g| g.slug == slug);
    if let Some(slug) = rid.strip_prefix("web:") {
        if let Some(g) = by_slug(slug) {
            return Some(g);
        }
    }
    if let Some(url) = lib.and_then(|l| l.launch.as_ref()).and_then(|l| l.url.as_deref()) {
        if let Some((slug, _)) = rare_library::profiles::for_game(rid, url) {
            if let Some(g) = by_slug(&slug) {
                return Some(g);
            }
        }
    }
    let steam = rid.strip_prefix("steam:").and_then(|n| n.parse::<u64>().ok()).or_else(|| lib.and_then(|l| l.launch.as_ref()).and_then(|l| l.appid));
    if let Some(id) = steam {
        let hit = games.iter().find(|g| crate::screens::sui::catalog::profile(&g.slug).and_then(|p| p.native).is_some_and(|n| n.steam_app_ids.contains(&id)));
        if hit.is_some() {
            return hit;
        }
    }
    let title = lib.map(|l| norm(&l.title)).filter(|t| !t.is_empty())?;
    games.iter().find(|g| norm(&g.name) == title || crate::screens::sui::catalog::profile(&g.slug).and_then(|p| p.native).is_some_and(|n| n.titles.iter().any(|t| norm(t) == title)))
}

/// The collections of the Sui game a run is (empty for anything that is not one).
pub fn collections_of(rid: &str, lib: Option<&Game>, games: &[SuiGame]) -> Vec<Coll> {
    catalog_for(rid, lib, games).map(|g| g.collections.iter().map(|c| (norm_type(&c.nft_type), c.name.clone())).collect()).unwrap_or_default()
}

fn mist(v: &Option<String>) -> Option<i128> {
    v.as_deref().and_then(|s| s.parse::<i128>().ok())
}

fn item_of(c: &ItemCard, floor: Option<i128>) -> Item {
    let image = c.image_url.as_deref().and_then(|u| crate::data::art::single(u, 96.0));
    Item {
        id: c.id.clone(),
        name: c.name.clone(),
        nft_type: c.listing.as_ref().and_then(|l| l.nft_type.clone()),
        image,
        collection: c.collection.as_ref().map(|r| r.name.clone()),
        listing: c.listing.as_ref().map(|l| Listing { price_mist: l.price_mist.parse().unwrap_or(0), all_in_mist: mist(&l.all_in_mist), venue: l.venue.clone(), seller: l.seller.clone() }),
        floor_mist: floor,
        top_offer_mist: mist(&c.top_offer_mist),
        last_sale_mist: mist(&c.last_sale_mist),
    }
}

/// loadItems and loadMarket for the game's collections, blocking: run it on a worker.
pub fn load(net: &dyn rare_market::net::Net, address: Option<&str>, colls: &[Coll]) -> Loaded {
    let per: Vec<(Vec<Item>, Vec<Item>)> = std::thread::scope(|sc| {
        let hs: Vec<_> = colls
            .iter()
            .map(|(ty, _)| {
                sc.spawn(move || {
                    let detail = api::get::<CollectionDetail>(net, &api::paths::collection(ty)).ok();
                    let stats = detail.as_ref().and_then(|d| d.stats.as_ref());
                    let floor = stats.and_then(|s| mist(&s.floor_mist));
                    let mine = address
                        .and_then(|a| api::get::<Page<ItemCard>>(net, &format!("/v1/accounts/{}/items?collection={}&limit=60", enc(a), enc(ty))).ok())
                        .map(|p| p.data.iter().map(|c| item_of(c, floor)).collect())
                        .unwrap_or_default();
                    let market = detail
                        .as_ref()
                        .filter(|d| !d.r#ref.slug.is_empty())
                        .and_then(|d| api::get::<Page<ItemCard>>(net, &format!("/v1/collections/{}/items?sort=price_asc&limit=12", enc(&d.r#ref.slug))).ok())
                        .map(|p| p.data.iter().map(|c| item_of(c, floor)).collect())
                        .unwrap_or_default();
                    (mine, market)
                })
            })
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    let items: Vec<Item> = per.iter().flat_map(|p| p.0.clone()).collect();
    let pages: Vec<Vec<Item>> = per.into_iter().map(|p| p.1).collect();
    Loaded { items: sort_items(&items), market: merge_market(&pages, address, 12) }
}

static RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A purchase finished: the open menu reads its tabs again.
pub fn request_reload() {
    RELOAD.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub fn take_reload() -> bool {
    RELOAD.swap(false, std::sync::atomic::Ordering::SeqCst)
}

/// Starts the load on a thread against the launcher's market net.
pub fn spawn(address: Option<String>, colls: Vec<Coll>) -> Receiver<Loaded> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let l = load(crate::screens::wallet::live::market_net(), address.as_deref(), &colls);
        let _ = tx.send(l);
    });
    rx
}

/// What the Game tab's "Your items" line reads.
pub fn overlay_items(items: &[Item]) -> Vec<OverlayItem> {
    items.iter().map(|i| OverlayItem { name: i.name.clone(), floor_mist: i.floor_mist.map(|m| m.max(0) as u128), top_offer_mist: i.top_offer_mist.map(|m| m.max(0) as u128) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::catalog::Collection;

    fn g(slug: &str, name: &str, colls: &[&str]) -> SuiGame {
        SuiGame { slug: slug.into(), name: name.into(), collections: colls.iter().map(|c| Collection { nft_type: (*c).into(), name: "C".into() }).collect(), ..Default::default() }
    }

    #[test]
    fn a_web_run_finds_its_collections() {
        let games = vec![g("act", "ACT", &["0x2::act::Hero"]), g("other", "Other", &[])];
        let c = collections_of("web:act", None, &games);
        assert_eq!(c.len(), 1);
        assert!(c[0].0.ends_with("::act::Hero"));
        assert!(collections_of("web:other", None, &games).is_empty());
        assert!(collections_of("steam:620", None, &games).is_empty());
    }
}
