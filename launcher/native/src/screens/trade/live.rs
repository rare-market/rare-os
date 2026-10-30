//! Trade on live data and its actions. Every page of the Trade stack reads the Trade API (the Trade API)
//! through the launcher's one cached net (`wallet::live::market_net`): the cached answer at once,
//! then the network, the page on top re-read at the web launcher's intervals. Balances come from
//! the chain. Quotes follow the ticket (300 ms after it settles); a review sheet that opens builds
//! its transaction (router swap, perps order, close, mirror) and Confirm hands it to Rare Wallet
//! (`crate::sign::sign_trade`) on a worker. The Market tab's `LiveMarket` hands every Trade act here.

use super::fmt::normalize_type;
use super::model::*;
use super::perp::{order_view, PerpData, PerpPage, Sheet};
use super::review::{self, Phase};
use super::root::{RootData, RootPage, Wallet};
use super::swap::{SwapData, SwapPage};
use super::tab::{Fixtures, Source, TradeTab, View};
use super::ticket::{Coin, Quote, SUI_TYPE};
use super::token::{sides, Side, TokenData, TokenPage, TIMEFRAMES};
use super::trader::{mirror_plan, MirrorPlan, TraderData, TraderPage};
use super::traders::{BoardExtras, Follow, LeaderRow, Rows, TradersData, Window};
use crate::screens::market::host::Page;
use crate::screens::market::live::{post, AppAct};
use crate::screens::overlay::common::Cx;
use rare_market::net::{Method, Net};
use rare_trade::chain::ChainReader;
use rare_trade::swap::{QuoteParams, Swap, SwapQuote};
use rare_trade::tx::UnsignedTx;
use rare_trade::wallet::{SignOutcome, SignRequest};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

fn wall_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

fn net() -> Arc<dyn Net> {
    Arc::new(crate::screens::wallet::live::market_net().clone())
}

// ---------------------------------------------------------------- paths

mod p {
    pub use rare_trade::api::token_paths as tok;
    pub use rare_trade::perps::perp_paths as perp;
    pub use rare_trade::traders::api::trader_paths as tr;
    pub const LOGOS: &str = rare_trade::api::PERP_LOGO_PATH;
}

fn win(w: Window) -> rare_trade::traders::api::BoardWindow {
    if w == Window::D7 {
        rare_trade::traders::api::BoardWindow::D7
    } else {
        rare_trade::traders::api::BoardWindow::D30
    }
}

fn bars_key(market: &str, tf: usize) -> String {
    format!("bars:{market}:{tf}")
}

fn bal_key(addr: &str) -> String {
    format!("bal:{}", addr.to_lowercase())
}

fn preview_key(market: &str, side: PerpSide, margin: f64, lev: f64) -> String {
    format!("preview:{market}:{}:{margin}:{lev}", side.lower())
}

fn side_of(s: PerpSide) -> rare_trade::perps::Side {
    match s {
        PerpSide::Long => rare_trade::perps::Side::Long,
        PerpSide::Short => rare_trade::perps::Side::Short,
    }
}

/// One read the pages need.
#[derive(Clone, Debug, PartialEq)]
enum Job {
    /// A cached Trade API path.
    Path(String),
    /// Perps bars for a market and timeframe (the window ends now, so never cached).
    Bars { market: String, tf: usize },
    /// The wallet's coin balances from the chain.
    Balances(String),
    Preview { market: String, side: PerpSide, margin: f64, lev: f64 },
}

impl Job {
    fn key(&self) -> String {
        match self {
            Job::Path(p) => p.clone(),
            Job::Bars { market, tf } => bars_key(market, *tf),
            Job::Balances(a) => bal_key(a),
            Job::Preview { market, side, margin, lev } => preview_key(market, *side, *margin, *lev),
        }
    }

    fn run(&self, n: &dyn Net, fresh: bool) -> Result<Value, String> {
        match self {
            Job::Path(path) => {
                if fresh {
                    rare_trade::http::refresh::<Value>(n, path).map_err(|e| e.to_string())
                } else {
                    rare_trade::http::get::<Value>(n, path).map_err(|e| e.to_string())
                }
            }
            Job::Bars { market, tf } => {
                let (_, res, span) = super::fmt::PERP_TIMEFRAMES[*tf];
                let now = wall_ms();
                let path = p::perp::candles(market, res, now - span, now);
                let r = rare_trade::http::trade_call(n, Method::Get, &path, None, None).map_err(|e| e.to_string())?;
                if r.ok() {
                    Ok(r.body)
                } else {
                    Err(rare_trade::http::error_of(r.status, &r.body).to_string())
                }
            }
            Job::Balances(addr) => {
                let sui = crate::screens::wallet::live::sui().ok_or("Sui is not reachable")?;
                let list = on_rt(rare_sui::grpc::ChainRead::list_balances(sui, addr)).map_err(|e| e.to_string())?;
                Ok(Value::Object(list.into_iter().map(|b| (normalize_type(&b.coin_type), Value::String(b.balance.to_string()))).collect()))
            }
            Job::Preview { market, side, margin, lev } => rare_trade::perps::preview_open(n, market, side_of(*side), *margin, *lev).map(|p| serde_json::to_value(p).unwrap_or(Value::Null)).map_err(|e| e.to_string()),
        }
    }
}

/// Runs a rare-sui future to its end from any thread (also from inside another runtime).
fn on_rt<T: Send>(f: impl std::future::Future<Output = T> + Send) -> T {
    std::thread::scope(|s| s.spawn(|| crate::screens::wallet::live::rt().block_on(f)).join().expect("chain read"))
}

/// Trade's chain reads over the wallet's gRPC client.
pub struct SuiReader;

