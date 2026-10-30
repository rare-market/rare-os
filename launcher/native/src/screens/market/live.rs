//! The Market tab as the app hosts it: `MarketTab` (Collections + Trade) on live data, with every
//! act its pages emit handled here. Marketplace reads and purchases are `feed::Feed`'s, Trade's
//! data and trades `trade::live::TradeLive`'s. What only the shell can do (toasts, the keyboard,
//! the launcher's Search, another tab, a web page) goes out through a small mailbox the app drains
//! each update ([`drain`]); the keyboard's answers come back through `Screen::answer`.

use super::data::{self, Answers};
use super::feed::Feed;
use super::host::*;
use super::shell::Marketplace;
use super::tab::MarketTab;
use crate::screens::trade::live::TradeLive;
use crate::screens::trade::root::Section;
use crate::screens::trade::tab::TradeTab;
use rare_render::focus::hints::Key;
use rare_render::input::Pad;
use rare_render::Id;
use std::cell::RefCell;

/// What the Market tab asks of the shell.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAct {
    Toast(String),
    Error(String),
    OpenWeb(String),
    /// The launcher's Search (Trade's Y).
    Search,
    /// Rare Wallet's tab.
    Wallet,
    /// Rare Wallet's Add account page (Sign in / Add a passkey to buy).
    WalletAdd,
    /// Rare Wallet's item sheet for an item the account holds (list, sell, cancel).
    WalletItem(String),
    /// The price alert sheet for this coin (coin type, symbol, price now in USD).
    PriceAlert { coin_type: String, symbol: String, price: f64 },
    /// The on-screen keyboard (`State::ask_text`); the answer comes back to `Screen::answer` under `key`.
    Osk { key: String, title: String, placeholder: Option<String>, value: String },
}

thread_local! {
    static OUT: RefCell<Vec<AppAct>> = const { RefCell::new(Vec::new()) };
}

/// Hands an act to the shell (UI thread only).
pub fn post(a: AppAct) {
    OUT.with(|o| o.borrow_mut().push(a));
}

