//! A coin's page in the Trade API's layout (trade/TokenPage.tsx, TradePanel.tsx): header with price and
//! change, the chart with its time ranges, the stat line, recent trades and holders (LT/RT), and
//! the Buy/Sell panel on the right. LT/RT step the amount while the panel has focus, X flips Buy
//! and Sell, A on the panel's button opens the review.

use super::chart_area::{self, ChartArea, ChartOut};
use super::fmt::*;
use super::model::*;
use super::review::{review_sheet, Review};
use super::ticket::*;
use super::ui::{action, chip, coin_logo, line_el, side_switch, stepper, tf_chips, Action, Tone, Val};
use crate::kit::format::{num_parts, num_text, usd_price, NumKind, NumOptions};
use crate::kit::{button, cell, glyph, head_row, num, panel, row, tabs, Btn, GSize, Pb, TabItem};
use crate::screens::market::host::*;
use rare_render::chart::{Candle, Textures};
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::{DrawList, Id, Scene, TextSys};
use std::collections::HashMap;

pub const TIMEFRAMES: [&str; 4] = ["1m", "5m", "1h", "1d"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Trades,
    Holders,
}

/// Everything the page reads; None while loading.
#[derive(Clone, Debug, Default)]
pub struct TokenData {
    pub now_ms: f64,
    pub coin_type: String,
    pub token: Option<TokenRow>,
    pub token_error: Option<String>,
    pub sui: Option<TokenRow>,
    pub candles: Vec<CandleRow>,
    pub trades: Vec<TradeRow>,
    pub trades_error: bool,
    pub holders: Option<HoldersPage>,
    pub holders_error: bool,
    pub wallet: Option<String>,
    /// Balances by normalised coin type, base units.
    pub balances: HashMap<String, u128>,
    /// The live quote for the current ticket (the app asks 300 ms after the amount settles).
    pub quote: Option<Quote>,
}

impl TokenData {
    pub fn from_fixture(f: &TokenFixture, wallet: Option<&str>) -> TokenData {
        TokenData {
            now_ms: f.now,
            coin_type: normalize_type(&f.coin_type),
            token: Some(f.token.clone()),
            token_error: None,
            sui: Some(f.sui.clone()),
            candles: f.candles.clone(),
            trades: f.trades.items.clone(),
            trades_error: false,
            holders: f.holders.clone(),
            holders_error: false,
            wallet: wallet.map(str::to_string),
            balances: if wallet.is_some() { f.balances.iter().map(|(k, v)| (normalize_type(k), v.parse().unwrap_or(0))).collect() } else { HashMap::new() },
            quote: f.quote.as_ref().map(|q| Quote {
                amount_in: q.amount_in.parse().unwrap_or(0),
                amount_out: q.amount_out.parse().unwrap_or(0),
                fee_bps: q.fee_bps,
                hops: q.hops,
                venues: q.venues.clone(),
                expires_at: q.expires_at,
            }),
        }
    }

    pub fn coin(&self) -> Option<Coin> {
        let t = self.token.as_ref()?;
        Some(Coin { coin_type: self.coin_type.clone(), symbol: t.symbol.clone(), decimals: t.decimals, logo: t.logo.clone(), price_usd: t.price_usd })
    }

    pub fn sui_coin(&self) -> Coin {
        Coin::sui(self.sui.as_ref().and_then(|s| s.price_usd), self.sui.as_ref().and_then(|s| s.logo.clone()))
    }
}

pub struct TokenState {
    pub side: Side,
    pub tf: usize,
    pub tab: Tab,
    pub reviewing: bool,
    pub ticket: Ticket,
}

// Focus tags.
const T_ALERT: u32 = 90;
const T_TF: u32 = 100;
pub const T_CHART: u32 = 150;
const T_SIDE: u32 = 200;
const T_PRESET: u32 = 300;
const T_MAX: u32 = 350;
const T_MINUS: u32 = 400;
const T_PLUS: u32 = 401;
const T_REVIEW: u32 = 500;
const T_ROW: u32 = 1000;

/// What the order panel shows and allows (TradePanel's derived values).
#[derive(Clone, Debug, PartialEq)]
pub struct PanelView {
    pub amount_label: String,
    pub under: String,
    pub out: Option<f64>,
    pub get: String,
    pub balance: String,
    pub blocked: bool,
    pub why: Option<String>,
    pub button: String,
}