impl ChainReader for SuiReader {
    fn balance(&self, owner: &str, coin_type: &str) -> rare_trade::Result<u128> {
        let sui = crate::screens::wallet::live::sui().ok_or_else(|| rare_trade::TradeApiError::network("Sui is not reachable"))?;
        let (o, t) = (owner.to_string(), coin_type.to_string());
        on_rt(async move { rare_sui::grpc::ChainRead::balance(sui, &o, &t).await }).map(|b| b.balance as u128).map_err(|e| rare_trade::TradeApiError::network(e.to_string()))
    }
    fn object_types(&self, ids: &[String]) -> rare_trade::Result<Vec<Option<String>>> {
        let sui = crate::screens::wallet::live::sui().ok_or_else(|| rare_trade::TradeApiError::network("Sui is not reachable"))?;
        let ids = ids.to_vec();
        let r = on_rt(async move { rare_sui::grpc::ChainRead::objects(sui, &ids, rare_sui::grpc::Include::NONE).await }).map_err(|e| rare_trade::TradeApiError::network(e.to_string()))?;
        Ok(r.into_iter().map(|o| o.ok().map(|o| o.object_type)).collect())
    }
    fn coin_metadata(&self, coin_type: &str) -> rare_trade::Result<Option<rare_trade::chain::CoinMetadata>> {
        let sui = crate::screens::wallet::live::sui().ok_or_else(|| rare_trade::TradeApiError::network("Sui is not reachable"))?;
        let t = coin_type.to_string();
        let m = on_rt(async move { rare_sui::grpc::ChainRead::coin_metadata(sui, &t).await }).map_err(|e| rare_trade::TradeApiError::network(e.to_string()))?;
        Ok(m.map(|m| rare_trade::chain::CoinMetadata { symbol: m.symbol, name: m.name, decimals: m.decimals, icon_url: m.icon_url }))
    }
}

// ---------------------------------------------------------------- Ask Rare

/// The swap client Ask Rare's wallet_swap uses (the same device key and router as Trade).
pub fn agent_swap() -> Arc<Swap> {
    Arc::new(Swap::new(net(), Arc::new(rare_trade::swap::KvKeys(Arc::new(FileKv::open()))), crate::screens::wallet::service::identity_maker()))
}

/// A swap Ask Rare prepared, after the review sheet: a toast, and balances read again.
pub fn note_outcome(o: &rare_trade::wallet::SignOutcome, done: &str) {
    use rare_trade::wallet::SignOutcome;
    match o {
        SignOutcome::Sent { .. } => {
            crate::data::keep::nudge();
            crate::features::bus::note(done.to_string(), false);
        }
        SignOutcome::Rejected => {}
        SignOutcome::Failed(e) => crate::features::bus::note(e.clone(), true),
    }
}

// ---------------------------------------------------------------- follows (on the device)

struct FileKv(std::path::PathBuf);

impl FileKv {
    fn open() -> FileKv {
        let base = std::env::var("XDG_STATE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
        FileKv(base.join("rare-native").join("trade.json"))
    }
    fn all(&self) -> serde_json::Map<String, Value> {
        std::fs::read(&self.0).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    fn save(&self, m: serde_json::Map<String, Value>) {
        if let Some(d) = self.0.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&self.0, serde_json::to_vec(&m).unwrap_or_default());
    }
}

impl rare_trade::kv::Kv for FileKv {
    fn get(&self, key: &str) -> Option<String> {
        self.all().get(key).and_then(|v| v.as_str().map(str::to_string))
    }
    fn set(&self, key: &str, value: &str) {
        let mut m = self.all();
        m.insert(key.into(), Value::String(value.into()));
        self.save(m);
    }
    fn remove(&self, key: &str) {
        let mut m = self.all();
        m.remove(key);
        self.save(m);
    }
}

// ---------------------------------------------------------------- the store

#[derive(Default)]
pub struct Store {
    map: HashMap<String, Value>,
    /// When each key last arrived (clock secs).
    at: HashMap<String, f64>,
    failed: HashSet<String>,
    flight: HashSet<String>,
    ver: u64,
    acct: Option<crate::sign::Account>,
    follows: Vec<Follow>,
    /// The swap picker's search: the query and its rows.
    search: Option<(String, Vec<TokenRow>)>,
}

impl Store {
    fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.map.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }
    fn items<T: DeserializeOwned + Default>(&self, key: &str) -> Vec<T> {
        self.get::<Items<T>>(key).map(|i| i.items).unwrap_or_default()
    }
    fn bad(&self, key: &str) -> bool {
        self.failed.contains(key) && !self.map.contains_key(key)
    }
    fn wallet(&self) -> Option<&str> {
        self.acct.as_ref().map(|a| a.address.as_str())
    }
    fn signer(&self) -> bool {
        self.acct.as_ref().is_some_and(|a| a.signer)
    }
    fn balances(&self) -> HashMap<String, u128> {
        let Some(a) = self.wallet() else { return HashMap::new() };
        self.get::<HashMap<String, String>>(&bal_key(a)).map(|m| m.into_iter().map(|(k, v)| (k, v.parse().unwrap_or(0))).collect()).unwrap_or_default()
    }
    fn account(&self) -> Option<Option<PerpsAccount>> {
        let a = self.wallet()?;
        let v = self.map.get(&p::perp::account(a))?;
        Some(v.get("account").cloned().and_then(|x| serde_json::from_value(x).ok()))
    }
    fn logos(&self) -> Vec<PerpLogo> {
        self.items(p::LOGOS)
    }

    fn traders(&self, w: Window) -> TradersData {
        let board = p::tr::board(win(w));
        TradersData {
            now_ms: wall_ms(),
            board: self.get::<Rows<LeaderRow>>(&board).map(|r| r.rows).unwrap_or_default(),
            board_loaded: self.map.contains_key(&board),
            board_error: self.bad(&board),
            extras: self.get::<BoardExtras>(&p::tr::extras(win(w))),
            teams: self.get::<Rows<LeaderRow>>(&p::tr::teams()).map(|r| r.rows).unwrap_or_default(),
            follows: self.follows.clone(),
        }
    }

    pub fn root(&self, w: Window) -> RootData {
        let (tr, ga, fr) = (p::tok::list(rare_trade::api::TokenList::Trending, 16), p::tok::list(rare_trade::api::TokenList::Gainers, 6), p::tok::list(rare_trade::api::TokenList::New, 6));
        let wallet = self.acct.as_ref().map(|a| Wallet { address: a.address.clone(), label: a.label.clone(), watch: !a.signer });
        let pf = self.wallet().map(p::tok::portfolio);
        RootData {
            now_ms: wall_ms(),
            trending: self.items(&tr),
            gainers: self.items(&ga),
            fresh: self.items(&fr),
            markets: self.items(&p::perp::markets()),
            logos: self.logos(),
            portfolio: pf.as_deref().and_then(|k| self.get(k)),
            account: self.account(),
            wallet,
            ready: true,
            trending_error: self.bad(&tr),
            gainers_error: self.bad(&ga),
            fresh_error: self.bad(&fr),
            perps_error: self.bad(&p::perp::markets()),
            portfolio_error: pf.as_deref().is_some_and(|k| self.bad(k)),
            account_error: self.wallet().is_some_and(|a| self.bad(&p::perp::account(a))),
            traders: self.traders(w),
        }
    }

