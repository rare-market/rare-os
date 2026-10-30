//! New trades by followed traders. A calm poll (a few traders per tick, paused while a game runs)
//! reads each trader's latest trades; anything newer than what was seen goes out as an event.
//!
//! The web launcher ran this on its own timer; here the host calls [`FollowFeed::tick`] with the
//! time and activity (rare-notify or the launcher shell), and subscribers get each event. Until a
//! notifier takes over (`set_toasts(false)`), [`FollowFeed::emit`] also says to show a toast.

use super::api::{trader_paths, TokenChip, TradesPage, TraderTrade};
use super::follow::Followed;
use crate::http::get;
use crate::num::{num, NumKind};
use crate::timing::{Activity, Cadence};
use rare_market::net::Net;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct FollowedTradeEvent {
    pub trader_address: String,
    pub trader_name: String,
    pub trade: TraderTrade,
    pub token: Option<TokenChip>,
}

/// The event name the web launcher dispatched (kept for hosts bridging to a web view).
pub const FOLLOWED_TRADE_EVENT: &str = "rare:followed-trade";

/// The trades in a page newer than `seen` (the newest time already handled), oldest first. With no
/// `seen` yet nothing is new: the first read only sets the mark. Trades before `since` never count.
pub fn fresh_trades(items: &[TraderTrade], seen: Option<f64>, since: f64) -> Vec<TraderTrade> {
    let Some(seen) = seen else { return Vec::new() };
    let mut out: Vec<TraderTrade> = items.iter().filter(|t| t.time_ms > seen && t.time_ms >= since).cloned().collect();
    out.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
    out
}

/// The newest time in a page, or the old mark when the page is empty.
pub fn newest_time(items: &[TraderTrade], seen: Option<f64>) -> f64 {
    items.iter().fold(seen.unwrap_or(0.0), |m, t| m.max(t.time_ms))
}

/// One plain sentence for a toast or a notification.
pub fn trade_sentence(e: &FollowedTradeEvent) -> String {
    let sym = match &e.token {
        Some(t) => t.symbol.clone(),
        None => e.trade.coin.rsplit("::").next().unwrap_or("a coin").to_string(),
    };
    format!("{} {} {} of {sym}", e.trader_name, if e.trade.buy { "bought" } else { "sold" }, num(NumKind::Money, e.trade.usd))
}

/// Traders read per tick, and the tick. 30 follows are each read about every 50 seconds.
pub const PER_TICK: usize = 6;
/// Every 10 s in use, every minute idle, every 2 minutes with another window in front.
pub const CADENCE: Cadence = Cadence { active: 10_000, idle: Some(60_000), away: Some(120_000) };
/// How many recent followed trades the Traders section lists.
pub const RECENT: usize = 40;

type Listener = Box<dyn Fn(&FollowedTradeEvent) + Send + Sync>;

/// The feed's marks, its turn through the follow list, and the latest events this session.
pub struct FollowFeed {
    seen: HashMap<String, f64>,
    cursor: usize,
    last_tick: Option<u64>,
    /// The latest followed trades this session, newest first (the Following list on Traders).
    pub recent: Vec<FollowedTradeEvent>,
    toasts: bool,
    listeners: Vec<(u64, Listener)>,
    next_id: u64,
}

impl Default for FollowFeed {
    fn default() -> Self {
        FollowFeed { seen: HashMap::new(), cursor: 0, last_tick: None, recent: Vec::new(), toasts: true, listeners: Vec::new(), next_id: 0 }
    }
}

impl FollowFeed {
    pub fn new() -> Self {
        Self::default()
    }

    /// One tick: the next few followed traders in turn.
    pub fn poll_once(&mut self, list: &[Followed], net: &dyn Net) -> Vec<FollowedTradeEvent> {
        if list.is_empty() {
            return Vec::new();
        }
        let n = PER_TICK.min(list.len());
        let batch: Vec<&Followed> = (0..n).map(|i| &list[(self.cursor + i) % list.len()]).collect();
        self.cursor = (self.cursor + n) % list.len();
        let mut out = Vec::new();
        for f in batch {
            let Ok(page) = get::<TradesPage>(net, &trader_paths::trades(&f.address, 10)) else { continue };
            let mark = self.seen.get(&f.address).copied();
            for t in fresh_trades(&page.items, mark, f.since) {
                let token = page.tokens.get(&t.coin).cloned();
                out.push(FollowedTradeEvent { trader_address: f.address.clone(), trader_name: f.name.clone(), trade: t, token });
            }
            self.seen.insert(f.address.clone(), newest_time(&page.items, mark));
        }
        out.sort_by(|a, b| a.trade.time_ms.total_cmp(&b.trade.time_ms));
        out
    }

    /// Runs a poll when one is due (never while a game has the screen) and emits what it found.
    /// Returns the events, oldest first.
    pub fn tick(&mut self, list: &[Followed], net: &dyn Net, now_ms: u64, activity: Activity, game_running: bool) -> Vec<FollowedTradeEvent> {
        if !CADENCE.due(self.last_tick, now_ms, activity, game_running) {
            return Vec::new();
        }
        self.last_tick = Some(now_ms);
        let events = self.poll_once(list, net);
        for e in &events {
            self.emit(e.clone());
        }
        events
    }

    /// Records an event and tells every subscriber. True when the launcher should also show a
    /// toast (no notifier has taken over).
    pub fn emit(&mut self, e: FollowedTradeEvent) -> bool {
        self.recent.retain(|r| r.trade.id != e.trade.id);
        self.recent.insert(0, e.clone());
        self.recent.truncate(RECENT);
        for (_, f) in &self.listeners {
            f(&e);
        }
        self.toasts
    }