/// TradePanel's `amountLabel`: "5 SUI" to buy, "50%" or "All" of the coin to sell.
pub fn amount_label(value: f64, side: Side, from: Option<&Coin>) -> String {
    let Some(f) = from else { return String::new() };
    match side {
        Side::Sell => {
            if value >= 100.0 {
                "All".into()
            } else {
                num_text(NumKind::Share, Some(value), NumOptions { decimals: Some(0), ..Default::default() })
            }
        }
        Side::Buy => format!("{} {}", trim6(value), f.symbol),
    }
}

/// `Number(v.toPrecision(6)).toLocaleString("en-US", { maximumFractionDigits: 6 })`.
pub fn trim6(v: f64) -> String {
    grouped(precision_num(v, 6), 0, 6)
}

pub fn panel_view(d: &TokenData, st: &TokenState) -> PanelView {
    let (from, to) = sides(d, st.side);
    let t = &st.ticket;
    let fr = from.as_ref();
    let amount_in = t.amount_in(fr);
    let coin_in = fr.map(|c| base_to_number(amount_in, c.decimals)).unwrap_or(0.0);
    let usd_in = fr.and_then(|c| c.price_usd.filter(|p| *p != 0.0)).map(|p| coin_in * p);
    let out = match (&t.quote, &to) {
        (Some(q), Some(c)) => Some(base_to_number(q.amount_out, c.decimals)),
        _ => None,
    };
    let nothing = st.side == Side::Sell && t.balance == Some(0);
    let blocked = from.is_none() || to.is_none() || amount_in == 0 || t.short(fr) || nothing || t.quote.is_none();
    let why = if d.wallet.is_none() {
        Some("Add a wallet first".to_string())
    } else if nothing {
        Some(format!("No {} to sell", fr.map(|c| c.symbol.as_str()).unwrap_or("")))
    } else if t.short(fr) {
        Some("Not enough balance".into())
    } else {
        t.quote_error.clone()
    };
    let mut under = String::new();
    if st.side == Side::Sell {
        if let Some(c) = fr {
            under = if amount_in > 0 { format!("{} {}", num_text(NumKind::Amount, Some(coin_in), NumOptions::default()), c.symbol) } else { format!("of your {}", c.symbol) };
        }
        if usd_in.is_some() && amount_in > 0 {
            under.push_str(", ");
        }
    }
    if let (Some(u), true) = (usd_in, amount_in > 0) {
        under.push_str(&format!("about {}", num_text(NumKind::Money, Some(u), NumOptions::default())));
    }
    let get = match (out, &to) {
        (Some(o), Some(c)) => format!("{} {}", num_text(NumKind::Amount, Some(o), NumOptions::default()), c.symbol),
        _ if t.quoting => "Getting a quote".into(),
        _ if t.quote_error.is_some() => "No route for this amount".into(),
        _ => String::new(),
    };
    let balance = match (fr, t.balance) {
        (Some(c), Some(b)) => format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(b, c.decimals)), NumOptions::default()), c.symbol),
        _ if d.wallet.is_some() => String::new(),
        _ => "No wallet".into(),
    };
    let button = match st.side {
        Side::Buy => format!("Review buy{}", to.as_ref().map(|c| format!(" {}", c.symbol)).unwrap_or_default()),
        Side::Sell => format!("Review sell{}", from.as_ref().map(|c| format!(" {}", c.symbol)).unwrap_or_default()),
    };
    PanelView { amount_label: amount_label(t.value, st.side, fr), under, out, get, balance, blocked, why, button }
}

/// (from, to) for a side: buying pays SUI for the coin, selling the reverse.
pub fn sides(d: &TokenData, side: Side) -> (Option<Coin>, Option<Coin>) {
    let coin = d.coin();
    let sui = Some(d.sui_coin());
    match side {
        Side::Buy => (sui, coin),
        Side::Sell => (coin, sui),
    }
}

pub struct TokenPage {
    pub data: TokenData,
    pub st: TokenState,
    pub chart: ChartArea,
    pub host: Host,
}

fn chart_fmt(v: f64) -> String {
    let s = usd_price(v);
    if s.is_empty() {
        "$0".into()
    } else {
        s
    }
}

impl TokenPage {
    pub fn new(cx: &mut Cx, parent: Id, data: TokenData, side: Side, now: f64) -> TokenPage {
        let mut chart = ChartArea::new(Box::new(chart_fmt), TIMEFRAMES.len(), 1);
        chart.tz = Some(0);
        let st = TokenState { side, tf: 1, tab: Tab::Trades, reviewing: false, ticket: Ticket::new(Mode::Units, 5.0) };
        let mut p = TokenPage { data, st, chart, host: Host::new(cx, parent) };
        p.reset_ticket();
        p.load_candles();
        p.render(cx, now);
        p
    }