    pub fn token(&self, coin: &str, tf: usize) -> TokenData {
        let t = normalize_type(coin);
        let token = self.get::<TokenRow>(&p::tok::token(&t));
        let holders_path = p::tok::holders(&t, 20);
        TokenData {
            now_ms: wall_ms(),
            coin_type: t.clone(),
            token_error: (token.is_none() && self.bad(&p::tok::token(&t))).then(|| "This coin is not available right now.".to_string()),
            token,
            sui: self.get(&p::tok::lean(SUI_TYPE)),
            candles: self.get::<Value>(&p::tok::candles(&t, TIMEFRAMES[tf], 300)).and_then(|v| v.get("candles").cloned()).and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default(),
            trades: self.items(&p::tok::trades(&t)),
            trades_error: self.bad(&p::tok::trades(&t)),
            holders: self.get(&holders_path),
            holders_error: self.bad(&holders_path),
            wallet: self.wallet().map(str::to_string),
            balances: self.balances(),
            quote: None,
        }
    }

    pub fn perp(&self, market: &str, tf: usize, t: Option<&super::perp::Ticket>) -> PerpData {
        let markets: Vec<PerpMarket> = self.items(&p::perp::markets());
        let m = markets.iter().find(|m| m.market_id.eq_ignore_ascii_case(market)).cloned();
        let m_missing = m.is_none();
        let logos = self.logos();
        let logo = m.as_ref().and_then(|m| perp_logo(&logos, &m.market_id, &m.symbol)).map(str::to_string);
        let usdc = self.balances().get(&normalize_type(super::trader::USDC_TYPE)).map(|v| *v as f64 / 1e6).or(self.wallet().map(|_| 0.0));
        let (preview, preview_error) = match t {
            Some(t) => {
                let k = preview_key(market, t.side, t.margin, t.lev);
                (self.get::<OrderPreview>(&k), self.failed.contains(&k).then(|| "Couldn't price this order right now.".to_string()))
            }
            None => (None, None),
        };
        PerpData {
            now_ms: wall_ms(),
            market: m,
            logo,
            bars: self.get::<Value>(&bars_key(market, tf)).and_then(|v| v.get("bars").cloned()).and_then(|b| serde_json::from_value(b).ok()).unwrap_or_default(),
            account: self.account(),
            preview,
            preview_error,
            usdc: self.wallet().and(usdc),
            wallet: self.wallet().map(str::to_string),
            // A failed read, or the list arrived without this market: no endless loading bar.
            markets_error: self.bad(&p::perp::markets()) || (m_missing && self.get::<Value>(&p::perp::markets()).is_some()),
        }
    }

    pub fn swap(&self) -> SwapData {
        let mut tokens: Vec<TokenRow> = self.get::<Value>(&p::tok::pick_list(60)).and_then(|v| v.get("tokens").cloned()).and_then(|t| serde_json::from_value(t).ok()).unwrap_or_default();
        if let Some((_, rows)) = &self.search {
            let seen: Vec<String> = rows.iter().map(|r| normalize_type(&r.coin_type)).collect();
            tokens.retain(|t| !seen.contains(&normalize_type(&t.coin_type)));
            tokens.splice(0..0, rows.iter().cloned());
        }
        SwapData { now_ms: wall_ms(), tokens, portfolio: self.wallet().and_then(|a| self.get(&p::tok::portfolio(a))), wallet: self.wallet().map(str::to_string), balances: self.balances(), quote: None, quote_error: None }
    }

    pub fn trader(&self, address: &str, win30: bool) -> TraderData {
        let w = win(if win30 { Window::D30 } else { Window::D7 });
        let a = address.to_lowercase();
        TraderData {
            now_ms: wall_ms(),
            address: a.clone(),
            profile: self.get(&p::tr::profile(&a)),
            stats: self.get(&p::tr::stats(&a, w)),
            stats_error: self.bad(&p::tr::stats(&a, w)),
            pnl: self.get::<super::trader::Points>(&p::tr::pnl(&a, w)).map(|p| p.points),
            pnl_error: self.bad(&p::tr::pnl(&a, w)),
            trades: self.get(&p::tr::trades(&a, 25)),
            trades_error: self.bad(&p::tr::trades(&a, 25)),
            linked: self.get(&p::tr::linked(&a)),
            follows: self.follows.clone(),
            wallet: self.wallet().map(str::to_string),
        }
    }
}

/// The reads a page needs now, with how often to re-read them while it is on top (secs).
fn wants(st: &Store, view: &View, page: &mut dyn Page) -> Vec<(Job, f64)> {
    let path = |s: String, every: f64| (Job::Path(s), every);
    let mut w = Vec::new();
    let wallet = st.wallet().map(str::to_string);
    let bal = |w: &mut Vec<(Job, f64)>| {
        if let Some(a) = &wallet {
            w.push((Job::Balances(a.clone()), 20.0));
        }
    };
    let any = page.any_mut();
    match view {
        View::Root(_) => {
            use rare_trade::api::TokenList::*;
            w.push(path(p::tok::list(Trending, 16), 30.0));
            w.push(path(p::tok::list(Gainers, 6), 30.0));
            w.push(path(p::tok::list(New, 6), 30.0));
            w.push(path(p::perp::markets(), 10.0));
            w.push(path(p::LOGOS.to_string(), 600.0));
            if let Some(a) = &wallet {
                w.push(path(p::tok::portfolio(a), 20.0));
                w.push(path(p::perp::account(a), 15.0));
            }
            let board = any.and_then(|a| a.downcast_mut::<RootPage>()).map(|r| r.board.0).unwrap_or(Window::D7);
            w.push(path(p::tr::board(win(board)), 60.0));
            w.push(path(p::tr::extras(win(board)), 300.0));
            w.push(path(p::tr::teams(), 600.0));
        }
        View::Token { coin, .. } => {
            let t = normalize_type(coin);
            let (tf, holders) = any.and_then(|a| a.downcast_mut::<TokenPage>()).map(|p| (p.st.tf, p.st.tab == super::token::Tab::Holders)).unwrap_or((1, false));
            w.push(path(p::tok::token(&t), 15.0));
            w.push(path(p::tok::lean(SUI_TYPE), 30.0));
            w.push(path(p::tok::candles(&t, TIMEFRAMES[tf], 300), 15.0));
            w.push(path(p::tok::trades(&t), 10.0));
            if holders {
                w.push(path(p::tok::holders(&t, 20), 60.0));
            }
            bal(&mut w);
        }
        View::Perp { market } => {
            let t = any.and_then(|a| a.downcast_mut::<PerpPage>()).map(|p| p.t.clone()).unwrap_or_default();
            w.push(path(p::perp::markets(), 5.0));
            w.push(path(p::LOGOS.to_string(), 600.0));
            w.push((Job::Bars { market: market.clone(), tf: t.tf }, 15.0));
            if let Some(a) = &wallet {
                w.push(path(p::perp::account(a), 10.0));
            }
            w.push((Job::Preview { market: market.clone(), side: t.side, margin: t.margin, lev: t.lev }, 30.0));
            bal(&mut w);
        }
        View::Swap { .. } => {
            w.push(path(p::tok::pick_list(60), 60.0));
            if let Some(a) = &wallet {
                w.push(path(p::tok::portfolio(a), 30.0));
            }
            bal(&mut w);
        }
        View::Trader { address } => {
            let a = address.to_lowercase();
            let win30 = any.and_then(|x| x.downcast_mut::<TraderPage>()).map(|p| p.win30).unwrap_or(true);
            let bw = win(if win30 { Window::D30 } else { Window::D7 });
            w.push(path(p::tr::profile(&a), 300.0));
            w.push(path(p::tr::stats(&a, bw), 60.0));
            w.push(path(p::tr::pnl(&a, bw), 300.0));
            w.push(path(p::tr::trades(&a, 25), 8.0));
            w.push(path(p::tr::linked(&a), 600.0));
            bal(&mut w);
        }
    }
    w
}