thread_local! {
    static OPEN_ON: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Opens the Market tab on a page the next time it ticks: `item:<id>`, `collection:<slug>`,
/// `perp:<market>`, `token:<coin>` or `trader:<address>`.
pub fn open_on(target: String) {
    OPEN_ON.with(|o| *o.borrow_mut() = Some(target));
}

/// What the Market tab asked of the shell since the last call.
pub fn drain() -> Vec<AppAct> {
    OUT.with(|o| std::mem::take(&mut *o.borrow_mut()))
}

/// The hint bar key for a hints.ts name.
pub fn hint_key(name: &str) -> Option<Key> {
    Some(match name {
        "a" => Key::A,
        "b" => Key::B,
        "x" => Key::X,
        "y" => Key::Y,
        "lt" => Key::Lt,
        "rt" => Key::Rt,
        "ltrt" => Key::LtRt,
        "lb" => Key::Lb,
        "rb" => Key::Rb,
        "start" | "menu" => Key::Start,
        "select" | "view" => Key::Select,
        "lc" => Key::Lc,
        "rc" => Key::Rc,
        _ => return None,
    })
}

const MARKET_SEARCH: &str = "market-search";

pub struct LiveMarket {
    pub tab: MarketTab,
    feed: Feed,
    trade: TradeLive,
    cart: super::cart::CartSheet,
}

impl LiveMarket {
    /// Live unless `RR_FIXTURE` is set (golden runs: the captured answers).
    pub fn new(cx: &mut Cx, parent: Id, now: f64) -> LiveMarket {
        let live = std::env::var("RR_FIXTURE").is_err();
        let seed = if live {
            Answers::default()
        } else {
            let mut a = data::home();
            for more in [data::item(), data::search(), data::collection()] {
                a.absorb(more);
            }
            // Shots of the "Could not load" states: RR_MARKET_FAIL=<path>,<path>.
            for p in std::env::var("RR_MARKET_FAIL").unwrap_or_default().split(',').filter(|p| !p.is_empty()) {
                a.fail(p.to_string());
            }
            a
        };
        let mut feed = if live { Feed::kept() } else { Feed::new(seed, live) };
        let trade = TradeLive::new(live);
        let a = feed.answers();
        let src = trade.source();
        let tab = MarketTab::new(cx, parent, rare_market::nav::Area::Collections, |cx, n| Marketplace::new(cx, n, a, vec![], now), |cx, n| TradeTab::new(cx, n, src, Section::Home, now));
        let cart = super::cart::CartSheet::new(cx, parent);
        let mut tab = tab;
        feed.prefill(&mut tab.market, cx, now);
        // Shots: RR_MARKET_OPEN=item:<id> | collection:<slug> opens that page (golden runs only).
        if !live {
            if let Ok(t) = std::env::var("RR_MARKET_OPEN") {
                open_on(t);
            }
        }
        LiveMarket { tab, feed, trade, cart }
    }

    /// Shows the page `open_on` named, over whatever the Market tab was on.
    fn goto(&mut self, cx: &mut Cx, target: &str, now: f64) {
        use rare_market::nav::{Area, View};
        let (kind, rest) = target.split_once(':').unwrap_or((target, ""));
        if rest.is_empty() {
            return;
        }
        match kind {
            "item" | "collection" | "search" => {
                let view = match kind {
                    "item" => View::Item { id: rest.to_string(), card: None, thumb: None },
                    "search" => View::Search { q: rest.to_string() },
                    _ => View::Collection { slug: rest.to_string() },
                };
                self.tab.go(cx, Area::Collections, None, now);
                self.tab.market.push(cx, view, now);
            }
            _ => {
                if let Some(view) = crate::screens::trade::tab::View::from_act(target) {
                    let sec = if matches!(view, crate::screens::trade::tab::View::Trader { .. }) { Section::Traders } else { Section::Home };
                    self.tab.go(cx, Area::Trade, Some(sec), now);
                    self.tab.trade.push_view(cx, view, now, false);
                }
            }
        }
    }

    /// An act a page handed up: handled here, or passed to the shell.
    fn route(&mut self, cx: &mut Cx, a: &str, now: f64) -> Out {
        if let Some(id) = a.strip_prefix("market:buy:") {
            self.feed.buy(id);
            return Out::Moved;
        }
        if let Some(slug) = a.strip_prefix("market:sweep:") {
            let name = self.feed.a.get::<rare_market::api::CollectionDetail>(&rare_market::api::paths::collection(slug)).map(|c| c.r#ref.name).unwrap_or_default();
            self.cart.open_sweep(cx, slug, &name, self.feed.a.account.clone(), now);
            return Out::Moved;
        }
        if a == "market:cart:open" {
            self.cart.open_cart(cx, self.feed.a.account.clone(), now);
            return Out::Moved;
        }
        if let Some(id) = a.strip_prefix("market:own:") {
            post(AppAct::WalletItem(id.to_string()));
            return Out::Moved;
        }
        if let Some(rest) = a.strip_prefix("osk:") {
            let (key, title, placeholder, value) = match rest.split_once('|') {
                // The marketplace's search field ("osk:<title>|<placeholder>").
                Some((t, p)) => (MARKET_SEARCH.to_string(), t.to_string(), Some(p.to_string()), String::new()),
                None if rest == "search-coin" => (format!("trade:{rest}"), "Search coins".to_string(), Some("Name, symbol or coin type".to_string()), String::new()),
                None => (format!("trade:{rest}"), rest.replace('-', " "), None, String::new()),
            };
            post(AppAct::Osk { key, title, placeholder, value });
            return Out::Moved;
        }
        if let Some(url) = a.strip_prefix("open:") {
            post(AppAct::OpenWeb(url.to_string()));
            return Out::Moved;
        }
        match a {
            "wallet:passkey:buy" => {
                post(AppAct::WalletAdd);
                return Out::Moved;
            }
            "tab:wallet" => {
                post(AppAct::Wallet);
                return Out::Moved;
            }
            "search" => {
                post(AppAct::Search);
                return Out::Moved;
            }
            _ => {}
        }
        if self.trade.act(&mut self.tab.trade, cx, a, now) {
            return Out::Moved;
        }
        log::info!("market act not handled: {a}");
        Out::None
    }
}

impl Drop for LiveMarket {
    fn drop(&mut self) {
        // The answers outlive the tab (`keep`).
        std::mem::replace(&mut self.feed, Feed::new(Answers::default(), false)).keep();
    }
}

impl Page for LiveMarket {
    fn prepare_paint(&mut self, scene: &rare_render::Scene, ts: &mut rare_render::TextSys, tex: &mut dyn rare_render::chart::Textures) {
        self.tab.prepare_paint(scene, ts, tex);
    }
    fn paint_slot(&mut self, sp: &rare_render::scene::SlotPaint, ts: &mut rare_render::TextSys, out: &mut rare_render::DrawList) {
        self.tab.paint_slot(sp, ts, out);
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        if self.cart.is_open() {
            return self.cart.hints();
        }
        self.tab.hints()
    }
}

impl Screen for LiveMarket {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        if self.cart.is_open() {
            return self.cart.touch_focus(cx, x, y, now);
        }
        self.tab.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        if self.cart.is_open() {
            return match self.cart.press(cx, pad, now) {
                Out::Act(a) => self.route(cx, &a, now),
                o => o,
            };
        }
        if pad == Pad::Start && self.tab.area == rare_market::nav::Area::Collections && super::cart::count() > 0 {
            self.cart.open_cart(cx, self.feed.a.account.clone(), now);
            return Out::Moved;
        }
        match self.tab.press(cx, pad, now) {
            Out::Act(a) => self.route(cx, &a, now),
            o => o,
        }
    }

    fn answer(&mut self, cx: &mut Cx, key: &str, value: Option<String>, now: f64) -> Out {
        if key == MARKET_SEARCH {
            return self.tab.market.search_answer(cx, value.as_deref(), now);
        }
        if let Some(k) = key.strip_prefix("trade:") {
            self.trade.osk(&mut self.tab.trade, cx, k, value, now);
            return Out::Moved;
        }
        Out::None
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if let Some(t) = OPEN_ON.with(|o| o.borrow_mut().take()) {
            self.goto(cx, &t, now);
        }
        if self.cart.tick(cx, now) {
            // Bought: the pages read their items again.
            self.feed.refresh_now();
        }
        self.feed.tick(&mut self.tab.market, cx, now);
        self.trade.tick(&mut self.tab.trade, cx, now);
        self.tab.tick(cx, now);
    }

    fn next_deadline(&self) -> Option<f64> {
        // Answers arrive on channels: poll while anything is out.
        let poll = (self.feed.busy() || self.trade.busy() || self.cart.working()).then(|| rare_render::clock::secs() + 0.1);
        [poll, self.tab.market.deadline()].into_iter().flatten().reduce(f64::min)
    }
}