    /// A new side starts the ticket over at its default (5 SUI to buy, 50 % to sell).
    fn reset_ticket(&mut self) {
        let (mode, initial) = if self.st.side == Side::Buy { (Mode::Units, 5.0) } else { (Mode::Pct, 50.0) };
        let quote = self.data.quote.clone();
        self.st.ticket = Ticket::new(mode, initial);
        let (from, _) = sides(&self.data, self.st.side);
        self.st.ticket.balance = from.and_then(|c| self.data.balances.get(&normalize_type(&c.coin_type)).copied());
        if self.data.wallet.is_some() && self.st.ticket.balance.is_none() {
            self.st.ticket.balance = Some(0);
        }
        // The fixture's quote is for its default ticket only.
        self.st.ticket.quote = quote.filter(|q| q.amount_in == self.st.ticket.amount_in(sides(&self.data, self.st.side).0.as_ref()) || self.st.side == Side::Buy && initial == 5.0);
    }

    /// New answers (live data): the page keeps its ticket and chart view.
    pub fn set_data(&mut self, cx: &mut Cx, data: TokenData, now: f64) {
        let candles = data.candles != self.data.candles;
        self.data = data;
        let (from, _) = sides(&self.data, self.st.side);
        if self.data.wallet.is_some() {
            self.st.ticket.balance = Some(from.and_then(|c| self.data.balances.get(&normalize_type(&c.coin_type)).copied()).unwrap_or(0));
        }
        if candles {
            self.load_candles();
        }
        self.render(cx, now);
    }

    fn load_candles(&mut self) {
        let rows: Vec<Candle> = self.data.candles.iter().map(|b| Candle { time: b.time as i64, open: b.open, high: b.high, low: b.low, close: b.close, volume: b.volume }).collect();
        self.chart.tf = self.st.tf;
        self.chart.set_rows(&rows, &format!("{}:{}", self.data.coin_type, TIMEFRAMES[self.st.tf]));
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let focused_chart = self.host.tag() == Some(T_CHART);
        let (d, st, chart) = (&self.data, &self.st, &self.chart);
        self.host.render(cx, now, |ui| view(ui, d, st, chart, focused_chart));
    }

