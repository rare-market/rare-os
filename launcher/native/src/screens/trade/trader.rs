//! A trader's page (trade/traders/TraderPage.tsx, MirrorSheet.tsx, mirror.ts): who they are, their
//! realized P/L curve, what they hold now, and their latest buys and sells. X follows them on this
//! device; A on a trade mirrors it at this account's size (the swap review, then Rare Wallet's).

use super::fmt::normalize_type;
use super::model::logo_url;
use super::review::{review_sheet, Review};
use super::ticket::{to_base, SUI_TYPE};

use super::traders::{ago, pct, trader_avatar, trader_name, Follow, TokenChip, UserChip};
use super::ui::{action, chip, coin_logo, tf_chips, Action, Tone, Val};
use crate::kit::format::{num_text, NumKind, NumOptions};
use crate::kit::{cell, dot, head_row, mini_curve, num, panel, row, section_head, DotTone};
use crate::screens::market::host::*;
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::Id;
use serde::Deserialize;
use std::collections::HashMap;

pub const BUY_SIZES: [f64; 5] = [1.0, 5.0, 10.0, 25.0, 50.0];
pub const SELL_SIZES: [f64; 3] = [25.0, 50.0, 100.0];
pub const USDC_TYPE: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub address: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Label {
    pub smart: bool,
    pub whale: bool,
    pub kol: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Holding {
    pub coin: String,
    pub amount: f64,
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
    pub unrealized: Option<f64>,
    pub token: Option<TokenChip>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub realized: f64,
    pub unrealized: f64,
    pub trades: f64,
    pub volume: f64,
    pub win_rate: Option<f64>,
    pub holdings_usd: Option<f64>,
    pub label: Option<Label>,
    pub bot: bool,
    pub holdings: Vec<Holding>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PnlPoint {
    pub time: f64,
    pub pnl_usd: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Points {
    pub points: Vec<PnlPoint>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TraderTrade {
    pub id: String,
    pub time_ms: f64,
    pub buy: bool,
    pub usd: f64,
    pub amount: f64,
    pub price_usd: f64,
    pub coin: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct TradesPage {
    pub items: Vec<TraderTrade>,
    pub tokens: HashMap<String, TokenChip>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct LinkedPage {
    pub total: u32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TraderFixture {
    pub now: f64,
    pub address: String,
    pub profile: Option<Profile>,
    pub stats: Option<Stats>,
    pub pnl: Option<Points>,
    pub trades: Option<TradesPage>,
    pub linked: Option<LinkedPage>,
}

pub fn trader_fixture() -> TraderFixture {
    serde_json::from_str(include_str!("fixtures/trader.json")).expect("trader fixture")
}

/// What the page reads.
#[derive(Clone, Debug, Default)]
pub struct TraderData {
    pub now_ms: f64,
    pub address: String,
    pub profile: Option<Profile>,
    pub stats: Option<Stats>,
    pub stats_error: bool,
    pub pnl: Option<Vec<PnlPoint>>,
    pub pnl_error: bool,
    pub trades: Option<TradesPage>,
    pub trades_error: bool,
    pub linked: Option<LinkedPage>,
    pub follows: Vec<Follow>,
    pub wallet: Option<String>,
}

impl TraderData {
    pub fn from_fixture(f: &TraderFixture, wallet: Option<&str>) -> TraderData {
        TraderData {
            now_ms: f.now,
            address: f.address.clone(),
            profile: f.profile.clone(),
            stats: f.stats.clone(),
            pnl: f.pnl.as_ref().map(|p| p.points.clone()),
            trades: f.trades.clone(),
            linked: f.linked.clone(),
            wallet: wallet.map(str::to_string),
            ..Default::default()
        }
    }

    pub fn user(&self) -> UserChip {
        let p = self.profile.clone().unwrap_or_default();
        UserChip { address: self.address.clone(), username: p.username, display_name: p.display_name, avatar_url: p.avatar_url }
    }

    pub fn following(&self) -> bool {
        self.follows.iter().any(|f| f.address.eq_ignore_ascii_case(&self.address))
    }

    /// Holdings worth a dollar or more, biggest first.
    pub fn holdings(&self) -> Vec<&Holding> {
        let mut h: Vec<&Holding> = self.stats.as_ref().map(|s| s.holdings.iter().filter(|h| h.value_usd.unwrap_or(0.0) >= 1.0).collect()).unwrap_or_default();
        h.sort_by(|a, b| b.value_usd.unwrap_or(0.0).partial_cmp(&a.value_usd.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
        h
    }

    /// The labels line: the ledger's labels and the funding links.
    pub fn words(&self) -> Vec<String> {
        let mut w = Vec::new();
        if let Some(l) = self.stats.as_ref().and_then(|s| s.label.as_ref()) {
            if l.smart {
                w.push("Smart money".to_string());
            }
            if l.whale {
                w.push("Whale".into());
            }
            if l.kol {
                w.push("KOL".into());
            }
        }
        if let Some(t) = self.linked.as_ref().map(|l| l.total).filter(|t| *t > 0) {
            w.push(format!("Linked to {t} {}", if t == 1 { "wallet" } else { "wallets" }));
        }
        w
    }
}

/// The size mirrored trades use (mirror.ts MirrorSize).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MirrorSize {
    pub buy_sui: f64,
    pub sell_pct: f64,
}

impl Default for MirrorSize {
    fn default() -> Self {
        MirrorSize { buy_sui: 5.0, sell_pct: 100.0 }
    }
}

/// The swap that mirrors a trade (mirror.ts `mirrorPlan`).
#[derive(Clone, Debug, PartialEq)]
pub enum MirrorPlan {
    Ok { buy: bool, coin_in: String, coin_out: String, amount_in: u128, decimals_in: u32 },
    No(String),
}

pub fn mirror_plan(trade: &TraderTrade, size: MirrorSize, coin_decimals: u32, held: Option<u128>) -> MirrorPlan {
    let coin = normalize_type(&trade.coin);
    let is_sui = coin == SUI_TYPE;
    if trade.buy {
        if size.buy_sui.is_nan() || size.buy_sui <= 0.0 {
            return MirrorPlan::No("Choose a size first.".into());
        }
        if is_sui {
            if trade.price_usd.is_nan() || trade.price_usd <= 0.0 {
                return MirrorPlan::No("No price for SUI right now.".into());
            }
            let usd = super::fmt::to_fixed(size.buy_sui * trade.price_usd, 6);
            return MirrorPlan::Ok { buy: true, coin_in: normalize_type(USDC_TYPE), coin_out: SUI_TYPE.into(), amount_in: super::ticket::to_base_str(&usd, 6), decimals_in: 6 };
        }
        return MirrorPlan::Ok { buy: true, coin_in: SUI_TYPE.into(), coin_out: coin, amount_in: to_base(size.buy_sui, 9), decimals_in: 9 };
    }
    let Some(held) = held else { return MirrorPlan::No("Checking your balance.".into()) };
    let spend = super::ticket::spendable(held, &coin);
    let amount = super::ticket::pct_of(spend, size.sell_pct);
    if amount == 0 {
        return MirrorPlan::No("You hold none of this coin.".into());
    }
    let out = if is_sui { normalize_type(USDC_TYPE) } else { SUI_TYPE.to_string() };
    MirrorPlan::Ok { buy: false, coin_in: coin, coin_out: out, amount_in: amount, decimals_in: coin_decimals }
}

pub struct TraderPage {
    pub data: TraderData,
    pub win30: bool,
    pub size: MirrorSize,
    pub mirroring: Option<usize>,
    pub host: Host,
}

const T_FOLLOW: u32 = 10;
const T_WIN: u32 = 100;
const T_BUY: u32 = 200;
const T_SELL: u32 = 300;
const T_TRADE: u32 = 1000;
const T_HOLD: u32 = 2000;

impl TraderPage {
    pub fn new(cx: &mut Cx, parent: Id, data: TraderData, now: f64) -> TraderPage {
        let mut p = TraderPage { data, win30: true, size: MirrorSize::default(), mirroring: None, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    pub fn set_data(&mut self, cx: &mut Cx, data: TraderData, now: f64) {
        self.data = data;
        self.render(cx, now);
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let (d, w, sz, m) = (&self.data, self.win30, self.size, self.mirroring);
        self.host.render(cx, now, |ui| view(ui, d, w, sz, m));
    }

    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        if self.mirroring.is_some() {
            return vec![("a", "Select".into()), ("b", "Cancel".into())];
        }
        vec![("a", "Select".into()), ("x", if self.data.following() { "Unfollow" } else { "Follow" }.into()), ("b", "Back".into())]
    }

    fn toggle_follow(&mut self) {
        let name = trader_name(&self.data.user());
        if self.data.following() {
            let a = self.data.address.clone();
            self.data.follows.retain(|f| !f.address.eq_ignore_ascii_case(&a));
        } else {
            self.data.follows.push(Follow { address: self.data.address.clone(), name });
        }
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        if self.mirroring.is_some() {
            return match b {
                Pad::B => {
                    self.mirroring = None;
                    self.render(cx, now);
                    Out::Moved
                }
                Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
                Pad::A => match self.host.tag() {
                    Some(super::review::T_CONFIRM) => Out::Act("mirror:sign".into()),
                    Some(super::review::T_CANCEL) => {
                        self.mirroring = None;
                        self.render(cx, now);
                        Out::Moved
                    }
                    _ => Out::None,
                },
                _ => Out::None,
            };
        }
        match b {
            Pad::X => {
                self.toggle_follow();
                self.render(cx, now);
                Out::Act("follows".into())
            }
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
            Pad::A => {
                let Some(tag) = self.host.tag() else { return Out::None };
                match tag {
                    T_FOLLOW => {
                        self.toggle_follow();
                        self.render(cx, now);
                        return Out::Act("follows".into());
                    }
                    T_WIN | 101 => {
                        self.win30 = tag == 101;
                        self.render(cx, now);
                        return Out::Act(format!("trader-window:{}", if self.win30 { "30d" } else { "7d" }));
                    }
                    // The size already chosen: say what it is (pressing it changes nothing).
                    t if (T_BUY..T_BUY + 5).contains(&t) && self.size.buy_sui == BUY_SIZES[(t - T_BUY) as usize] => return Out::Act(format!("toast:Each mirrored buy is {} SUI.", self.size.buy_sui)),
                    t if (T_SELL..T_SELL + 3).contains(&t) && self.size.sell_pct == SELL_SIZES[(t - T_SELL) as usize] => return Out::Act(format!("toast:Each mirrored sell is {}% of what you hold.", self.size.sell_pct)),
                    t if (T_BUY..T_BUY + 5).contains(&t) => self.size.buy_sui = BUY_SIZES[(t - T_BUY) as usize],
                    t if (T_SELL..T_SELL + 3).contains(&t) => self.size.sell_pct = SELL_SIZES[(t - T_SELL) as usize],
                    t if (T_TRADE..T_HOLD).contains(&t) => self.mirroring = Some((t - T_TRADE) as usize),
                    t if t >= T_HOLD => {
                        let h = self.data.holdings();
                        return match h.get((t - T_HOLD) as usize) {
                            Some(x) => Out::Act(format!("token:{}", x.coin)),
                            None => Out::None,
                        };
                    }
                    _ => return Out::None,
                }
                self.render(cx, now);
                Out::Moved
            }
            _ => Out::None,
        }
    }
}

fn step_out(moved: bool) -> Out {
    if moved {
        Out::Moved
    } else {
        Out::None
    }
}

const TRADE_COLS: [Track; 5] = [Track::Rem(3.0), Track::Rem(3.5), Track::Fr(1.0), Track::Rem(6.5), Track::Rem(6.5)];

fn ct(ui: &Ui, text: impl Into<String>, t: T, end: bool) -> El {
    cell(ui, end).child(ui.text(text, t.tab()))
}

fn short_coin(coin: &str) -> String {
    coin.rsplit("::").next().unwrap_or(coin).to_string()
}

pub fn view(ui: &mut Ui, d: &TraderData, win30: bool, size: MirrorSize, mirroring: Option<usize>) -> El {
    let user = d.user();
    let name = trader_name(&user);
    let following = d.following();

    // Header.
    let mut labels = ui.bx(s().row().wrap().items_center().gap_xy(1.0, 0.25).mt(0.35));
    for w in d.words() {
        labels = labels.child(ui.text(w, T::new(Ty::Label).c(tok::MUTED)));
    }
    if d.stats.as_ref().is_some_and(|s_| s_.bot) {
        labels = labels.child(ui.text("Trades like a bot", T::new(Ty::Label).c(tok::MUTED)));
    }
    let who = ui
        .bx(s().col().flex(1.0).min0())
        .child(ui.text(name.clone(), T::new(Ty::Title).w(600).track(-0.02).trunc()))
        .child(ui.text(d.address.clone(), T::new(Ty::Meta).mono().tab().c(tok::MUTED).trunc()))
        .child(labels);
    let follow = action(ui, s(), Action { def: true, lg: false, hint: Some(if following { "Unfollow" } else { "Follow" }.into()), ..Action::new(if following { "Following" } else { "Follow" }, if following { Tone::Secondary } else { Tone::Primary }, T_FOLLOW) });
    let avatar = trader_avatar(ui, &user, 4.5);
    let head = ui.bx(s().row().items_end().gap(1.25).px(2.5)).child(avatar).child(who).child(ui.bx(s().row().gap(0.5)).group(crate::kit::group("tr-trader-actions", true)).child(follow));

    // Stats.
    let st = d.stats.as_ref();
    let o = NumOptions::default();
    let stats = stat_line(
        ui,
        s(),
        vec![
            ("Realized P/L", Stat::Num(NumKind::Pnl, st.map(|x| x.realized), o)),
            ("Unrealized", Stat::Num(NumKind::Pnl, st.map(|x| x.unrealized), o)),
            ("Win rate", Stat::Text(st.map(|x| pct(x.win_rate)).unwrap_or_default())),
            ("Trades", Stat::Text(st.map(|x| super::fmt::grouped(x.trades, 0, 0)).unwrap_or_default())),
            ("Volume", Stat::Num(NumKind::Market, st.map(|x| x.volume), o)),
            ("Holdings", Stat::Num(NumKind::Money, st.and_then(|x| x.holdings_usd), o)),
        ],
        true,
        true,
    );
    let stats_panel = panel(ui, s().col().mx(2.5).mt(1.25).px(1.25).py(1.0)).maybe(stats);

    // P/L curve.
    let points: Vec<(f64, f64)> = d.pnl.as_ref().map(|p| p.iter().map(|x| (x.time, x.pnl_usd)).collect()).unwrap_or_default();
    let note = if points.len() > 1 {
        None
    } else if d.pnl_error {
        Some("Unavailable")
    } else {
        Some("")
    };
    let win_chips = tf_chips(ui, &["7 days", "30 days"], if win30 { 1 } else { 0 }, "tr-trader-win", T_WIN);
    let curve_w = ui.w / ui.rem - 5.0 - 24.0 - 1.25 - 2.5;
    let mut curve_box = ui.bx(s().mt(0.75).h(11.0).wp(1.0));
    if points.len() > 1 {
        curve_box = curve_box.child(mini_curve(ui, s(), &points, curve_w, 11.0, None));
    }
    let pl = panel(ui, s().col().p(1.25)).child(ui.bx(s().row().items_center().gap(1.0)).child(section_head(ui, s(), "P/L", note, None)).child(ui.bx(s().ml_auto()).child(win_chips))).child(curve_box);

    // Latest trades.
    let live = ui.bx(s().row().items_center().gap(0.4)).child(dot(ui, s(), DotTone::Brand, true)).child(ui.text("Live", T::new(Ty::Small).c(tok::MUTED)));
    let heads: Vec<El> = ["Age", "Side", "Coin", "Value", "Price"].iter().enumerate().map(|(i, l)| ct(ui, *l, T::new(Ty::Label).w(500).c(tok::MUTED), i > 2)).collect();
    let mut tp = panel(ui, s().col().p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "Latest trades", None, Some(live))).child(head_row(ui, &TRADE_COLS, heads));
    let items = d.trades.as_ref().map(|t| t.items.as_slice()).unwrap_or(&[]);
    let mut list = ui.bx(s().col()).group(crate::kit::group("tr-trader-trades", false));
    for (i, t) in items.iter().enumerate() {
        let tok_ = d.trades.as_ref().and_then(|p| p.tokens.get(&t.coin));
        let sym = tok_.map(|x| x.symbol.clone()).unwrap_or_else(|| short_coin(&t.coin));
        let logo = logo_url(tok_.and_then(|x| x.logo.as_deref()));
        let small = T::new(Ty::Small);
        let coin = cell(ui, false).child(ui.bx(s().row().items_center().gap(0.6).min0()).child(coin_logo(ui, logo.as_deref(), &sym, 1.75)).child(ui.text(sym.clone(), small.w(600).trunc())));
        let cells = vec![
            ct(ui, ago(t.time_ms, d.now_ms), small.c(tok::MUTED), false),
            ct(ui, if t.buy { "Buy" } else { "Sell" }, small.w(600).c(if t.buy { tok::POS } else { tok::NEG }), false),
            coin,
            cell(ui, true).maybe(num(ui, s(), NumKind::Money, Some(t.usd), o, small, true)),
            cell(ui, true).maybe(num(ui, s(), NumKind::Price, Some(t.price_usd), o, small.c(tok::MUTED), true)),
        ];
        list = list.child(row(ui, &TRADE_COLS, T_TRADE + i as u32, Some(if t.buy { "Mirror buy" } else { "Mirror sell" }), cells).key(rare_render::view::key(&t.id)));
    }
    tp = tp.child(list);
    if items.is_empty() {
        let msg = if d.trades_error {
            "Could not load trades. This page refreshes on its own."
        } else if d.trades.is_some() {
            "No trades yet."
        } else {
            ""
        };
        tp = tp.child(center_text(ui, s().py(2.0).wp(1.0), msg, T::new(Ty::Small).c(tok::MUTED)));
    }
    let left = ui.bx(s().col().gap(1.25).min0()).child(pl).child(tp);

    // Mirror size.
    let mut buys = ui.bx(s().row().gap(0.4).mt(0.4)).group(crate::kit::group("tr-mirror-buy", true));
    for (i, v) in BUY_SIZES.iter().enumerate() {
        let c = super::ui::chip_check(ui, &format!("{}", *v as i64), size.buy_sui == *v, T_BUY + i as u32);
        buys = buys.child(c);
    }
    let mut sells = ui.bx(s().row().gap(0.4).mt(0.4)).group(crate::kit::group("tr-mirror-sell", true));
    for (i, v) in SELL_SIZES.iter().enumerate() {
        let c = super::ui::chip_check(ui, &format!("{}%", *v as i64), size.sell_pct == *v, T_SELL + i as u32);
        sells = sells.child(c);
    }
    let mirror = panel(ui, s().col().p(1.25))
        .child(section_head(ui, s(), "Mirror size", None, None))
        .child(ui.text_s(s().mt(0.25), "A on a trade builds the same swap at your size. You review it before anything is signed.", T::new(Ty::Small).c(tok::MUTED).wrap()))
        .child(ui.text_s(s().mt(0.9), "Each buy, in SUI", T::new(Ty::Label).c(tok::MUTED)))
        .child(buys)
        .child(ui.text_s(s().mt(0.9), "Each sell, of what you hold", T::new(Ty::Label).c(tok::MUTED)))
        .child(sells);

    // Holdings.
    let holdings = d.holdings();
    let hn = (!holdings.is_empty()).then(|| holdings.len().to_string());
    let mut hp = panel(ui, s().col().p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "Holdings", hn.as_deref(), None));
    let mut hl = ui.bx(s().col()).group(crate::kit::group("tr-trader-holdings", false));
    for (i, h) in holdings.iter().take(12).enumerate() {
        let sym = h.token.as_ref().map(|x| x.symbol.clone()).unwrap_or_else(|| short_coin(&h.coin));
        let logo = logo_url(h.token.as_ref().and_then(|x| x.logo.as_deref()));
        let mut names = ui.bx(s().col().flex(1.0).min0()).child(ui.text(sym.clone(), T::new(Ty::Body).w(600).trunc()));
        if let Some(u) = h.unrealized.filter(|u| u.abs() >= 0.01) {
            names = names.maybe(num(ui, s(), NumKind::Pnl, Some(u), o, T::new(Ty::Label), true));
        }
        let value = num(ui, s(), NumKind::Money, h.value_usd, o, T::new(Ty::Small).w(500), true);
        let r = ui.bx(s().row().items_center().gap(0.75).h(3.25).wp(1.0).px(0.75)).radius(ui.r(0.5)).child(coin_logo(ui, logo.as_deref(), &sym, 2.0)).child(names).maybe(value);
        hl = hl.child(crate::kit::nav(crate::kit::row_look(r), T_HOLD + i as u32, Some("Trade"), None, false).key(rare_render::view::key(&h.coin)));
    }
    hp = hp.child(hl);
    if holdings.is_empty() {
        let msg = if d.stats.is_some() {
            "Holds nothing priced right now."
        } else if d.stats_error {
            "Holdings are unavailable right now."
        } else {
            ""
        };
        hp = hp.child(center_text(ui, s().py(2.0).wp(1.0), msg, T::new(Ty::Small).c(tok::MUTED)));
    }
    let right = ui.bx(s().col().gap(1.25).min0()).child(mirror).child(hp);
    let grid = ui.bx(s().grid(&[Track::Fr(1.0), Track::Rem(24.0)]).gap(1.25).mx(2.5).mt(1.25).items_start()).child(left).child(right);
    let mut page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(rare_render::view::key("tr-trader")).child(head).child(stats_panel).child(grid);

    if let Some(t) = mirroring.and_then(|i| items.get(i)) {
        let sym = d.trades.as_ref().and_then(|p| p.tokens.get(&t.coin)).map(|x| x.symbol.clone()).unwrap_or_else(|| short_coin(&t.coin));
        let usd = num_text(NumKind::Money, Some(t.usd), o);
        let r = Review {
            title: format!("Mirror {} of {sym}", if t.buy { "buy" } else { "sell" }),
            sub: Some(format!("{name} {} {usd}. Your size: {}.", if t.buy { "bought" } else { "sold" }, if t.buy { format!("{} SUI", size.buy_sui) } else { format!("{}% of your {sym}", size.sell_pct) })),
            action: if t.buy { "Confirm buy" } else { "Confirm sell" }.into(),
            tone: if t.buy { Tone::Buy } else { Tone::Sell },
            lines: vec![("Trader".into(), Val::t(format!("{} {usd} of {sym}", if t.buy { "Bought" } else { "Sold" })), false)],
        };
        page = page.child(review_sheet(ui, &r, d.wallet.is_some()));
    }
    page
}

impl Page for TraderPage {
    fn any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for TraderPage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host.tick(cx, now) {
            self.render(cx, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(buy: bool, coin: &str, price: f64) -> TraderTrade {
        TraderTrade { buy, coin: coin.into(), price_usd: price, ..Default::default() }
    }

    #[test]
    fn mirror_plans() {
        let s = MirrorSize::default();
        match mirror_plan(&trade(true, "0xdeeb::deep::DEEP", 0.02), s, 6, None) {
            MirrorPlan::Ok { coin_in, amount_in, decimals_in, .. } => {
                assert_eq!(coin_in, SUI_TYPE);
                assert_eq!(amount_in, 5_000_000_000);
                assert_eq!(decimals_in, 9);
            }
            p => panic!("{p:?}"),
        }
        match mirror_plan(&trade(true, "0x2::sui::SUI", 0.92), s, 9, None) {
            MirrorPlan::Ok { coin_in, amount_in, .. } => {
                assert_eq!(coin_in, normalize_type(USDC_TYPE));
                assert_eq!(amount_in, 4_600_000);
            }
            p => panic!("{p:?}"),
        }
        assert_eq!(mirror_plan(&trade(false, "0xdeeb::deep::DEEP", 0.02), s, 6, Some(0)), MirrorPlan::No("You hold none of this coin.".into()));
        match mirror_plan(&trade(false, "0xdeeb::deep::DEEP", 0.02), MirrorSize { buy_sui: 5.0, sell_pct: 50.0 }, 6, Some(1_000)) {
            MirrorPlan::Ok { amount_in, coin_out, .. } => {
                assert_eq!(amount_in, 500);
                assert_eq!(coin_out, SUI_TYPE);
            }
            p => panic!("{p:?}"),
        }
    }

    #[test]
    fn page_words() {
        let d = TraderData::from_fixture(&trader_fixture(), Some(super::super::DEMO));
        assert_eq!(trader_name(&d.user()), "0x87cd...cde7");
        assert_eq!(d.words(), vec!["Whale".to_string()]);
        let h = d.holdings();
        assert_eq!(h[0].token.as_ref().unwrap().symbol, "WAL");
        assert_eq!(h.len(), 6);
    }
}