/// What a page's data depends on besides the store (its timeframe, ticket, window).
fn state_key(view: &View, page: &mut dyn Page) -> String {
    let Some(a) = page.any_mut() else { return String::new() };
    match view {
        View::Root(_) => a.downcast_mut::<RootPage>().map(|r| format!("{:?}", r.board.0)).unwrap_or_default(),
        View::Token { .. } => a.downcast_mut::<TokenPage>().map(|p| format!("{}", p.st.tf)).unwrap_or_default(),
        View::Perp { .. } => a.downcast_mut::<PerpPage>().map(|p| format!("{}:{:?}:{}:{}", p.t.tf, p.t.side, p.t.margin, p.t.lev)).unwrap_or_default(),
        View::Trader { .. } => a.downcast_mut::<TraderPage>().map(|p| format!("{}", p.win30)).unwrap_or_default(),
        View::Swap { .. } => String::new(),
    }
}

/// The pages' first data, from whatever the store holds.
struct LiveSource(Rc<RefCell<Store>>);

impl Source for LiveSource {
    fn root(&self) -> RootData {
        self.0.borrow().root(Window::D7)
    }
    fn token(&self, coin: &str) -> TokenData {
        self.0.borrow().token(coin, 1)
    }
    fn perp(&self, market: &str) -> PerpData {
        self.0.borrow().perp(market, 1, Some(&Default::default()))
    }
    fn swap(&self) -> SwapData {
        self.0.borrow().swap()
    }
    fn trader(&self, address: &str) -> TraderData {
        self.0.borrow().trader(address, true)
    }
}

// ---------------------------------------------------------------- reviews

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// A router swap from the coin page or the swap page.
    Swap { coin_in: String, coin_out: String, amount_in: u128, title: String },
    Perp { req: rare_trade::perps::PerpTxRequest, title: String },
}

struct ReviewJob {
    key: String,
    kind: Kind,
    phase: Phase,
    tx: Option<UnsignedTx>,
    /// The account the transaction was built for; it must still be the active one when signed.
    owner: Option<String>,
}

enum Msg {
    Answer(String, Result<Value, String>),
    Quote(String, Result<SwapQuote, String>),
    Built(String, Result<(UnsignedTx, Option<u64>), String>),
    Signed(String, SignOutcome),
    Search(String, Vec<TokenRow>),
}

pub struct TradeLive {
    live: bool,
    store: Rc<RefCell<Store>>,
    net: Arc<dyn Net>,
    swap: Option<Arc<Swap>>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Per page (by position and view): the store version and page state its data was built from.
    built: HashMap<String, (u64, String)>,
    /// The quote the ticket on top waits for: (key, due time).
    quote_due: Option<(String, f64)>,
    quote_flight: Option<String>,
    quotes: HashMap<String, SwapQuote>,
    review: Option<ReviewJob>,
    next_acct: f64,
    /// The `keep::epoch` this saw last (a sent transaction, a notice, waking up: balances again).
    epoch: u64,
}

impl TradeLive {
    pub fn new(live: bool) -> TradeLive {
        let (tx, rx) = channel();
        // Live, the store is the app's (`keep::shared`): coming back to Trade draws the last values
        // at once and refreshes them in place. Reads in flight for an earlier visit are asked again.
        let store = if live { crate::data::keep::shared("trade.store", Store::default) } else { Rc::new(RefCell::new(Store::default())) };
        store.borrow_mut().flight.clear();
        let mut t = TradeLive { live, store, net: net(), swap: None, tx, rx, built: HashMap::new(), quote_due: None, quote_flight: None, quotes: HashMap::new(), review: None, next_acct: 0.0, epoch: crate::data::keep::epoch() };
        if live {
            t.refresh_account();
            t.seed();
        }
        t
    }

    fn swap_client(&mut self) -> Arc<Swap> {
        self.swap.get_or_insert_with(|| Arc::new(Swap::new(self.net.clone(), Arc::new(rare_trade::swap::KvKeys(Arc::new(FileKv::open()))), crate::screens::wallet::service::identity_maker()))).clone()
    }

    fn refresh_account(&mut self) {
        let a = crate::sign::account();
        let mut s = self.store.borrow_mut();
        if s.acct != a {
            s.follows = rare_trade::traders::follow::read_follows(a.as_ref().map(|x| x.address.as_str()), &FileKv::open()).into_iter().map(|f| Follow { address: f.address, name: f.name }).collect();
            s.acct = a;
            s.ver += 1;
        }
    }