    fn area(&self) -> &'static str {
        match self.host.tag() {
            Some(T_CHART) => "chart",
            Some(t) if (T_SIDE..=T_REVIEW).contains(&t) => "order",
            _ => "other",
        }
    }

    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        if self.st.reviewing {
            return vec![("a", "Select".into()), ("b", "Cancel".into())];
        }
        if self.chart.mode.on {
            return chart_area::HINTS.iter().map(|(b, l)| (*b, l.to_string())).collect();
        }
        let flip = ("x", if self.st.side == Side::Buy { "Sell" } else { "Buy" }.to_string());
        let other = ("ltrt", if self.st.tab == Tab::Trades { "Holders" } else { "Trades" }.to_string());
        match self.area() {
            "order" => vec![("a", "Select".into()), ("lt", "Less".into()), ("rt", "More".into()), flip, ("y", "Search".into()), ("b", "Back".into())],
            "chart" => vec![("a", "Control chart".into()), ("select", "Chart mode".into()), flip, ("y", "Search".into()), other, ("b", "Back".into())],
            _ => vec![("a", "Select".into()), flip, ("y", "Search".into()), other, ("b", "Back".into())],
        }
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, repeat: bool, now: f64) -> Out {
        if self.st.reviewing {
            return match b {
                Pad::B => {
                    self.st.reviewing = false;
                    self.render(cx, now);
                    Out::Moved
                }
                Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
                Pad::A => match self.host.tag() {
                    Some(super::review::T_CONFIRM) => Out::Act("swap:sign".into()),
                    Some(super::review::T_CANCEL) => {
                        self.st.reviewing = false;
                        self.render(cx, now);
                        Out::Moved
                    }
                    _ => Out::None,
                },
                _ => Out::None,
            };
        }
        let focused = self.host.tag() == Some(T_CHART);
        match self.chart.press(b, repeat, focused) {
            ChartOut::Pass => {}
            ChartOut::Tf(i) => {
                self.st.tf = i;
                self.render(cx, now);
                return Out::Act(format!("tf:{}", TIMEFRAMES[i]));
            }
            ChartOut::Took | ChartOut::Mode(_) => {
                self.render(cx, now);
                return Out::Moved;
            }
        }
        match b {
            Pad::Lt | Pad::Rt if self.area() == "order" => {
                let (from, _) = sides(&self.data, self.st.side);
                self.st.ticket.bump(if b == Pad::Rt { 1 } else { -1 }, from.as_ref());
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
                self.render(cx, now);
                Out::Act("quote".into())
            }
            Pad::Lt | Pad::Rt => {
                self.st.tab = if self.st.tab == Tab::Trades { Tab::Holders } else { Tab::Trades };
                self.render(cx, now);
                Out::Moved
            }
            Pad::X => {
                self.st.side = if self.st.side == Side::Buy { Side::Sell } else { Side::Buy };
                self.reset_ticket();
                self.render(cx, now);
                Out::Moved
            }
            Pad::B => Out::Close,
            Pad::Y => Out::Act("search".into()),
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                let was = self.host.tag();
                let moved = self.host.step(cx, b, now);
                if was != self.host.tag() && (was == Some(T_CHART) || self.host.tag() == Some(T_CHART)) {
                    self.render(cx, now);
                }
                step_out(moved)
            }
            Pad::A => self.activate(cx, now),
            _ => Out::None,
        }
    }

    fn activate(&mut self, cx: &mut Cx, now: f64) -> Out {
        let Some(tag) = self.host.tag() else { return Out::None };
        let (from, _) = sides(&self.data, self.st.side);
        let presets = presets_for(self.st.ticket.mode, from.as_ref());
        match tag {
            T_ALERT => return Out::Act("alert".into()),
            t if (T_TF..T_TF + 4).contains(&t) => {
                self.st.tf = (t - T_TF) as usize;
                self.render(cx, now);
                return Out::Act(format!("tf:{}", TIMEFRAMES[self.st.tf]));
            }
            t if t == T_SIDE || t == T_SIDE + 1 => {
                let side = if t == T_SIDE { Side::Buy } else { Side::Sell };
                if side != self.st.side {
                    self.st.side = side;
                    self.reset_ticket();
                }
            }
            t if (T_PRESET..T_PRESET + 10).contains(&t) => {
                self.st.ticket.value = presets[(t - T_PRESET) as usize];
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
            }
            T_MAX => {
                let m = self.st.ticket.max(from.as_ref());
                self.st.ticket.value = (m * 1e4).floor() / 1e4;
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
            }
            T_MINUS | T_PLUS => {
                self.st.ticket.bump(if tag == T_PLUS { 1 } else { -1 }, from.as_ref());
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
            }
            T_REVIEW => {
                if panel_view(&self.data, &self.st).blocked {
                    return Out::None;
                }
                self.st.reviewing = true;
            }
            t if t >= T_ROW => return Out::Act(format!("trader:{}", self.data.trades.get((t - T_ROW) as usize).map(|r| r.trader.as_str()).unwrap_or(""))),
            _ => return Out::None,
        }
        self.render(cx, now);
        Out::Moved
    }
}

fn step_out(moved: bool) -> Out {
    if moved {
        Out::Moved
    } else {
        Out::None
    }
}

