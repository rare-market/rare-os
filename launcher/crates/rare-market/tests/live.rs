//! Mainnet dry run of buying, simulate only (the ported launcher/src/marketplace/venues/live.test.ts):
//! `LIVE=1 cargo test -p rare-market --test live -- --ignored --nocapture`.
//!
//! Picks real listings under 50 SUI from rare.market (one per common source), builds each purchase
//! with `build_buy_transaction` for a funded address and builds it on the node with
//! `rare_sui::tx::build`, which simulates it and fails if the simulation does. Nothing is signed and
//! nothing is executed.

use rare_market::api::{ItemCard, Page};
use rare_market::buy::{build_buy_transaction, StoreIds};
use rare_market::net::{Call, LineStream, Net, NetError, Reply};
use rare_market::venues::VenueConfig;
use std::collections::BTreeMap;
use std::sync::Arc;

const BUYER: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const MAX: i128 = 50_000_000_000;
const WANT: [&str; 3] = ["tradeport/simple", "kiosk/native", "tradeport/kiosk"];

/// rare.market straight over HTTP, optionally answering no kiosks for the buyer.
struct Live {
    no_kiosks: bool,
}

impl Net for Live {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        let path = key.strip_prefix("rare:").ok_or_else(|| NetError::NotAllowed(key.into()))?;
        if self.no_kiosks && path.ends_with("/kiosks") {
            return Ok(Arc::from(r#"{"data":[]}"#));
        }
        match ureq::get(&format!("https://rare.market{path}")).call() {
            Ok(r) => Ok(Arc::from(r.into_string().map_err(|e| NetError::Network(e.to_string()))?)),
            Err(ureq::Error::Status(c, _)) => Err(NetError::Status(format!("rare.market {c} {path}: status code {c}"))),
            Err(e) => Err(NetError::Network(e.to_string())),
        }
    }
    fn peek(&self, _: &str) -> Option<Arc<str>> {
        None
    }
    fn call(&self, c: &Call) -> Result<Reply, NetError> {
        Err(NetError::NotAllowed(c.url.clone()))
    }
    fn stream(&self, c: &Call) -> Result<LineStream, NetError> {
        Err(NetError::NotAllowed(c.url.clone()))
    }
}

fn get<T: serde::de::DeserializeOwned>(n: &Live, path: &str) -> Option<T> {
    serde_json::from_str(&n.read(&format!("rare:{path}")).ok()?).ok()
}

/// The cheapest listing under 50 SUI for each wanted source, from the busiest collections.
fn pick_listings(n: &Live) -> BTreeMap<String, ItemCard> {
    let mut picks = BTreeMap::new();
    #[derive(serde::Deserialize)]
    struct Col {
        slug: String,
    }
    let cols: Page<Col> = get(n, "/v1/collections?window=1d&sort=volume&limit=30").expect("collections");
    for c in cols.data {
        if WANT.iter().all(|s| picks.contains_key(*s)) {
            break;
        }
        let page: Page<ItemCard> = get(n, &format!("/v1/collections/{}/items?sort=price_asc&limit=36", c.slug)).unwrap_or(Page { data: vec![], next_cursor: None, total: None });
        for it in page.data {
            let Some(l) = &it.listing else { continue };
            let Some(src) = l.source.clone().filter(|s| WANT.contains(&s.as_str()) && !picks.contains_key(s)) else { continue };
            let all_in: i128 = l.all_in_mist.as_deref().unwrap_or(&l.price_mist).parse().unwrap_or(i128::MAX);
            if all_in <= MAX {
                picks.insert(src, it);
            }
        }
    }
    picks
}

fn simulate(n: &Live, item: &ItemCard) -> Result<(), String> {
    let l = item.listing.as_ref().unwrap();
    let tx = build_buy_transaction(n, &VenueConfig::default(), item, l, BUYER, None, Some(&StoreIds::default())).map_err(|e| e.0)?;
    // One runtime for every build: rare-sui keeps one client per network, bound to the runtime
    // it first connected on.
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let rt = RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap());
    rt.block_on(async {
        let sui = rare_sui::Sui::current(&rare_sui::NetworkConfig::mainnet()).map_err(|e| e.to_string())?;
        rare_sui::tx::build(&sui, &tx, &Default::default()).await.map(|b| println!("  built {} bytes, digest {}", b.bytes.len(), b.digest)).map_err(|e| e.to_string())
    })
}

#[test]
#[ignore = "mainnet; LIVE=1"]
fn buy_on_mainnet_simulate_only() {
    if std::env::var("LIVE").is_err() {
        return;
    }
    let n = Live { no_kiosks: false };
    let picks = pick_listings(&n);
    println!("buyer {BUYER}");
    assert!(!picks.is_empty());
    for (source, item) in &picks {
        let l = item.listing.as_ref().unwrap();
        println!("\n== {source}: {} {}\n  listed {} MIST, all-in {:?}", item.name, item.id, l.price_mist, l.all_in_mist);
        simulate(&n, item).unwrap();
    }
    // A buyer with no kiosk yet: the PTB makes one.
    let n2 = Live { no_kiosks: true };
    let item = picks.get("kiosk/native").or_else(|| picks.get("tradeport/kiosk")).or_else(|| picks.values().next()).unwrap();
    println!("\n== new kiosk, {}: {}", item.listing.as_ref().unwrap().source.as_deref().unwrap_or(""), item.name);
    simulate(&n2, item).unwrap();
}
