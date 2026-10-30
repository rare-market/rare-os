//! Market buys from an address balance (no keys, nothing signed or sent): builds the launcher's
//! purchase for the cheapest listings of the top collections, for `sender`, and dry-runs it.
//!
//! `cargo run -p rare-market --example abbuy -- <sender> [max SUI] [collections]`

use std::sync::Arc;

use rare_market::api::{self, paths, CollectionRow, ItemCard, ItemDetail, Page, Sort};
use rare_market::net::{Call, LineStream, Net, NetError, Reply, MARKET, TRADE};
use rare_sui::grpc::ChainRead;

struct Live(ureq::Agent);

impl Net for Live {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        let url = if let Some(p) = key.strip_prefix("rare:") {
            if p.starts_with("http") { p.to_string() } else { format!("{MARKET}{p}") }
        } else if let Some(p) = key.strip_prefix("trade:") {
            format!("{TRADE}{p}")
        } else {
            return Err(NetError::NotAllowed(key.into()));
        };
        let r = self.0.get(&url).call().map_err(|e| NetError::Status(format!("{url}: {e}")))?;
        Ok(r.into_string().map_err(|e| NetError::Network(e.to_string()))?.into())
    }
    fn peek(&self, _key: &str) -> Option<Arc<str>> {
        None
    }
    fn call(&self, c: &Call) -> Result<Reply, NetError> {
        let mut r = self.0.request(c.method.as_str(), &c.url);
        for (k, v) in &c.headers {
            r = r.set(k, v);
        }
        let res = match &c.body {
            Some(b) => r.set("content-type", "application/json").send_string(b),
            None => r.call(),
        };
        match res {
            Ok(r) => Ok(Reply { status: r.status(), body: r.into_string().unwrap_or_default() }),
            Err(ureq::Error::Status(s, r)) => Ok(Reply { status: s, body: r.into_string().unwrap_or_default() }),
            Err(e) => Err(NetError::Network(e.to_string())),
        }
    }
    fn stream(&self, _c: &Call) -> Result<LineStream, NetError> {
        Err(NetError::NotAllowed("stream".into()))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sender = args.first().expect("sender").clone();
    let max_mist = (args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(1.0) * 1e9) as u128;
    let want = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4usize);
    let n = Live(ureq::AgentBuilder::new().user_agent("RareLauncher/0.1").build());
    let sui = rare_sui::Sui::current(&rare_sui::NetworkConfig::mainnet()).unwrap();
    let b = sui.balance(&sender, "0x2::sui::SUI").await.unwrap();
    println!("sender SUI {} (coins {:?}, address balance {:?})", b.balance, b.coin_balance, b.address_balance);

    let cols: Page<CollectionRow> = api::get(&n, &paths::collections("1d", "volume", 30)).unwrap();
    let mut tried = 0;
    for c in cols.data {
        if tried >= want {
            break;
        }
        let Ok(items) = api::get::<Page<ItemCard>>(&n, &paths::items(&c.r#ref.slug, Sort::PriceAsc, "")) else { continue };
        let Some(card) = items.data.into_iter().find(|i| i.listing.as_ref().is_some_and(|l| l.price_mist.parse::<u128>().is_ok_and(|p| p > 0 && p <= max_mist))) else { continue };
        let Ok(d) = api::get::<ItemDetail>(&n, &paths::item(&card.id)) else { continue };
        let Some(listing) = d.card.listing.clone() else { continue };
        tried += 1;
        let label = format!("{} / {} on {} for {} MIST", c.r#ref.slug, d.card.name, listing.venue, listing.price_mist);
        let tx = match rare_market::buy::build_buy_transaction(&n, &Default::default(), &d.card, &listing, &sender, None, None) {
            Ok(t) => t,
            Err(e) => {
                println!("{label}: not buildable here: {}", e.0);
                continue;
            }
        };
        let mut t = tx.clone();
        t.set_sender(&sender);
        match rare_sui::tx::build(&sui, &t, &Default::default()).await {
            Ok(built) => {
                let res = built.data.gas_payment.objects.iter().filter(|o| o.digest().inner()[12..32] == [0xac; 20]).count();
                let sim = sui.simulate(&built.data).await.unwrap();
                let st = sim.transaction.as_ref().and_then(|t| t.effects.as_ref()).and_then(|e| e.status.as_ref());
                println!("{label}: built, {} gas object(s) ({res} reservation), dry run success {:?} {:?}", built.data.gas_payment.objects.len(), st.and_then(|s| s.success), st.and_then(|s| s.error.as_ref()).and_then(|e| e.description.clone()));
            }
            Err(e) => println!("{label}: build FAILED {e}"),
        }
    }
}