/// The page (TokenPage's JSX).
pub fn view(ui: &mut Ui, d: &TokenData, st: &TokenState, chart: &ChartArea, focused_chart: bool) -> El {
    let page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(rare_render::view::key("tr-token-scroll"));
    let Some(t) = d.token.as_ref() else {
        let msg = match d.token_error.as_deref() {
            Some("Token not indexed yet") => "This coin is not in the market data yet.",
            Some(_) => "Could not load this coin.",
            None => "",
        };
        if msg.is_empty() {
            return page.child(super::ui::loading_bar(ui));
        }
        return page.child(center_text(ui, s().pt(5.0).wp(1.0), msg, T::new(Ty::Small).c(tok::MUTED)));
    };

    // ---- header
    let logo = logo_url(t.logo.as_deref());
    let price = num_parts(NumKind::Price, t.price_usd, NumOptions::default()).map(|p| p.text()).unwrap_or_default();
    let ch = num_parts(NumKind::Change, t.change24(), NumOptions::default());
    let ch_col = ch.as_ref().map(|p| crate::kit::num::tone_color(p.tone, tok::FG)).unwrap_or(tok::FG);
    let title = baseline_row(ui, s().gap(0.5), vec![(T::new(Ty::Title).w(600).trunc(), t.name.clone()), (T::new(Ty::Body).w(500).c(tok::MUTED), t.symbol.clone())]);
    let pr = baseline_row(ui, s().gap(0.75), vec![(T::new(Ty::Display).w(600).tab(), price), (T::new(Ty::Body).tab().c(ch_col), ch.map(|p| p.text()).unwrap_or_default()), (T::new(Ty::Small).c(tok::MUTED), "24h".into())]);
    let pr = roll_first(ui, pr, t.price_usd);
    let names = ui.bx(s().col().shrink().min0()).child(title).child(pr);
    let mut right = ui.bx(s().row().items_center().gap(0.75).ml_auto());
    if t.price_usd.is_some_and(|p| p > 0.0) {
        right = right.child(button(ui, Btn { hint: Some("Set a price alert".into()), ..Btn::new("Price alert", T_ALERT).sm() }));
    }
    right = right.child(tf_chips(ui, &TIMEFRAMES, st.tf, "tr-tf", T_TF));
    let logo_el = coin_logo(ui, logo.as_deref(), &t.symbol, 3.5);
    let head = ui.bx(s().row().items_center().gap(1.0)).child(logo_el).child(names).child(right);

    // ---- stats
    let o = NumOptions::default();
    let bs = match (t.buys24h, t.sells24h) {
        (Some(b), Some(s_)) => format!("{} / {}", num_text(NumKind::Count, Some(b), o), num_text(NumKind::Count, Some(s_), o)),
        _ => String::new(),
    };
    let nz = |v: Option<f64>| v.filter(|x| *x != 0.0);
    let stats = stat_line(
        ui,
        s(),
        vec![
            ("Market cap", Stat::Num(NumKind::Market, nz(t.mcap), o)),
            ("24h volume", Stat::Num(NumKind::Market, nz(t.volume24h), o)),
            ("Liquidity", Stat::Num(NumKind::Market, nz(t.liquidity), o)),
            ("Holders", Stat::Num(NumKind::Count, nz(t.holders), o)),
            ("Buys / sells 24h", Stat::Text(bs)),
            ("Age", Stat::Text(if t.created_ms.is_some_and(|c| c != 0.0) { age(t.created_ms, d.now_ms) } else { String::new() })),
        ],
        true,
        false,
    );
    let stats_panel = panel(ui, s().col().mt(1.0).px(1.25).py(0.75)).maybe(stats);

    // ---- trades / holders
    let items = [TabItem::new("Trades"), TabItem::new("Holders")];
    let lt = glyph(ui, Pb::Lt, GSize::Sm);
    let rt = glyph(ui, Pb::Rt, GSize::Sm);
    let tabs_el = tabs(ui, s(), &items, if st.tab == Tab::Trades { 0 } else { 1 }, true);
    let tabs_row = ui.bx(s().row().items_stretch().gap(1.0).h(2.25).px(0.25)).child(ui.bx(s().row().items_center()).child(lt)).child(tabs_el).child(ui.bx(s().row().items_center()).child(rt));
    let table = match st.tab {
        Tab::Trades => trades_table(ui, &d.trades, &t.symbol, d.trades_error, d.now_ms),
        Tab::Holders => holders_table(ui, d.holders.as_ref(), d.holders_error),
    };
    let lists = ui.bx(s().col().mt(1.25)).child(tabs_row).child(panel(ui, s().col().mt(0.5).p(0.75)).child(table));

    let chart_el = chart.el(ui, s().mt(1.0).h(19.0), T_CHART, focused_chart);
    let left = ui.bx(s().col().min0()).child(head).child(chart_el).child(stats_panel).child(lists);
    let order = trade_panel(ui, d, st);
    let grid = ui.bx(s().grid(&[Track::Fr(1.0), Track::Rem(24.0)]).gap(1.5).px(2.5).items_start()).child(left).child(ui.bx(s().col().self_start()).child(order));
    let mut page = page.child(grid);
    if st.reviewing {
        if let Some(r) = review_for(d, st) {
            page = page.child(review_sheet(ui, &r, d.wallet.is_some()));
        }
    }
    page
}

