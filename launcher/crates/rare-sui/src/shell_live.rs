//! Live Sui data for the shell: SUI price and its 24 h line, network status, the account's
//! portfolio, top collections and the latest sales, plus game token prices. Ports
//! `src/lib/live.ts`.
//!
//! Sources: the Trade API's public extension API for prices, candles and portfolios; rare.market's /v1
//! API for status, collections and events. The host ticks [`ShellLive::tick`] every
//! [`ACTIVE`] in use and every [`IDLE`] idle, never while a game or another window is in front,
//! and calls [`ShellLive::resume`] after sleep. What is worth keeping between starts is
//! [`Kept`] (rare-state key [`KEPT_KEY`]), so the first frame shows the last price and line.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::market::{self, CollectionRow, MarketApi, MarketEvent, Page};
use crate::prices::{LinePoint, PriceClient, SUI_PERP};

pub const ACTIVE: Duration = Duration::from_secs(20);
pub const IDLE: Duration = Duration::from_secs(120);
/// rare-state key of [`Kept`] (`ui:live` in the web launcher).
pub const KEPT_KEY: &str = "ui:live";
const SUI: &str = "0x2::sui::SUI";
const TOKEN_EVERY: f64 = 120_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellPortfolio {
    pub address: String,
    pub total_usd: f64,
    pub change24h_usd: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPrice {
    pub price_usd: Option<f64>,
    pub change24h: Option<f64>,
}

/// What is kept between starts (everything but `fresh`, `loading` and `degraded`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Kept {
    pub sui_usd: Option<f64>,
    pub sui_change24h: Option<f64>,
    pub sui_volume24h: Option<f64>,
    pub sui_line: Vec<LinePoint>,
    pub tps: Option<f64>,
    pub checkpoint: Option<f64>,
    pub portfolio: Option<ShellPortfolio>,
    pub collections: Option<Vec<CollectionRow>>,
    pub sales: Option<Vec<MarketEvent>>,
    /// Game tokens by coin type (Sui Games cards and pages).
    pub tokens: HashMap<String, TokenPrice>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub kept: Kept,
    pub degraded: bool,
    /// Ids of sales that arrived since the last poll, for the one-time tint.
    pub fresh: Vec<String>,
    pub loading: bool,
}

/// What is due when (the tick itself runs every 20 s in use, every 2 minutes idle).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Schedule {
    status: f64,
    minute: f64,
    line: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Status,
    Minute,
    Line,
}

impl Schedule {
    fn every(j: Job) -> f64 {
        match j {
            Job::Status | Job::Minute => 60_000.0,
            Job::Line => 5.0 * 60_000.0,
        }
    }
    /// Whether `j` is due at `now` (with 2 s of slack so a 20 s tick doesn't slip a period),
    /// marking it run when it is.
    pub fn due(&mut self, j: Job, now: f64, all: bool) -> bool {
        let last = match j {
            Job::Status => &mut self.status,
            Job::Minute => &mut self.minute,
            Job::Line => &mut self.line,
        };
        if !all && now - *last < Self::every(j) - 2_000.0 {
            return false;
        }
        *last = now;
        true
    }
}

/// Zero-price transfers between a game's own kiosks are not market sales: keep priced ones, 8.
pub fn real_sales(list: Vec<MarketEvent>) -> Vec<MarketEvent> {
    list.into_iter().filter(|e| e.price_mist.as_deref().and_then(|p| p.parse::<u128>().ok()).is_some_and(|p| p > 0)).take(8).collect()
}

/// Sales new since the previous list (none on the first load).
pub fn fresh_ids(prev: Option<&[MarketEvent]>, next: &[MarketEvent]) -> Vec<String> {
    let Some(prev) = prev else { return vec![] };
    let seen: HashSet<&str> = prev.iter().map(|e| e.id.as_str()).collect();
    next.iter().filter(|e| !seen.contains(e.id.as_str())).map(|e| e.id.clone()).collect()
}

#[derive(Default)]
struct Ctl {
    schedule: Schedule,
    paused: bool,
    /// After a resume: refreshes at 3 s, 8 s and 20 s (each skipped once one has worked).
    resume: Option<Resume>,
    account: Option<String>,
    token_asked: HashMap<String, f64>,
}

#[derive(Clone, Copy, Debug)]
struct Resume {
    at: f64,
    done: [bool; 3],
    ok: bool,
    /// Plain ticks wait until the first refresh.
    holding: bool,
}

const RESUME_AT: [f64; 3] = [3_000.0, 8_000.0, 20_000.0];

/// The shell's live store (`useLive`).
pub struct ShellLive {
    state: Mutex<Live>,
    ctl: Mutex<Ctl>,
    version: AtomicU64,
    trade: PriceClient,
}