    /// The front page's reads from the cache, so it draws its first frame with them.
    fn seed(&mut self) {
        use rare_trade::api::TokenList::*;
        let n = crate::screens::wallet::live::market_net();
        let mut s = self.store.borrow_mut();
        let mut paths = vec![p::tok::list(Trending, 16), p::tok::list(Gainers, 6), p::tok::list(New, 6), p::perp::markets(), p::LOGOS.to_string()];
        if let Some(a) = s.wallet().map(str::to_string) {
            paths.push(p::tok::portfolio(&a));
            paths.push(p::perp::account(&a));
        }
        for path in paths {
            if s.map.contains_key(&path) {
                continue;
            }
            if let Some(v) = rare_trade::http::peek::<Value>(n, &path) {
                s.map.insert(path, v);
            }
        }
        // The chain balances from the last run (they are not in the read cache).
        if let Some(a) = s.wallet().map(str::to_string) {
            let k = bal_key(&a);
            if !s.map.contains_key(&k) {
                if let Some(v) = crate::data::keep::load::<Value>(&format!("trade.{k}")) {
                    s.map.insert(k, v);
                }
            }
        }
    }

    /// The pages' data source.
    pub fn source(&self) -> Box<dyn Source> {
        if self.live {
            Box::new(LiveSource(self.store.clone()))
        } else {
            Box::new(Fixtures)
        }
    }

    fn fetch(&mut self, job: Job) {
        let key = job.key();
        let fresh = {
            let mut s = self.store.borrow_mut();
            if !s.flight.insert(key.clone()) {
                return;
            }
            // The cached copy right away (a page seen before draws at once).
            if let Job::Path(path) = &job {
                if !s.map.contains_key(path) {
                    if let Some(v) = rare_trade::http::peek::<Value>(crate::screens::wallet::live::market_net(), path) {
                        s.map.insert(path.clone(), v);
                        s.ver += 1;
                    }
                }
            }
            s.at.contains_key(&key)
        };
        let (tx, n) = (self.tx.clone(), self.net.clone());
        std::thread::Builder::new()
            .name("rare-trade-read".into())
            .spawn(move || {
                let r = job.run(&*n, fresh);
                if let Err(e) = &r {
                    log::warn!("trade read {key}: {e}");
                }
                let _ = tx.send(Msg::Answer(key, r));
            })
            .ok();
    }

    /// Marks reads stale so the next tick asks again (after a trade).
    fn invalidate(&mut self, keys: &[String]) {
        let mut s = self.store.borrow_mut();
        for k in keys {
            s.at.remove(k);
            s.failed.remove(k);
        }
    }

    fn after_trade(&mut self) {
        let Some(a) = self.store.borrow().wallet().map(str::to_string) else { return };
        self.invalidate(&[bal_key(&a), p::tok::portfolio(&a), p::perp::account(&a)]);
    }

    /// A Trade page's act ("quote", "swap:sign", "perp:sign", "tf:1h", ...). False when it is not
    /// Trade's.
    pub fn act(&mut self, tab: &mut TradeTab, cx: &mut Cx, a: &str, now: f64) -> bool {
        let kind = a.split(':').next().unwrap_or(a);
        match a {
            "swap:sign" | "perp:sign" | "perp:sign-close" | "mirror:sign" => {
                self.confirm(tab, cx, now);
                return true;
            }
            "follows" => {
                self.save_follows(tab);
                return true;
            }
            _ if a.starts_with("toast:") => {
                post(AppAct::Toast(a["toast:".len()..].to_string()));
                return true;
            }
            "alert" => {
                match tab.top_coin() {
                    Some((coin_type, symbol, price)) => post(AppAct::PriceAlert { coin_type, symbol, price }),
                    None => post(AppAct::Toast("Price alerts need a price for this coin.".into())),
                }
                return true;
            }
            _ => {}
        }
        // Timeframes, windows and quotes follow the pages' state on the next tick.
        matches!(kind, "quote" | "tf" | "traders" | "trader-window")
    }

    fn save_follows(&mut self, tab: &mut TradeTab) {
        let mut list: Option<Vec<Follow>> = None;
        tab.each_page(|_, top, p| {
            if top {
                if let Some(t) = p.any_mut().and_then(|a| a.downcast_mut::<TraderPage>()) {
                    list = Some(t.data.follows.clone());
                }
            }
        });
        let Some(list) = list else { return };
        let mut s = self.store.borrow_mut();
        let acct = s.wallet().map(str::to_string);
        let now = wall_ms();
        let mut followed: Vec<rare_trade::traders::follow::Followed> = rare_trade::traders::follow::read_follows(acct.as_deref(), &FileKv::open());
        followed.retain(|f| list.iter().any(|x| x.address.eq_ignore_ascii_case(&f.address)));
        for f in &list {
            if !followed.iter().any(|x| x.address.eq_ignore_ascii_case(&f.address)) {
                followed = rare_trade::traders::follow::toggle_follow(&followed, &f.address, &f.name, now);
            }
        }
        rare_trade::traders::follow::write_follows(acct.as_deref(), &followed, &FileKv::open());
        s.follows = list;
        s.ver += 1;
    }