/// The swap review (TokenPage's ReviewSheet children).
pub fn review_for(d: &TokenData, st: &TokenState) -> Option<Review> {
    let t = d.token.as_ref()?;
    let (from, to) = sides(d, st.side);
    let (from, to) = (from?, to?);
    let amt = base_to_number(st.ticket.amount_in(Some(&from)), from.decimals);
    let mut lines = vec![(if st.side == Side::Buy { "You pay" } else { "You sell" }.to_string(), Val::t(format!("{} {}", num_text(NumKind::Amount, Some(amt), NumOptions::default()), from.symbol)), false)];
    if let Some(q) = &st.ticket.quote {
        lines.push(("You get about".into(), Val::t(format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(q.amount_out, to.decimals)), NumOptions::default()), to.symbol)), true));
        lines.push(("Minimum received".into(), Val::t(format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(min_out(q.amount_out), to.decimals)), NumOptions::default()), to.symbol)), false));
        lines.push(("Price".into(), Val::t(t.price_usd.map(usd_price).unwrap_or_default()), false));
        lines.push(("Route".into(), Val::t(route_text(q)), false));
        lines.push(("Fee".into(), Val::t(format!("{:.2}%", q.fee_bps as f64 / 100.0)), false));
        lines.push(("Slippage".into(), Val::t("1%"), false));
    }
    Some(Review {
        title: format!("{} {}", if st.side == Side::Buy { "Buy" } else { "Sell" }, t.symbol),
        sub: Some(t.name.clone()),
        action: if st.side == Side::Buy { "Confirm buy" } else { "Confirm sell" }.into(),
        tone: if st.side == Side::Buy { Tone::Buy } else { Tone::Sell },
        lines,
    })
}

/// TradePanel: Buy/Sell, the amount with its stepper, presets, what you get, and the button.
pub fn trade_panel(ui: &mut Ui, d: &TokenData, st: &TokenState) -> El {
    let pv = panel_view(d, st);
    let (from, to) = sides(d, st.side);
    let t = &st.ticket;
    let switch = side_switch(ui, [("Buy", Tone::Buy), ("Sell", Tone::Sell)], if st.side == Side::Buy { 0 } else { 1 }, "tr-side", T_SIDE);
    let amount = center_text(ui, s().wp(1.0), pv.amount_label.clone(), T::new(Ty::Display).w(600).tab());
    let stepped = stepper(ui, "amount", "tr-amount", T_MINUS, T_PLUS, amount);
    let under = center_text(ui, s().mt(0.25).h(1.25).wp(1.0), pv.under.clone(), T::new(Ty::Small).c(tok::MUTED).tab());
    let pay = ui.bx(s().col()).child(ui.text(if st.side == Side::Buy { "You pay" } else { "You sell" }, T::new(Ty::Small).c(tok::MUTED))).child(ui.bx(s().col().mt(0.25)).child(stepped)).child(under);

    let mut chips = ui.bx(s().row().gap(0.5)).group(crate::kit::group("tr-presets", true));
    for (i, v) in presets_for(t.mode, from.as_ref()).iter().enumerate() {
        let label = if st.side == Side::Sell { if *v >= 100.0 { "Max".to_string() } else { format!("{}%", trim6(*v)) } } else { trim6(*v) };
        let c = chip(ui, &label, (t.value - v).abs() < 1e-9, T_PRESET + i as u32);
        chips = chips.child(c);
    }
    let max = t.max(from.as_ref());
    if st.side == Side::Buy && max.is_finite() && max > 0.0 {
        let c = chip(ui, "Max", (t.value - max).abs() < 1e-9, T_MAX);
        chips = chips.child(c);
    }

    let get = if pv.get.is_empty() { None } else { Some(ui.text(pv.get.clone(), T::new(Ty::Small).w(600).tab().c(if pv.out.is_some() { tok::FG } else { tok::MUTED }))) };
    let mut lines_el = ui.bx(s().col()).child(line_el(ui, "You get about", get, false, false));
    if let (Some(q), Some(c)) = (&t.quote, &to) {
        let min = format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(min_out(q.amount_out), c.decimals)), NumOptions::default()), c.symbol);
        let v = Some(ui.text(min, T::new(Ty::Small).tab()));
        lines_el = lines_el.child(line_el(ui, "Minimum received", v, false, false));
        let v = Some(ui.text(route_text(q), T::new(Ty::Small).tab()));
        lines_el = lines_el.child(line_el(ui, "Route", v, false, false));
        let v = Some(ui.text(format!("{:.2}%", q.fee_bps as f64 / 100.0), T::new(Ty::Small).tab()));
        lines_el = lines_el.child(line_el(ui, "Fee", v, false, false));
    }
    let bal = if pv.balance.is_empty() { None } else { Some(ui.text(pv.balance.clone(), T::new(Ty::Small).tab())) };
    lines_el = lines_el.child(line_el(ui, "Balance", bal, false, true));

    let tone = if st.side == Side::Buy { Tone::Buy } else { Tone::Sell };
    let hint = if pv.blocked { pv.why.clone() } else { Some("Review".into()) };
    let review = action(ui, s(), Action { def: true, full: true, disabled: pv.blocked, hint, ..Action::new(pv.button.clone(), tone, T_REVIEW) });
    let mut p = panel(ui, s().col().gap(1.0).p(1.25)).child(switch).child(pay).child(chips).child(lines_el).child(review);
    if let Some(w) = pv.why.as_ref().filter(|_| !t.quoting) {
        p = p.child(center_text(ui, s().mt(-0.5).wp(1.0), w.clone(), T::new(Ty::Label).c(tok::MUTED)));
    }
    p
}

