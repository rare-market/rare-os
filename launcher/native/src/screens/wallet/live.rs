//! The wallet's live reads, off the UI thread: the portfolio (balances over gRPC with the Trade API's
//! prices, `rare_sui::balances::portfolio`), the activity feed (the Trade API, `rare_sui::activity`) and the
//! cross-game inventory (rare.market through the instant cache, `rare_market::inventory`). Each
//! answer comes back on a channel the page drains in `tick`.

use super::types::{ActivityChange, ActivityItem, ActivityKind, Portfolio, PortfolioToken};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, OnceLock};

pub enum Answer {
    Portfolio(String, Result<Portfolio, String>),
    Activity(String, Result<(Vec<ActivityItem>, Option<String>), String>),
    Inventory(String, Result<rare_market::inventory::Inventory, String>),
    /// One account's portfolio total in USD for the Accounts page; None when it could not be read.
    AccountValue(String, Option<f64>),
}

/// One tokio runtime for the screens' chain reads (gRPC needs one).
pub fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("rare-wallet-io").build().expect("tokio runtime"))
}

/// rare.market reads through the launcher's instant cache (`~/.cache/rare/state`).
pub fn market_net() -> &'static rare_market::net::StoreNet {
    static NET: OnceLock<rare_market::net::StoreNet> = OnceLock::new();
    NET.get_or_init(|| rare_market::net::StoreNet::new(rare_system::instant::open().state))
}

pub fn sui() -> Option<&'static rare_sui::Sui> {
    static SUI: OnceLock<Option<rare_sui::Sui>> = OnceLock::new();
    SUI.get_or_init(|| {
        let _g = rt().enter();
        rare_sui::Sui::new(rare_sui::Network::Mainnet, rare_sui::network::DEFAULT_RPC_MAINNET).ok()
    })
    .as_ref()
}

fn http() -> Arc<dyn rare_sui::prices::HttpGet> {
    static H: OnceLock<Arc<rare_sui::prices::ReqwestGet>> = OnceLock::new();
    H.get_or_init(|| {
        let _g = rt().enter();
        Arc::new(rare_sui::prices::ReqwestGet::default())
    })
    .clone()
}

fn portfolio_of(p: rare_sui::balances::Portfolio) -> Portfolio {
    Portfolio {
        total_usd: p.total_usd,
        change_24h_usd: p.change24h_usd,
        priced: p.priced,
        tokens: p
            .tokens
            .into_iter()
            .map(|t| PortfolioToken {
                coin_type: t.balance.coin_type,
                symbol: t.balance.symbol,
                name: t.balance.name,
                decimals: t.balance.decimals,
                icon_url: t.balance.icon_url,
                raw: t.balance.raw as i128,
                amount: t.balance.amount,
                price_usd: t.price_usd,
                value_usd: t.value_usd,
                change_24h: t.change24h,
                verified: t.verified,
            })
            .collect(),
    }
}

fn activity_of(a: rare_sui::activity::ActivityItem) -> ActivityItem {
    use rare_sui::activity::{Kind, Status};
    ActivityItem {
        digest: a.digest,
        timestamp: a.timestamp,
        kind: match a.kind {
            Kind::Send => ActivityKind::Send,
            Kind::Receive => ActivityKind::Receive,
            Kind::Swap => ActivityKind::Swap,
            Kind::Other => ActivityKind::Other,
        },
        changes: a.changes.into_iter().map(|c| ActivityChange { coin_type: c.coin_type, amount: c.amount, symbol: c.symbol, decimals: c.decimals }).collect(),
        summary: a.summary,
        counterparty: a.counterparty,
        failed: a.status == Status::Failed,
        count: a.count,
    }
}

/// Starts the reads for `address`; answers arrive on the returned channel.
pub fn load(address: &str, inventory: bool) -> Receiver<Answer> {
    let (tx, rx) = channel();
    let a = address.to_string();
    let t = tx.clone();
    rt().spawn(async move {
        let r = match sui() {
            Some(s) => {
                let trade = rare_sui::prices::PriceClient::new(rare_sui::prices::TRADE_URL, http());
                rare_sui::balances::portfolio(s, &trade, &a).await.map(portfolio_of).map_err(|e| e.to_string())
            }
            None => Err("Sui is not reachable".into()),
        };
        let _ = t.send(Answer::Portfolio(a, r));
    });
    let a = address.to_string();
    let t = tx.clone();
    rt().spawn(async move {
        let feed = rare_sui::activity::ActivityFeed::new(None, rare_sui::Network::Mainnet, Some(http()));
        let r = feed.list(&a, None).await.map(|p| (p.items.into_iter().map(activity_of).collect(), p.cursor)).map_err(|e| e.to_string());
        let _ = t.send(Answer::Activity(a, r));
    });
    if inventory {
        let a = address.to_string();
        std::thread::Builder::new()
            .name("rare-inventory".into())
            .spawn(move || {
                let net = market_net();
                let games = || {
                    Ok(crate::screens::sui::catalog::seed()
                        .into_iter()
                        .map(|g| rare_market::inventory::SuiGame {
                            slug: g.slug,
                            name: g.name,
                            logo_url: g.logo_url,
                            collections: g.collections.into_iter().map(|c| rare_market::inventory::GameCollection { nft_type: c.nft_type, name: c.name }).collect(),
                        })
                        .collect())
                };
                let r = rare_market::inventory::load_inventory(net, &a, games).map_err(|e| e.to_string());
                let _ = tx.send(Answer::Inventory(a, r));
            })
            .ok();
    }
    rx
}

/// The next activity page after `cursor`.
pub fn more(address: &str, cursor: &str) -> Receiver<Answer> {
    let (tx, rx) = channel();
    let (a, c) = (address.to_string(), cursor.to_string());
    rt().spawn(async move {
        let feed = rare_sui::activity::ActivityFeed::new(None, rare_sui::Network::Mainnet, Some(http()));
        let r = feed.list(&a, Some(&c)).await.map(|p| (p.items.into_iter().map(activity_of).collect(), p.cursor)).map_err(|e| e.to_string());
        let _ = tx.send(Answer::Activity(a, r));
    });
    rx
}

/// The portfolio total of each address, one read per address, for the Accounts page.
pub fn values(addresses: &[String]) -> Receiver<Answer> {
    let (tx, rx) = channel();
    for a in addresses {
        let (a, t) = (a.clone(), tx.clone());
        rt().spawn(async move {
            let v = match sui() {
                Some(s) => {
                    let trade = rare_sui::prices::PriceClient::new(rare_sui::prices::TRADE_URL, http());
                    rare_sui::balances::portfolio(s, &trade, &a).await.ok().and_then(|p| p.total_usd)
                }
                None => None,
            };
            let _ = t.send(Answer::AccountValue(a, v));
        });
    }
    rx
}