    /// The keyboard's answer for a Trade field.
    pub fn osk(&mut self, _tab: &mut TradeTab, _cx: &mut Cx, key: &str, value: Option<String>, _now: f64) {
        if key != "search-coin" {
            return;
        }
        let Some(q) = value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) else {
            self.store.borrow_mut().search = None;
            self.store.borrow_mut().ver += 1;
            return;
        };
        let (tx, n) = (self.tx.clone(), self.net.clone());
        std::thread::Builder::new()
            .name("rare-trade-search".into())
            .spawn(move || {
                let rows = rare_trade::search::search_coins(&*n, &SuiReader, &q);
                let rows: Vec<TokenRow> = rows.into_iter().filter_map(|r| serde_json::to_value(r).ok().and_then(|v| serde_json::from_value(v).ok())).collect();
                let _ = tx.send(Msg::Search(q, rows));
            })
            .ok();
    }

    // ------------------------------------------------------------ tick

    pub fn tick(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        if !self.live {
            return;
        }
        if now >= self.next_acct {
            self.next_acct = now + 10.0;
            self.refresh_account();
        }
        let ep = crate::data::keep::epoch();
        if ep != self.epoch {
            self.epoch = ep;
            self.after_trade();
        }
        self.drain(tab, cx, now);
        // Reads the pages want: what is missing, and what is stale on the page on top.
        let mut jobs: Vec<Job> = Vec::new();
        {
            let st = self.store.borrow();
            tab.each_page(|view, top, page| {
                for (job, every) in wants(&st, view, page) {
                    let key = job.key();
                    let stale = match st.at.get(&key) {
                        None => !st.failed.contains(&key),
                        Some(t) => top && now - t >= every,
                    };
                    if stale && !st.flight.contains(&key) && !jobs.contains(&job) {
                        jobs.push(job);
                    }
                }
            });
        }
        for j in jobs {
            self.fetch(j);
        }
        self.apply(tab, cx, now);
        self.quotes_tick(tab, cx, now);
        self.review_tick(tab, cx, now);
    }

    /// Hands every page new data when the store or the page's own state moved.
    fn apply(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        let st = self.store.borrow();
        let mut seen = HashSet::new();
        let mut i = 0usize;
        let built = &mut self.built;
        tab.each_page(|view, _top, page| {
            let id = format!("{i}:{view:?}");
            i += 1;
            let sk = state_key(view, page);
            seen.insert(id.clone());
            if built.get(&id) == Some(&(st.ver, sk.clone())) {
                return;
            }
            built.insert(id, (st.ver, sk));
            let Some(a) = page.any_mut() else { return };
            match view {
                View::Root(_) => {
                    if let Some(p) = a.downcast_mut::<RootPage>() {
                        let d = st.root(p.board.0);
                        p.set_data(cx, d, now);
                    }
                }
                View::Token { coin, .. } => {
                    if let Some(p) = a.downcast_mut::<TokenPage>() {
                        let d = st.token(coin, p.st.tf);
                        p.set_data(cx, d, now);
                    }
                }
                View::Perp { market } => {
                    if let Some(p) = a.downcast_mut::<PerpPage>() {
                        let d = st.perp(market, p.t.tf, Some(&p.t));
                        p.set_data(cx, d, now);
                    }
                }
                View::Swap { .. } => {
                    if let Some(p) = a.downcast_mut::<SwapPage>() {
                        p.set_data(cx, st.swap(), now);
                    }
                }
                View::Trader { address } => {
                    if let Some(p) = a.downcast_mut::<TraderPage>() {
                        let d = st.trader(address, p.win30);
                        p.set_data(cx, d, now);
                    }
                }
            }
        });
        built.retain(|k, _| seen.contains(k));
    }

    fn drain(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Answer(key, r) => {
                    let mut s = self.store.borrow_mut();
                    s.flight.remove(&key);
                    s.at.insert(key.clone(), now);
                    match r {
                        Ok(v) => {
                            s.failed.remove(&key);
                            if s.map.get(&key) != Some(&v) {
                                if key.starts_with("bal:") {
                                    crate::data::keep::save(&format!("trade.{key}"), &v);
                                }
                                s.map.insert(key, v);
                                s.ver += 1;
                            }
                        }
                        Err(_) => {
                            if s.failed.insert(key) {
                                s.ver += 1;
                            }
                        }
                    }
                }
                Msg::Search(q, rows) => {
                    let mut s = self.store.borrow_mut();
                    if rows.is_empty() {
                        post(AppAct::Toast(format!("No coins found for {q}")));
                    }
                    s.search = Some((q, rows));
                    s.ver += 1;
                }
                Msg::Quote(key, r) => {
                    self.quote_flight = None;
                    let q = r.as_ref().ok().cloned();
                    if let Some(q) = &q {
                        self.quotes.insert(key.clone(), q.clone());
                    }
                    let err = r.err();
                    tab.each_page(|_, top, page| {
                        if !top {
                            return;
                        }
                        let Some(a) = page.any_mut() else { return };
                        let quote = q.as_ref().map(native_quote);
                        if let Some(p) = a.downcast_mut::<TokenPage>() {
                            if ticket_key_token(p) == Some(key.clone()) {
                                p.st.ticket.quote = quote;
                                p.st.ticket.quoting = false;
                                p.st.ticket.quote_error = err.clone();
                                p.render(cx, now);
                            }
                        } else if let Some(p) = a.downcast_mut::<SwapPage>() {
                            if ticket_key_swap(p) == Some(key.clone()) {
                                p.st.ticket.quote = quote;
                                p.st.ticket.quoting = false;
                                p.st.ticket.quote_error = err.clone();
                                p.render(cx, now);
                            }
                        }
                    });
                }
                Msg::Built(key, r) => {
                    let Some(job) = self.review.as_mut().filter(|j| j.key == key) else { continue };
                    match r {
                        Ok((tx, gas)) => {
                            job.tx = Some(tx);
                            job.phase = Phase::Ready { gas_mist: gas, covered: false };
                        }
                        Err(e) => {
                            log::warn!("trade build: {e}");
                            job.phase = Phase::Failed(format!("Couldn't build this trade. {}", rare_trade::chain::plain_chain_error(&e)));
                        }
                    }
                    self.show_phase(tab, cx, now);
                }
                Msg::Signed(key, out) => {
                    let Some(job) = self.review.as_mut().filter(|j| j.key == key) else { continue };
                    match out {
                        SignOutcome::Sent { dry_run, .. } => {
                            let perp = matches!(job.kind, Kind::Perp { .. });
                            self.review = None;
                            review::set_live(None);
                            close_review(tab, cx, now);
                            post(AppAct::Toast(if dry_run {
                                "Signed as a test and checked against the network. Nothing was sent.".into()
                            } else if perp {
                                "Sent. It shows in Positions once the network has it.".into()
                            } else {
                                "Swap sent. Your balance updates in a moment.".into()
                            }));
                            self.after_trade();
                        }
                        SignOutcome::Rejected => {
                            job.phase = if job.tx.is_some() { Phase::Ready { gas_mist: None, covered: false } } else { Phase::Checking };
                            self.show_phase(tab, cx, now);
                        }
                        SignOutcome::Failed(e) => {
                            job.phase = Phase::Failed(format!("The trade didn't go through: {}", rare_trade::chain::plain_chain_error(&e)));
                            self.show_phase(tab, cx, now);
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ quotes

    fn quotes_tick(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        let owner = self.store.borrow().wallet().map(str::to_string);
        let mut want: Option<(String, QuoteParams)> = None;
        tab.each_page(|_, top, page| {
            if !top {
                return;
            }
            let Some(a) = page.any_mut() else { return };
            if let Some(p) = a.downcast_mut::<TokenPage>() {
                if !p.st.ticket.quoting {
                    return;
                }
                let (from, to) = sides(&p.data, p.st.side);
                let (Some(from), Some(to)) = (from, to) else { return };
                let amount = p.st.ticket.amount_in(Some(&from));
                if amount == 0 {
                    p.st.ticket.quoting = false;
                    p.render(cx, now);
                    return;
                }
                want = ticket_key_token(p).map(|k| (k, QuoteParams { coin_in: from.coin_type, coin_out: to.coin_type, amount_in: amount, owner: owner.clone() }));
            } else if let Some(p) = a.downcast_mut::<SwapPage>() {
                if !p.st.ticket.quoting {
                    return;
                }
                let from = p.data.coin(&p.st.from);
                let amount = p.st.ticket.amount_in(from.as_ref());
                if amount == 0 || p.st.from == p.st.to {
                    p.st.ticket.quoting = false;
                    p.render(cx, now);
                    return;
                }
                want = ticket_key_swap(p).map(|k| (k, QuoteParams { coin_in: p.st.from.clone(), coin_out: p.st.to.clone(), amount_in: amount, owner: owner.clone() }));
            }
        });
        let Some((key, params)) = want else {
            self.quote_due = None;
            return;
        };
        match &self.quote_due {
            Some((k, _)) if *k == key => {}
            _ => self.quote_due = Some((key.clone(), now + 0.3)),
        }
        let due = self.quote_due.as_ref().map(|(_, t)| *t).unwrap_or(now);
        if now < due || self.quote_flight.as_deref() == Some(key.as_str()) {
            return;
        }
        if let Some(q) = self.quotes.get(&key).filter(|q| q.reusable(wall_ms() as u64)).cloned() {
            let _ = self.tx.send(Msg::Quote(key, Ok(q)));
            return;
        }
        self.quote_flight = Some(key.clone());
        let (tx, sw) = (self.tx.clone(), self.swap_client());
        std::thread::Builder::new()
            .name("rare-trade-quote".into())
            .spawn(move || {
                let r = sw.quote(&params).map_err(|e| e.to_string());
                let _ = tx.send(Msg::Quote(key, r));
            })
            .ok();
    }

    // ------------------------------------------------------------ reviews

    /// Shows the review's phase on the sheet and redraws the page on top.
    fn show_phase(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        let signer = self.store.borrow().signer();
        review::set_live(self.review.as_ref().map(|j| (j.phase.clone(), signer)));
        tab.each_page(|_, top, page| {
            if top {
                rerender(page, cx, now);
            }
        });
    }

    fn review_tick(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        let st = self.store.borrow();
        let mut open: Option<(String, Kind)> = None;
        let mut depth = 0usize;
        tab.each_page(|view, top, page| {
            depth += 1;
            if !top {
                return;
            }
            let id = format!("{depth}:{view:?}");
            if let Some(k) = review_kind(&st, page) {
                open = Some((format!("{id}:{k:?}"), k));
            }
        });
        drop(st);
        match (open, self.review.as_ref().map(|j| j.key.clone())) {
            (None, None) => {}
            (None, Some(_)) => {
                self.review = None;
                review::set_live(None);
            }
            (Some((k, _)), Some(cur)) if k == cur => {}
            (Some((key, kind)), _) => {
                self.refresh_account();
                let owner = self.store.borrow().wallet().map(str::to_string);
                self.review = Some(ReviewJob { key: key.clone(), kind: kind.clone(), phase: Phase::Checking, tx: None, owner });
                self.show_phase(tab, cx, now);
                self.build(key, kind);
            }
        }
    }

    fn build(&mut self, key: String, kind: Kind) {
        let Some(owner) = self.store.borrow().wallet().map(str::to_string) else {
            let _ = self.tx.send(Msg::Built(key, Err("Add a wallet first.".into())));
            return;
        };
        let (tx, n, sw) = (self.tx.clone(), self.net.clone(), self.swap_client());
        let cached = match &kind {
            Kind::Swap { coin_in, coin_out, amount_in, .. } => self.quotes.get(&format!("{}>{}:{amount_in}", normalize_type(coin_in), normalize_type(coin_out))).cloned(),
            _ => None,
        };
        std::thread::Builder::new()
            .name("rare-trade-build".into())
            .spawn(move || {
                let r = match kind {
                    Kind::Swap { coin_in, coin_out, amount_in, .. } => (|| {
                        let q = match cached.filter(|q| q.reusable(wall_ms() as u64)) {
                            Some(q) => q,
                            None => sw.quote(&QuoteParams { coin_in, coin_out, amount_in, owner: Some(owner.clone()) }).map_err(|e| e.to_string())?,
                        };
                        let b = sw.build_default(&q, &owner, false).map_err(|e| e.to_string())?;
                        Ok((b.tx, b.gas_estimate_mist.map(|g| g as u64)))
                    })(),
                    Kind::Perp { req, .. } => rare_trade::perps::build_perp_tx(&*n, &owner, &req).map(|t| (t, None)).map_err(|e| e.to_string()),
                };
                let _ = tx.send(Msg::Built(key, r));
            })
            .ok();
    }

    /// Confirm on the open review: Rare Wallet reviews, signs and sends it.
    fn confirm(&mut self, tab: &mut TradeTab, cx: &mut Cx, now: f64) {
        // The active account may have changed since the last poll: read it fresh.
        self.refresh_account();
        let stale = {
            let cur = self.store.borrow().wallet().map(str::to_string);
            self.review.as_ref().is_some_and(|j| owner_changed(j.owner.as_deref(), cur.as_deref()))
        };
        if stale {
            if let Some(job) = self.review.as_mut() {
                job.phase = Phase::Failed("The active account changed. Close this and start the trade again.".into());
            }
            self.show_phase(tab, cx, now);
            return;
        }
        let (addr, signer) = {
            let s = self.store.borrow();
            (s.wallet().map(str::to_string), s.signer())
        };
        let Some(job) = self.review.as_mut() else { return };
        let (Some(addr), true, Some(txn), Phase::Ready { .. }) = (addr, signer, job.tx.clone(), &job.phase) else { return };
        let title = match &job.kind {
            Kind::Swap { title, .. } | Kind::Perp { title, .. } => title.clone(),
        };
        job.phase = Phase::Signing;
        let key = job.key.clone();
        self.show_phase(tab, cx, now);
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name("rare-trade-sign".into())
            .spawn(move || {
                let req = SignRequest { tx: txn, account: addr, title, origin_name: rare_trade::wallet::ORIGIN_NAME, origin_kind: rare_trade::wallet::ORIGIN_KIND, sponsorship: None };
                let out = crate::sign::sign_trade(&req, Some(&SuiReader));
                let _ = tx.send(Msg::Signed(key, out));
            })
            .ok();
    }

    /// Work in flight (keep frames coming).
    pub fn busy(&self) -> bool {
        let s = self.store.borrow();
        !s.flight.is_empty() || self.quote_flight.is_some() || self.quote_due.is_some() || matches!(self.review.as_ref().map(|r| &r.phase), Some(Phase::Checking | Phase::Signing))
    }
}

fn native_quote(q: &SwapQuote) -> Quote {
    Quote { amount_in: q.amount_in, amount_out: q.amount_out, fee_bps: q.fee_bps.round() as u32, hops: q.hops, venues: q.venues.clone(), expires_at: q.expires_at }
}

fn ticket_key_token(p: &TokenPage) -> Option<String> {
    let (from, to) = sides(&p.data, p.st.side);
    let (from, to): (Coin, Coin) = (from?, to?);
    Some(format!("{}>{}:{}", normalize_type(&from.coin_type), normalize_type(&to.coin_type), p.st.ticket.amount_in(Some(&from))))
}

fn ticket_key_swap(p: &SwapPage) -> Option<String> {
    let from = p.data.coin(&p.st.from)?;
    Some(format!("{}>{}:{}", normalize_type(&p.st.from), normalize_type(&p.st.to), p.st.ticket.amount_in(Some(&from))))
}

fn rerender(page: &mut dyn Page, cx: &mut Cx, now: f64) {
    let Some(a) = page.any_mut() else { return };
    if let Some(p) = a.downcast_mut::<TokenPage>() {
        p.render(cx, now);
    } else if let Some(p) = a.downcast_mut::<SwapPage>() {
        p.render(cx, now);
    } else if let Some(p) = a.downcast_mut::<PerpPage>() {
        p.render(cx, now);
    } else if let Some(p) = a.downcast_mut::<RootPage>() {
        p.render(cx, now);
    } else if let Some(p) = a.downcast_mut::<TraderPage>() {
        p.render(cx, now);
    }
}

/// Closes the review sheet on the page on top (after the trade went out).
fn close_review(tab: &mut TradeTab, cx: &mut Cx, now: f64) {
    tab.each_page(|_, top, page| {
        if !top {
            return;
        }
        let Some(a) = page.any_mut() else { return };
        if let Some(p) = a.downcast_mut::<TokenPage>() {
            p.st.reviewing = false;
        } else if let Some(p) = a.downcast_mut::<SwapPage>() {
            p.st.reviewing = false;
        } else if let Some(p) = a.downcast_mut::<PerpPage>() {
            p.t.sheet = None;
        } else if let Some(p) = a.downcast_mut::<RootPage>() {
            p.closing = None;
        } else if let Some(p) = a.downcast_mut::<TraderPage>() {
            p.mirroring = None;
        }
        rerender(page, cx, now);
    });
}

/// The trade the open review sheet on `page` is for, if one is open.
fn review_kind(st: &Store, page: &mut dyn Page) -> Option<Kind> {
    let a = page.any_mut()?;
    if let Some(p) = a.downcast_mut::<TokenPage>() {
        if !p.st.reviewing {
            return None;
        }
        let (from, to) = sides(&p.data, p.st.side);
        let (from, to) = (from?, to?);
        let sym = p.data.token.as_ref().map(|t| t.symbol.clone()).unwrap_or_default();
        return Some(Kind::Swap { amount_in: p.st.ticket.amount_in(Some(&from)), coin_in: from.coin_type, coin_out: to.coin_type, title: format!("{} {sym}", if p.st.side == Side::Buy { "Buy" } else { "Sell" }) });
    }
    if let Some(p) = a.downcast_mut::<SwapPage>() {
        if !p.st.reviewing {
            return None;
        }
        let from = p.data.coin(&p.st.from)?;
        let to = p.data.coin(&p.st.to)?;
        return Some(Kind::Swap { amount_in: p.st.ticket.amount_in(Some(&from)), coin_in: from.coin_type, coin_out: to.coin_type.clone(), title: format!("Swap {} for {}", from.symbol, to.symbol) });
    }
    if let Some(p) = a.downcast_mut::<PerpPage>() {
        let sheet = p.t.sheet?;
        let m = p.data.market.as_ref()?;
        return Some(match sheet {
            Sheet::Close => Kind::Perp { req: rare_trade::perps::PerpTxRequest::Close { market_id: m.market_id.clone(), fraction: None }, title: format!("Close {}", m.symbol) },
            Sheet::Open => {
                let ov = order_view(&p.data, &p.t);
                if ov.plan.needs_account {
                    Kind::Perp { req: rare_trade::perps::PerpTxRequest::CreateAccount, title: "Set up perps".into() }
                } else {
                    Kind::Perp { req: rare_trade::perps::open_request(&m.market_id, side_of(p.t.side), p.t.margin, p.t.lev, ov.plan.short_by), title: format!("{} {} {}x", p.t.side.word(), m.symbol, p.t.lev) }
                }
            }
        });
    }
    if let Some(p) = a.downcast_mut::<RootPage>() {
        let i = p.closing?;
        let pos = p.data.positions().get(i).map(|x| (x.market_id.clone(), x.symbol.clone()))?;
        return Some(Kind::Perp { req: rare_trade::perps::PerpTxRequest::Close { market_id: pos.0, fraction: None }, title: format!("Close {}", pos.1) });
    }
    if let Some(p) = a.downcast_mut::<TraderPage>() {
        let i = p.mirroring?;
        let t = p.data.trades.as_ref()?.items.get(i)?.clone();
        let held = st.wallet().map(|_| st.balances().get(&normalize_type(&t.coin)).copied().unwrap_or(0));
        return match mirror_plan(&t, p.size, 9, held) {
            MirrorPlan::Ok { buy, coin_in, coin_out, amount_in, .. } => Some(Kind::Swap { coin_in, coin_out, amount_in, title: if buy { "Mirror a buy".into() } else { "Mirror a sell".into() } }),
            MirrorPlan::No(_) => None,
        };
    }
    None
}

/// True when a transaction built for `built_for` would not be signed by the active account.
fn owner_changed(built_for: Option<&str>, active: Option<&str>) -> bool {
    match (built_for, active) {
        (Some(a), Some(b)) => !a.eq_ignore_ascii_case(b),
        (None, _) => false,
        (Some(_), None) => true,
    }
}

#[cfg(test)]
mod account_tests {
    use super::owner_changed;
    #[test]
    fn signs_only_for_the_account_it_was_built_for() {
        assert!(!owner_changed(Some("0xAB"), Some("0xab")));
        assert!(owner_changed(Some("0xab"), Some("0xcd")));
        assert!(owner_changed(Some("0xab"), None));
        assert!(!owner_changed(None, Some("0xab")));
    }
}