impl Default for ShellLive {
    fn default() -> Self {
        ShellLive::new(PriceClient::default())
    }
}

impl ShellLive {
    pub fn new(trade: PriceClient) -> Self {
        ShellLive { state: Mutex::new(Live::default()), ctl: Mutex::new(Ctl::default()), version: AtomicU64::new(0), trade }
    }

    pub fn snapshot(&self) -> Live {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn trade(&self) -> &PriceClient {
        &self.trade
    }

    fn set(&self, f: impl FnOnce(&mut Live)) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let before = s.clone();
        f(&mut s);
        if *s != before {
            self.version.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn ctl<R>(&self, f: impl FnOnce(&mut Ctl) -> R) -> R {
        f(&mut self.ctl.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The first frame: what was kept last time, unless something newer is already in.
    pub fn seed(&self, kept: Kept) {
        self.set(|s| {
            if s.kept.sui_usd.is_none() {
                s.kept = kept;
            }
        });
    }

    /// What to persist under [`KEPT_KEY`].
    pub fn kept(&self) -> Kept {
        self.snapshot().kept
    }

    async fn sui(&self) {
        if let Some(t) = self.trade.token_detail(SUI).await {
            self.set(|s| {
                s.kept.sui_usd = t.meta.price_usd;
                s.kept.sui_change24h = t.meta.change24h;
                s.kept.sui_volume24h = t.volume24h;
            });
        }
    }

    async fn sui_line(&self, now: f64) {
        if let Ok(line) = self.trade.candles(SUI_PERP, 60, now - 24.0 * 3_600_000.0, now).await {
            if line.len() > 1 {
                self.set(|s| s.kept.sui_line = line);
            }
        }
    }

    async fn status(&self, api: &dyn MarketApi) -> bool {
        match market::status(api).await {
            Ok(st) => {
                self.set(|s| {
                    s.kept.tps = Some(st.tps);
                    s.kept.checkpoint = Some(st.checkpoint);
                    s.degraded = st.degraded;
                    s.kept.sui_usd = s.kept.sui_usd.or(Some(st.sui_usd));
                });
                true
            }
            Err(_) => false,
        }
    }

    async fn portfolio(&self, address: Option<String>) {
        let Some(address) = address else {
            self.set(|s| s.kept.portfolio = None);
            return;
        };
        if let Ok(p) = self.trade.portfolio(&address).await {
            self.set(|s| s.kept.portfolio = Some(ShellPortfolio { address, total_usd: p.total_usd, change24h_usd: p.change24h_usd }));
        }
    }

    async fn collections(&self, api: &dyn MarketApi) {
        if let Ok(r) = market::get_as::<Page<CollectionRow>>(api, "/v1/collections?window=24h&sort=volume&limit=6&verified=true").await {
            self.set(|s| s.kept.collections = Some(r.data));
        }
    }

    async fn sales(&self, api: &dyn MarketApi) {
        if let Ok(r) = market::get_as::<Page<MarketEvent>>(api, "/v1/events?limit=30&kind=sale").await {
            let list = real_sales(r.data);
            self.set(|s| {
                s.fresh = fresh_ids(s.kept.sales.as_deref(), &list);
                s.kept.sales = Some(list);
            });
        }
    }

    /// The account whose portfolio shows on Home and in the top bar (watch or signed in).
    pub async fn set_account(&self, address: Option<&str>) {
        let changed = self.ctl(|c| {
            if c.account.as_deref() == address {
                return false;
            }
            c.account = address.map(str::to_string);
            true
        });
        if changed {
            self.portfolio(address.map(str::to_string)).await;
        }
    }

    pub fn account_of_portfolio(&self, address: &str) -> Option<f64> {
        self.snapshot().kept.portfolio.filter(|p| p.address.eq_ignore_ascii_case(address)).map(|p| p.total_usd)
    }

    /// Pause while a game has the screen; un-pausing refreshes in full at once.
    pub async fn pause(&self, api: &dyn MarketApi, on: bool, now: f64) {
        let was = self.ctl(|c| std::mem::replace(&mut c.paused, on));
        if was && !on {
            self.tick(api, true, now).await;
        }
    }

    /// After a resume Wi-Fi needs a few seconds: keep what is on screen, skip the polls that
    /// would only fail, and refresh in full at 3 s, 8 s and 20 s (each skipped once one worked).
    /// The host then calls [`ShellLive::resume_due`] from its timer.
    pub fn resume(&self, now: f64) {
        self.ctl(|c| c.resume = Some(Resume { at: now, done: [false; 3], ok: false, holding: true }));
    }

    /// Runs a pending post-resume refresh if one is due; ends the hold after 20.5 s.
    pub async fn resume_due(&self, api: &dyn MarketApi, now: f64) {
        let run = self.ctl(|c| {
            let Some(r) = c.resume.as_mut() else { return false };
            let mut run = false;
            for (i, at) in RESUME_AT.iter().enumerate() {
                if !r.done[i] && now - r.at >= *at {
                    r.done[i] = true;
                    if !r.ok && !c.paused {
                        run = true;
                        r.holding = false;
                    }
                }
            }
            if now - r.at >= 20_500.0 {
                c.resume = None;
            }
            run
        });
        if run {
            self.tick_inner(api, true, now).await;
            let s = self.snapshot();
            let ok = s.kept.tps.is_some() && !s.degraded;
            self.ctl(|c| {
                if let Some(r) = c.resume.as_mut() {
                    r.ok = ok;
                }
            });
        }
    }

    fn held(&self) -> bool {
        self.ctl(|c| c.resume.is_some_and(|r| r.holding))
    }

    /// One poll: price and sales each tick; status, portfolio and collections every minute; the
    /// line every 5 minutes. Nothing while paused; only `all` while held after a resume.
    pub async fn tick(&self, api: &dyn MarketApi, all: bool, now: f64) {
        if self.held() && !all {
            return;
        }
        self.tick_inner(api, all, now).await;
    }

    async fn tick_inner(&self, api: &dyn MarketApi, all: bool, now: f64) {
        let (paused, status, minute, line, account) = self.ctl(|c| (c.paused, c.schedule.due(Job::Status, now, all), c.schedule.due(Job::Minute, now, all), c.schedule.due(Job::Line, now, all), c.account.clone()));
        if paused {
            return;
        }
        let first = self.snapshot().kept.sui_usd.is_none() && all;
        if first {
            self.set(|s| s.loading = true);
        }
        let status_job = async {
            if status {
                self.status(api).await;
            }
        };
        let minute_job = async {
            if minute {
                futures::join!(self.portfolio(account), self.collections(api));
            }
        };
        let line_job = async {
            if line {
                self.sui_line(now).await;
            }
        };
        futures::join!(self.sui(), self.sales(api), status_job, minute_job, line_job);
        if first {
            self.set(|s| s.loading = false);
        }
    }

    /// Prices for these coin types (game tokens), fetched at most every 2 minutes each.
    pub async fn load_tokens(&self, types: &[String], now: f64) {
        let ask: Vec<String> = self.ctl(|c| {
            types
                .iter()
                .filter(|t| {
                    if now - c.token_asked.get(*t).copied().unwrap_or(f64::NEG_INFINITY) < TOKEN_EVERY {
                        return false;
                    }
                    c.token_asked.insert((*t).clone(), now);
                    true
                })
                .cloned()
                .collect()
        });
        let got = futures::future::join_all(ask.iter().map(|t| async move { (t.clone(), self.trade.token_detail(t).await) })).await;
        for (t, d) in got {
            if let Some(d) = d {
                self.set(|s| {
                    s.kept.tokens.insert(t, TokenPrice { price_usd: d.meta.price_usd, change24h: d.meta.change24h });
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prices::HttpGet;
    use crate::{BoxFut, Error};
    use serde_json::{json, Value};
    use std::sync::Arc;

    struct FakeTradeApi(Mutex<Vec<String>>);
    impl HttpGet for FakeTradeApi {
        fn get_json(&self, url: &str) -> BoxFut<'_, Value> {
            self.0.lock().unwrap().push(url.to_string());
            let v = if url.contains("/ext/token/") {
                json!({ "priceUsd": 3.5, "change24h": 1.5, "volume24h": 1000 })
            } else if url.contains("/perps/candles") {
                json!({ "bars": [{ "time": 1, "close": 3.4 }, { "time": 2, "close": 3.5 }] })
            } else {
                json!({ "totalUsd": 12, "change24hUsd": 1 })
            };
            Box::pin(async move { Ok(v) })
        }
    }
    struct Api {
        sales: Mutex<Value>,
        calls: Mutex<Vec<String>>,
        down: bool,
    }
    impl MarketApi for Api {
        fn get(&self, p: &str) -> BoxFut<'_, Value> {
            self.calls.lock().unwrap().push(p.to_string());
            let v = if self.down {
                Err(Error::http(None, "offline"))
            } else if p == "/v1/status" {
                Ok(json!({ "suiUsd": 9, "tps": 200, "checkpoint": 5, "degraded": false }))
            } else if p.starts_with("/v1/events") {
                Ok(self.sales.lock().unwrap().clone())
            } else {
                Ok(json!({ "data": [{ "slug": "c", "name": "C", "volumeMist": "1" }], "nextCursor": null }))
            };
            Box::pin(async move { v })
        }
    }
    fn sale(id: &str, price: Option<&str>) -> Value {
        json!({ "id": id, "kind": "sale", "priceMist": price, "at": "" })
    }
    fn live() -> (ShellLive, Arc<FakeTradeApi>) {
        let y = Arc::new(FakeTradeApi(Mutex::new(vec![])));
        (ShellLive::new(PriceClient::new("https://y.test", y.clone())), y)
    }
    fn api() -> Api {
        Api { sales: Mutex::new(json!({ "data": [sale("s1", Some("5")), sale("s0", Some("0")), sale("s2", None)] })), calls: Mutex::new(vec![]), down: false }
    }

    #[tokio::test]
    async fn first_tick_loads_everything_and_later_ticks_follow_the_schedule() {
        let (l, y) = live();
        let a = api();
        l.set_account(Some("0xabc")).await;
        l.tick(&a, true, 1_000_000.0).await;
        let s = l.snapshot();
        assert_eq!(s.kept.sui_usd, Some(3.5), "trade's price wins over the status price");
        assert_eq!(s.kept.sui_change24h, Some(1.5));
        assert_eq!(s.kept.tps, Some(200.0));
        assert_eq!(s.kept.sui_line.len(), 2);
        assert_eq!(s.kept.portfolio.as_ref().unwrap().total_usd, 12.0);
        assert_eq!(s.kept.collections.as_ref().unwrap().len(), 1);
        assert_eq!(s.kept.sales.as_ref().unwrap().iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["s1"]);
        assert!(s.fresh.is_empty(), "nothing is fresh on the first load");
        assert!(!s.loading);
        assert_eq!(l.account_of_portfolio("0xABC"), Some(12.0));

        // 20 s later: price and sales only
        a.calls.lock().unwrap().clear();
        let before = y.0.lock().unwrap().len();
        *a.sales.lock().unwrap() = json!({ "data": [sale("s3", Some("7")), sale("s1", Some("5"))] });
        l.tick(&a, false, 1_020_000.0).await;
        assert_eq!(*a.calls.lock().unwrap(), ["/v1/events?limit=30&kind=sale"]);
        assert_eq!(y.0.lock().unwrap().len() - before, 1);
        assert_eq!(l.snapshot().fresh, ["s3"]);
        // a minute (less 2 s of slack) later: status and collections too
        a.calls.lock().unwrap().clear();
        l.tick(&a, false, 1_058_500.0).await;
        assert_eq!(a.calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn paused_does_nothing_and_unpausing_refreshes() {
        let (l, _) = live();
        let a = api();
        l.pause(&a, true, 0.0).await;
        l.tick(&a, true, 1.0).await;
        assert!(a.calls.lock().unwrap().is_empty());
        l.pause(&a, false, 2.0).await;
        assert!(!a.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn resume_holds_then_refreshes_until_one_works() {
        let (l, _) = live();
        let a = Api { down: true, ..api() };
        l.resume(0.0);
        l.tick(&a, false, 1_000.0).await;
        assert!(a.calls.lock().unwrap().is_empty(), "held: plain ticks skip");
        l.resume_due(&a, 3_000.0).await;
        let n = a.calls.lock().unwrap().len();
        assert!(n > 0, "the 3 s refresh ran");
        // it failed (offline), so the 8 s one runs too
        l.resume_due(&a, 8_000.0).await;
        assert!(a.calls.lock().unwrap().len() > n);
        // back online: the 20 s one works, then the hold ends
        let ok = api();
        l.resume_due(&ok, 20_000.0).await;
        assert_eq!(l.snapshot().kept.tps, Some(200.0));
        assert!(!l.held(), "plain ticks run again after the first refresh");
        l.resume_due(&ok, 20_600.0).await;
        assert!(l.ctl(|c| c.resume.is_none()));
    }

    #[tokio::test]
    async fn token_prices_at_most_every_two_minutes() {
        let (l, y) = live();
        let t = vec!["0xab::g::G".to_string()];
        l.load_tokens(&t, 0.0).await;
        l.load_tokens(&t, 60_000.0).await;
        assert_eq!(y.0.lock().unwrap().len(), 1);
        l.load_tokens(&t, 121_000.0).await;
        assert_eq!(y.0.lock().unwrap().len(), 2);
        assert_eq!(l.snapshot().kept.tokens["0xab::g::G"].price_usd, Some(3.5));
    }

    #[test]
    fn kept_round_trips_and_seeds() {
        let (l, _) = live();
        let k = Kept { sui_usd: Some(1.0), sui_line: vec![LinePoint { t: 1.0, v: 2.0 }], ..Default::default() };
        let j = serde_json::to_string(&k).unwrap();
        assert!(j.contains("\"suiUsd\":1.0") || j.contains("\"suiUsd\":1"));
        l.seed(serde_json::from_str(&j).unwrap());
        assert_eq!(l.kept(), k);
        l.seed(Kept::default());
        assert_eq!(l.kept().sui_usd, Some(1.0), "a seed never overwrites live data");
    }
}