const TRADE_COLS: [Track; 6] = [Track::Rem(4.0), Track::Rem(4.0), Track::Fr(1.0), Track::Rem(7.0), Track::Rem(7.0), Track::Rem(8.0)];

fn ct(ui: &Ui, text: impl Into<String>, t: T, end: bool) -> El {
    cell(ui, end).child(ui.text(text, t.tab()))
}

/// The recent trades (TradesTable).
pub fn trades_table(ui: &mut Ui, rows: &[TradeRow], symbol: &str, error: bool, now_ms: f64) -> El {
    if rows.is_empty() {
        return center_text(ui, s().py(2.0).wp(1.0), if error { "Couldn't load trades." } else { "" }, T::new(Ty::Small).c(tok::MUTED));
    }
    let heads: Vec<El> = ["Age", "Side", "Amount", "Value", "Price", "Trader"].iter().enumerate().map(|(i, l)| ct(ui, *l, T::new(Ty::Label).w(500).c(tok::MUTED), i > 1)).collect();
    let mut out = ui.bx(s().col()).child(head_row(ui, &TRADE_COLS, heads));
    let mut list = ui.bx(s().col()).group(crate::kit::group("tr-trades", false));
    for (i, r) in rows.iter().take(30).enumerate() {
        let small = T::new(Ty::Small);
        let value = cell(ui, true).maybe(num(ui, s(), NumKind::Money, Some(r.usd), NumOptions::default(), small, true));
        let cells = vec![
            ct(ui, age(Some(r.time_ms), now_ms), small.c(tok::MUTED), false),
            ct(ui, if r.buy { "Buy" } else { "Sell" }, small.w(500).c(if r.buy { tok::POS } else { tok::NEG }), false),
            ct(ui, format!("{} {}", num_text(NumKind::Amount, Some(r.amount), NumOptions::default()), symbol), small, true),
            value,
            ct(ui, usd_price(r.price_usd), small.c(tok::MUTED), true),
            ct(ui, short_addr(&r.trader), small.mono().c(tok::MUTED), true),
        ];
        list = list.child(row(ui, &TRADE_COLS, T_ROW + i as u32, None, cells).key(rare_render::view::key(&r.id)));
    }
    out = out.child(list);
    out
}

const HOLDER_COLS: [Track; 5] = [Track::Rem(2.5), Track::Fr(1.0), Track::Rem(8.0), Track::Rem(7.0), Track::Rem(5.0)];