    /// Subscribe to followed traders' new trades. Returns the id for [`FollowFeed::off`].
    pub fn on_followed_trade(&mut self, f: impl Fn(&FollowedTradeEvent) + Send + Sync + 'static) -> u64 {
        self.next_id += 1;
        self.listeners.push((self.next_id, Box::new(f)));
        self.next_id
    }

    pub fn off(&mut self, id: u64) {
        self.listeners.retain(|(i, _)| *i != id);
    }

    /// A notifier that shows these itself turns the fallback toasts off.
    pub fn set_toasts(&mut self, on: bool) {
        self.toasts = on;
    }

    /// Forgets the marks.
    pub fn reset(&mut self) {
        self.seen.clear();
        self.cursor = 0;
    }
}

#[cfg(test)]
mod tests {
    //! feed.test.ts
    use super::*;
    use rare_market::net::{Method, MockNet};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn a() -> String {
        format!("0x{}", "a".repeat(64))
    }
    fn t(id: &str, time_ms: f64, buy: bool) -> TraderTrade {
        TraderTrade { id: id.into(), digest: id.into(), time_ms, buy, usd: 120.0, amount: 10.0, price_usd: 12.0, trader: a(), coin: "0x2::sui::SUI".into() }
    }
    fn ids(v: &[TraderTrade]) -> Vec<&str> {
        v.iter().map(|x| x.id.as_str()).collect()
    }

    #[test]
    fn first_read_only_sets_the_mark() {
        assert!(fresh_trades(&[t("a", 5.0, true)], None, 0.0).is_empty());
        assert_eq!(newest_time(&[t("a", 5.0, true), t("b", 9.0, true)], None), 9.0);
    }

    #[test]
    fn returns_newer_trades_oldest_first_never_before_the_follow() {
        let items = [t("c", 30.0, true), t("b", 20.0, true), t("a", 10.0, true)];
        assert_eq!(ids(&fresh_trades(&items, Some(10.0), 0.0)), ["b", "c"]);
        assert_eq!(ids(&fresh_trades(&items, Some(10.0), 25.0)), ["c"]);
        assert_eq!(newest_time(&[], Some(42.0)), 42.0);
    }

    fn page(items: Vec<TraderTrade>, tokens: serde_json::Value) -> String {
        json!({ "items": items.iter().map(|x| json!({ "id": x.id, "digest": x.digest, "timeMs": x.time_ms, "buy": x.buy, "usd": x.usd, "amount": x.amount, "priceUsd": x.price_usd, "trader": x.trader, "coin": x.coin })).collect::<Vec<_>>(), "nextBefore": null, "tokens": tokens }).to_string()
    }

    #[test]
    fn reports_a_followed_traders_new_trades_on_the_next_read() {
        let n = MockNet::new();
        let url = format!("https://rare.market/v1/trade/users/{}/trades?limit=10", a());
        n.on_call(Method::Get, &url, 200, page(vec![t("a", 10.0, true)], json!({})));
        n.on_call(Method::Get, &url, 200, page(vec![t("b", 20.0, false), t("a", 10.0, true)], json!({ "0x2::sui::SUI": { "coinType": "0x2::sui::SUI", "symbol": "SUI", "name": "Sui", "logo": null, "priceUsd": 1 } })));
        let list = [Followed { address: a(), name: "@whale".into(), since: 0.0 }];
        let mut feed = FollowFeed::new();
        assert!(feed.poll_once(&list, &n).is_empty());
        let out = feed.poll_once(&list, &n);
        assert_eq!(out.iter().map(|e| (e.trade.id.as_str(), e.token.as_ref().map(|t| t.symbol.as_str()), e.trader_name.as_str())).collect::<Vec<_>>(), [("b", Some("SUI"), "@whale")]);
        assert_eq!(trade_sentence(&out[0]), "@whale sold $120.00 of SUI");
        assert!(feed.poll_once(&list, &n).is_empty());
        assert!(n.calls().iter().all(|c| c.url == url));
    }

    #[test]
    fn a_failing_read_is_skipped() {
        let n = MockNet::new();
        let mut feed = FollowFeed::new();
        assert!(feed.poll_once(&[Followed { address: a(), name: "x".into(), since: 0.0 }], &n).is_empty());
    }

    #[test]
    fn subscribers_get_every_followed_trade_and_can_unsubscribe() {
        let mut feed = FollowFeed::new();
        feed.set_toasts(false);
        let got = Arc::new(Mutex::new(Vec::new()));
        let g = got.clone();
        let off = feed.on_followed_trade(move |e| g.lock().unwrap().push(e.trade.id.clone()));
        let ev = |id: &str, at: f64| FollowedTradeEvent { trader_address: a(), trader_name: "x".into(), trade: t(id, at, true), token: None };
        assert!(!feed.emit(ev("z", 1.0)));
        feed.off(off);
        feed.emit(ev("y", 2.0));
        assert_eq!(*got.lock().unwrap(), ["z"]);
        assert_eq!(feed.recent.iter().map(|e| e.trade.id.as_str()).collect::<Vec<_>>(), ["y", "z"]);
    }

    #[test]
    fn ticks_on_the_cadence_and_never_over_a_game() {
        let n = MockNet::new();
        let mut feed = FollowFeed::new();
        let list = [Followed { address: a(), name: "x".into(), since: 0.0 }];
        feed.tick(&list, &n, 0, Activity::Active, true);
        assert!(n.calls().is_empty());
        feed.tick(&list, &n, 0, Activity::Active, false);
        feed.tick(&list, &n, 5_000, Activity::Active, false);
        assert_eq!(n.calls().len(), 1);
        feed.tick(&list, &n, 10_000, Activity::Active, false);
        assert_eq!(n.calls().len(), 2);
    }
}