/// The top holders (HoldersTable).
pub fn holders_table(ui: &mut Ui, page: Option<&HoldersPage>, error: bool) -> El {
    let Some(p) = page.filter(|p| !p.holders.is_empty()) else {
        let msg = if error {
            "Couldn't load holders."
        } else if page.is_some() {
            "No holders seen yet."
        } else {
            ""
        };
        return center_text(ui, s().py(2.0).wp(1.0), msg, T::new(Ty::Small).c(tok::MUTED));
    };
    let head2 = match p.count.filter(|c| *c != 0.0) {
        Some(c) => format!("{} holders", num_text(NumKind::Count, Some(c), NumOptions::default())),
        None => "Holder".into(),
    };
    let heads = vec![
        ct(ui, "#", T::new(Ty::Label).w(500).c(tok::MUTED), false),
        ct(ui, head2, T::new(Ty::Label).w(500).c(tok::MUTED), false),
        ct(ui, "Amount", T::new(Ty::Label).w(500).c(tok::MUTED), true),
        ct(ui, "Value", T::new(Ty::Label).w(500).c(tok::MUTED), true),
        ct(ui, "Share", T::new(Ty::Label).w(500).c(tok::MUTED), true),
    ];
    let out = ui.bx(s().col()).child(head_row(ui, &HOLDER_COLS, heads));
    let mut list = ui.bx(s().col()).group(crate::kit::group("tr-holders", false));
    for (i, h) in p.holders.iter().enumerate() {
        let small = T::new(Ty::Small);
        let mut who = ui.bx(s().row().items_center().gap(0.5).min0()).child(ui.text(short_addr(&h.address), small.mono()));
        if h.kind != "wallet" {
            let tag = match (&h.venue, h.kind.as_str()) {
                (Some(v), _) => venue_name(v),
                (None, "pool") => "Pool".into(),
                (None, k) => k.to_string(),
            };
            who = who.child(ui.text(tag, T::new(Ty::Label).c(tok::MUTED)));
        }
        let value = cell(ui, true).maybe(num(ui, s(), NumKind::Money, h.value_usd, NumOptions { compact: true, ..Default::default() }, small, true));
        let cells = vec![
            ct(ui, (i + 1).to_string(), small.c(tok::MUTED), false),
            cell(ui, false).child(who),
            ct(ui, num_text(NumKind::Amount, Some(h.amount), NumOptions::default()), small, true),
            value,
            ct(ui, h.pct_supply.map(|v| num_text(NumKind::Share, Some(v), NumOptions::default())).unwrap_or_default(), small, true),
        ];
        list = list.child(row(ui, &HOLDER_COLS, T_ROW + 500 + i as u32, None, cells).key(rare_render::view::key(&h.address)));
    }
    out.child(list)
}

impl Page for TokenPage {
    fn any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn prepare_paint(&mut self, scene: &Scene, ts: &mut TextSys, tex: &mut dyn Textures) {
        if self.data.token.is_none() {
            return;
        }
        let Some(area) = self.host.id(chart_area::AREA) else { return };
        self.chart.prepare(scene, area, ts, tex);
    }
    fn paint_slot(&mut self, sp: &rare_render::scene::SlotPaint, ts: &mut TextSys, out: &mut DrawList) {
        if !self.st.reviewing {
            self.chart.paint_slot(sp, ts, out);
        }
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for TokenPage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, false, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host.tick(cx, now) {
            self.render(cx, now);
        }
        self.chart.tick(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(side: Side, d: &TokenData) -> TokenState {
        let mut st = TokenState { side, tf: 1, tab: Tab::Trades, reviewing: false, ticket: Ticket::new(if side == Side::Buy { Mode::Units } else { Mode::Pct }, if side == Side::Buy { 5.0 } else { 50.0 }) };
        let (from, _) = sides(d, side);
        st.ticket.balance = from.and_then(|c| d.balances.get(&normalize_type(&c.coin_type)).copied()).or(Some(0));
        st
    }

    #[test]
    fn buy_panel_says_what_the_web_says() {
        let d = TokenData::from_fixture(&token_deep(), Some(super::super::DEMO));
        let mut st = state(Side::Buy, &d);
        st.ticket.quote = d.quote.clone();
        let pv = panel_view(&d, &st);
        assert_eq!(pv.amount_label, "5 SUI");
        assert_eq!(pv.under, "about $4.62");
        assert_eq!(pv.get, "276.43 DEEP");
        assert_eq!(pv.balance, "57.5 SUI");
        assert_eq!(pv.button, "Review buy DEEP");
        assert!(!pv.blocked);
        assert_eq!(pv.why, None);
        let r = review_for(&d, &st).unwrap();
        assert_eq!(r.title, "Buy DEEP");
        assert!(matches!(&r.lines[2].1, Val::Text(t) if t == "273.66 DEEP"));
    }

    #[test]
    fn sell_panel_without_the_coin() {
        let d = TokenData::from_fixture(&token_deep(), Some(super::super::DEMO));
        let st = state(Side::Sell, &d);
        let pv = panel_view(&d, &st);
        assert_eq!(pv.amount_label, "50%");
        assert_eq!(pv.under, "of your DEEP");
        assert_eq!(pv.why.as_deref(), Some("No DEEP to sell"));
        assert_eq!(pv.button, "Review sell DEEP");
        assert!(pv.blocked);
        assert_eq!(trim6(1234.5678912), "1,234.57");
    }
}
