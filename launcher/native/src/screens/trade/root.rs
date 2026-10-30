//! The Trade area's front page (trade/RootPage.tsx): the Market tab's sections (LT/RT) over Home
//! (portfolio, trending, top movers, new launches, perps), Perps (trade/PerpsSection.tsx) and
//! Positions (trade/PositionsSection.tsx). Traders lives in `traders`.

use super::cards::{perp_card, token_card, token_list_row, token_meta};
use super::fmt::*;
use super::model::*;
use super::review::{review_sheet, Review};
use super::ui::{action, coin_logo, Action, Tone, Val};
use crate::kit::format::{num_text, NumKind, NumOptions};
use crate::kit::{cell, glyph, head_row, icon, nav, num, panel, rail, row, section_head, tabs, tile, GSize, Pb, TabItem};
use crate::screens::market::host::*;
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::Id;

/// The four sections along the Market tab's front pages (market/area.ts).
pub const MARKET_SECTIONS: [&str; 4] = ["Collections", "Coins", "Perps", "Traders"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Home,
    Perps,
    Traders,
    Positions,
}

impl Section {
    /// Which of the four tabs shows (Positions counts as Perps).
    pub fn tab(self) -> usize {
        match self {
            Section::Home => 1,
            Section::Perps | Section::Positions => 2,
            Section::Traders => 3,
        }
    }
}

/// The wallet account trading (useTradeAccount).
#[derive(Clone, Debug, Default)]
pub struct Wallet {
    pub address: String,
    pub label: String,
    pub watch: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RootData {
    pub now_ms: f64,
    pub trending: Vec<TokenRow>,
    pub gainers: Vec<TokenRow>,
    pub fresh: Vec<TokenRow>,
    pub markets: Vec<PerpMarket>,
    pub logos: Vec<PerpLogo>,
    pub portfolio: Option<Portfolio>,
    /// None: not loaded; Some(None): no perps account.
    pub account: Option<Option<PerpsAccount>>,
    pub wallet: Option<Wallet>,
    /// The wallet store has loaded (useTradeAccount's `ready`).
    pub ready: bool,
    pub trending_error: bool,
    pub gainers_error: bool,
    pub fresh_error: bool,
    pub perps_error: bool,
    pub portfolio_error: bool,
    pub account_error: bool,
    pub traders: super::traders::TradersData,
}

impl RootData {
    pub fn from_fixture(f: &RootFixture, wallet: Option<Wallet>) -> RootData {
        RootData {
            now_ms: f.now,
            trending: f.trending.items.clone(),
            gainers: f.gainers.items.clone(),
            fresh: f.fresh.items.clone(),
            markets: f.markets.items.clone(),
            logos: f.logos.items.clone(),
            portfolio: if wallet.is_some() { f.portfolio.clone() } else { None },
            account: if wallet.is_some() { Some(f.account.account.clone()) } else { None },
            wallet,
            ready: true,
            ..Default::default()
        }
    }

    /// Perps markets by volume, biggest first.
    pub fn markets_by_volume(&self) -> Vec<&PerpMarket> {
        let mut v: Vec<&PerpMarket> = self.markets.iter().collect();
        v.sort_by(|a, b| b.volume_usd.unwrap_or(0.0).partial_cmp(&a.volume_usd.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    fn logo(&self, m: &PerpMarket) -> Option<String> {
        perp_logo(&self.logos, &m.market_id, &m.symbol).map(str::to_string)
    }

    /// Positions' coins: priced holdings worth a cent or more, and SUI always.
    pub fn coins(&self) -> Vec<&Holding> {
        let sui = super::fmt::normalize_type("0x2::sui::SUI");
        self.portfolio.as_ref().map(|p| p.holdings.iter().filter(|h| h.value_usd >= 0.01 || normalize_type(&h.coin_type) == sui).collect()).unwrap_or_default()
    }

    pub fn positions(&self) -> Vec<&PerpPosition> {
        self.account.as_ref().and_then(|a| a.as_ref()).map(|a| a.positions.iter().collect()).unwrap_or_default()
    }
}

// Focus tags.
const T_SEARCH: u32 = 10;
const T_SWAP: u32 = 20;
const T_SECOND: u32 = 21;
const T_TRENDING: u32 = 1000;
const T_GAINERS: u32 = 2000;
const T_FRESH: u32 = 3000;
const T_PERP_CARD: u32 = 4000;
const T_ALL_MARKETS: u32 = 4999;
const T_PERP_ROW: u32 = 5000;
const T_POS_ROW: u32 = 6000;
const T_POS_CLOSE: u32 = 6500;
const T_COIN_ROW: u32 = 7000;

/// What the app does on A (the Trade stack pushes a page, or the shell switches area or tab).
#[derive(Clone, Debug, PartialEq)]
pub enum Go {
    Token(String, Option<&'static str>),
    Perp(String),
    Swap,
    Search,
    Wallet,
    Collections,
}

impl Go {
    pub fn act(&self) -> String {
        match self {
            Go::Token(t, side) => format!("token:{t}{}", side.map(|s| format!(":{s}")).unwrap_or_default()),
            Go::Perp(m) => format!("perp:{m}"),
            Go::Swap => "swap".into(),
            Go::Search => "search".into(),
            Go::Wallet => "tab:wallet".into(),
            Go::Collections => "area:collections".into(),
        }
    }
}

pub struct RootPage {
    pub data: RootData,
    pub section: Section,
    pub host: Host,
    /// The position whose Close sheet is open.
    pub closing: Option<usize>,
    /// Top traders' window and sort (useBoardPrefs).
    pub board: (super::traders::Window, super::traders::Sort),
}

impl RootPage {
    pub fn new(cx: &mut Cx, parent: Id, data: RootData, section: Section, now: f64) -> RootPage {
        let mut p = RootPage { data, section, host: Host::new(cx, parent), closing: None, board: (super::traders::Window::D7, super::traders::Sort::Pnl) };
        p.render(cx, now);
        p
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let (d, sec, closing, board) = (&self.data, self.section, self.closing, self.board);
        self.host.render(cx, now, |ui| view(ui, d, sec, closing, board));
    }

    pub fn set_data(&mut self, cx: &mut Cx, data: RootData, now: f64) {
        self.data = data;
        self.render(cx, now);
    }

    pub fn set_section(&mut self, cx: &mut Cx, s_: Section, now: f64) {
        if self.section != s_ {
            self.section = s_;
            self.closing = None;
            self.render(cx, now);
        }
    }

    /// LT / RT: the next or previous of the four market sections. None: stays in Trade.
    pub fn step_section(&self, dir: i32) -> Result<Section, Go> {
        let i = (self.section.tab() as i32 + dir).rem_euclid(4) as usize;
        match i {
            0 => Err(Go::Collections),
            1 => Ok(Section::Home),
            2 => Ok(Section::Perps),
            _ => Ok(Section::Traders),
        }
    }

    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        if self.closing.is_some() {
            return vec![("a", "Select".into()), ("b", "Cancel".into())];
        }
        let mut h = vec![("a", "Open".to_string())];
        if self.section == Section::Traders {
            h.push(("x", if self.board.0 == super::traders::Window::D7 { "30 days" } else { "7 days" }.into()));
        }
        h.push(("ltrt", "Sections".into()));
        h.push(("y", "Search".into()));
        h.push(("qam", "Quick settings".into()));
        h
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        if let Some(_i) = self.closing {
            return match b {
                Pad::B => {
                    self.closing = None;
                    self.render(cx, now);
                    Out::Moved
                }
                Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
                Pad::A => match self.host.tag() {
                    Some(super::review::T_CONFIRM) => Out::Act("perp:sign-close".into()),
                    Some(super::review::T_CANCEL) => {
                        self.closing = None;
                        self.render(cx, now);
                        Out::Moved
                    }
                    _ => Out::None,
                },
                _ => Out::None,
            };
        }
        match b {
            Pad::Lt | Pad::Rt => match self.step_section(if b == Pad::Rt { 1 } else { -1 }) {
                Ok(s_) => {
                    self.set_section(cx, s_, now);
                    Out::Moved
                }
                Err(go) => Out::Act(go.act()),
            },
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
            Pad::Y => Out::Act(Go::Search.act()),
            Pad::X if self.section == Section::Traders => {
                use super::traders::Window;
                self.board.0 = if self.board.0 == Window::D7 { Window::D30 } else { Window::D7 };
                self.render(cx, now);
                Out::Act(format!("traders:{}", if self.board.0 == Window::D7 { "7d" } else { "30d" }))
            }
            Pad::A => self.activate(cx, now),
            _ => Out::None,
        }
    }

    fn activate(&mut self, cx: &mut Cx, now: f64) -> Out {
        let Some(tag) = self.host.tag() else { return Out::None };
        let d = &self.data;
        let go = match tag {
            T_SEARCH => Go::Search,
            T_SWAP => Go::Swap,
            T_SECOND => {
                if d.wallet.is_some() {
                    self.set_section(cx, Section::Positions, now);
                    return Out::Moved;
                }
                Go::Wallet
            }
            T_ALL_MARKETS => {
                self.set_section(cx, Section::Perps, now);
                return Out::Moved;
            }
            t if (T_TRENDING..T_TRENDING + 999).contains(&t) => Go::Token(d.trending[(t - T_TRENDING) as usize].coin_type.clone(), None),
            t if (T_GAINERS..T_GAINERS + 999).contains(&t) => Go::Token(d.gainers[(t - T_GAINERS) as usize].coin_type.clone(), None),
            t if (T_FRESH..T_FRESH + 999).contains(&t) => Go::Token(d.fresh[(t - T_FRESH) as usize].coin_type.clone(), None),
            t if (T_PERP_CARD..T_ALL_MARKETS).contains(&t) => Go::Perp(d.markets_by_volume()[(t - T_PERP_CARD) as usize].market_id.clone()),
            t if (T_PERP_ROW..T_POS_ROW).contains(&t) => Go::Perp(d.markets_by_volume()[(t - T_PERP_ROW) as usize].market_id.clone()),
            t if (T_POS_ROW..T_POS_CLOSE).contains(&t) => Go::Perp(d.positions()[(t - T_POS_ROW) as usize].market_id.clone()),
            t if (T_POS_CLOSE..T_COIN_ROW).contains(&t) => {
                self.closing = Some((t - T_POS_CLOSE) as usize);
                self.render(cx, now);
                return Out::Moved;
            }
            t if (super::traders::T_WINDOW..super::traders::T_WINDOW + 2).contains(&t) => {
                self.board.0 = if t == super::traders::T_WINDOW { super::traders::Window::D7 } else { super::traders::Window::D30 };
                self.render(cx, now);
                return Out::Act(format!("traders:{}", if self.board.0 == super::traders::Window::D7 { "7d" } else { "30d" }));
            }
            t if (super::traders::T_SORT..super::traders::T_SORT + 2).contains(&t) => {
                self.board.1 = if t == super::traders::T_SORT { super::traders::Sort::Pnl } else { super::traders::Sort::WinRate };
                self.render(cx, now);
                return Out::Moved;
            }
            t if (super::traders::T_TRADER..super::traders::T_FOLLOWING).contains(&t) => {
                let r = d.traders.ranked(self.board.1);
                return match r.get((t - super::traders::T_TRADER) as usize) {
                    Some(x) => Out::Act(format!("trader:{}", x.address)),
                    None => Out::None,
                };
            }
            t if (super::traders::T_FOLLOWING..super::traders::T_FOLLOWING + 100).contains(&t) => {
                return match d.traders.follows.get((t - super::traders::T_FOLLOWING) as usize) {
                    Some(f) => Out::Act(format!("trader:{}", f.address)),
                    None => Out::None,
                };
            }
            t if t >= T_COIN_ROW => {
                let h = d.coins()[(t - T_COIN_ROW) as usize];
                let sui = normalize_type(&h.coin_type) == normalize_type("0x2::sui::SUI");
                Go::Token(h.coin_type.clone(), Some(if sui { "buy" } else { "sell" }))
            }
            _ => return Out::None,
        };
        Out::Act(go.act())
    }
}

fn step_out(moved: bool) -> Out {
    if moved {
        Out::Moved
    } else {
        Out::None
    }
}

/// RootPage: the sections and the search button over the section's page.
pub fn view(ui: &mut Ui, d: &RootData, sec: Section, closing: Option<usize>, board: (super::traders::Window, super::traders::Sort)) -> El {
    let items: Vec<TabItem> = MARKET_SECTIONS.iter().map(|l| TabItem::new(*l)).collect();
    let lt = glyph(ui, Pb::Lt, GSize::Sm);
    let rt = glyph(ui, Pb::Rt, GSize::Sm);
    let tabs_el = tabs(ui, s(), &items, sec.tab(), true);
    let sections = ui.bx(s().row().items_stretch().gap(1.0).h(2.25)).child(ui.bx(s().row().items_center()).child(lt)).child(tabs_el).child(ui.bx(s().row().items_center()).child(rt));
    let search_icon = icon(ui, "search", 1.1, tok::ICON2);
    let search = ui
        .bx(s().row().items_center().gap(0.6).h(2.5).w(19.0).px(0.9))
        .bg(tok::L1)
        .border(1.0, tok::BORDER_LIGHT)
        .radius(ui.r(0.25))
        .child(search_icon)
        .child(ui.text("Search any coin", T::new(Ty::Small).c(tok::MUTED)).focus_color(tok::FG));
    let search = fill_nav(search, T_SEARCH, Some("Search coins"), None, false);
    let head = ui.bx(s().row().items_center().gap(2.0).px(2.5).pb(1.0)).child(sections).child(ui.bx(s().ml_auto()).group(crate::kit::group("tr-search", false)).child(search));
    let body = match sec {
        Section::Home => home(ui, d),
        Section::Perps => perps(ui, d),
        Section::Positions => positions(ui, d, closing),
        Section::Traders => super::traders::section(ui, &d.traders, board.0, board.1),
    };
    ui.bx(s().fill().col()).child(head).child(ui.bx(s().col().flex(1.0).min_h0()).child(body))
}

fn page(ui: &Ui, name: &str) -> El {
    ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(rare_render::view::key(name))
}

fn quiet(ui: &Ui, text: &str, py: f32) -> El {
    center_text(ui, s().py(py).wp(1.0), text, T::new(Ty::Small).c(tok::MUTED))
}

/// Home: the Trade API's home (portfolio and actions, trending, top movers, new launches, perps).
fn home(ui: &mut Ui, d: &RootData) -> El {
    let mut summary = ui.bx(s().col().min0());
    match &d.wallet {
        Some(w) => {
            summary = summary.child(ui.text("Portfolio", T::new(Ty::Small).c(tok::MUTED)));
            let mut line = ui.bx(s().row().items_end().gap(0.75).mt(0.25));
            match &d.portfolio {
                Some(p) => {
                    if let Some(n) = num(ui, s(), NumKind::Money, Some(p.total_usd), NumOptions { live: true, ..Default::default() }, T::new(Ty::Display).w(600), true) {
                        line = line.child(n);
                    }
                    let mut today = ui.bx(s().row().items_end().gap(0.25).mb(0.3));
                    if let Some(n) = num(ui, s(), NumKind::Pnl, Some(p.change24h_usd), NumOptions::default(), T::new(Ty::Small), true) {
                        today = today.child(n);
                    }
                    if let Some(n) = num(ui, s(), NumKind::Change, Some(p.change24h_pct), NumOptions::default(), T::new(Ty::Small).c(tok::MUTED), false) {
                        today = today.child(n);
                    }
                    line = line.child(today.child(ui.text("today", T::new(Ty::Small).c(tok::MUTED))));
                }
                None => {
                    line = line.child(ui.text(if d.portfolio_error { "Unavailable" } else { "" }, T::new(Ty::Display).w(600).c(tok::MUTED)));
                }
            }
            let who = if w.label.is_empty() { format!("{}...{}", &w.address[..6.min(w.address.len())], &w.address[w.address.len().saturating_sub(4)..]) } else { w.label.clone() };
            summary = summary.child(line).child(ui.text_s(s().mt(0.25), format!("{who}{}", if w.watch { ", watch only" } else { "" }), T::new(Ty::Label).c(tok::MUTED).trunc()));
        }
        None => {
            summary = summary
                .child(ui.text("Trade any coin on Sui", T::new(Ty::Row).w(600)))
                .child(ui.text_s(s().mt(0.25), "Add a Rare Wallet account to buy, sell and open perps from here.", T::new(Ty::Small).c(tok::MUTED)));
        }
    }
    let swap = action(ui, s(), Action::new("Swap", Tone::Primary, T_SWAP));
    let second = action(ui, s(), Action::new(if d.wallet.is_some() { "Positions" } else { "Open Wallet" }, Tone::Secondary, T_SECOND));
    let actions = ui.bx(s().row().gap(0.5).ml_auto()).group(crate::kit::group("tr-actions", true)).child(swap).child(second);
    let top = panel(ui, s().row().items_center().gap(2.0).mx(2.5).px(1.5).py(1.25)).child(summary).child(actions);

    let mut pg = page(ui, "tr-home").child(top);
    // Trending.
    if !d.trending.is_empty() {
        let cards: Vec<El> = d.trending.iter().enumerate().map(|(i, t)| token_card(ui, t, T_TRENDING + i as u32, i == 0)).collect();
        pg = pg.child(rail(ui, s().mt(2.25), "tr-trending", Some("Trending"), None, cards));
    } else if d.trending_error {
        let sec = ui.bx(s().col().mt(2.25)).child(section_head(ui, s().mb(0.9).px(2.5), "Trending", None, None)).child(quiet(ui, "Could not load trending coins. This page refreshes on its own.", 2.0));
        pg = pg.child(sec);
    }
    // Top movers and new launches.
    let movers: Vec<El> = d.gainers.iter().enumerate().map(|(i, t)| token_list_row(ui, t, Some(token_meta(t, false, d.now_ms)), T_GAINERS + i as u32, false)).collect();
    let fresh: Vec<El> = d.fresh.iter().enumerate().map(|(i, t)| token_list_row(ui, t, Some(token_meta(t, true, d.now_ms)), T_FRESH + i as u32, false)).collect();
    let right24 = ui.text("24h", T::new(Ty::Small).c(tok::MUTED));
    let mut left = panel(ui, s().col().p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "Top movers", None, Some(right24))).child(ui.bx(s().col()).group(crate::kit::group("tr-gainers", false)).kids(movers));
    if d.gainers_error && d.gainers.is_empty() {
        left = left.child(quiet(ui, "Could not load movers.", 2.0));
    }
    let mut right = panel(ui, s().col().p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "New launches", None, None)).child(ui.bx(s().col()).group(crate::kit::group("tr-new", false)).kids(fresh));
    if d.fresh_error && d.fresh.is_empty() {
        right = right.child(quiet(ui, "Could not load new launches.", 2.0));
    }
    pg = pg.child(ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(1.25).mx(2.5).mt(2.25).items_start()).child(left).child(right));
    // Perps.
    let markets: Vec<&PerpMarket> = d.markets_by_volume().into_iter().take(12).collect();
    if !markets.is_empty() {
        let mut cards: Vec<El> = markets.iter().enumerate().map(|(i, m)| perp_card(ui, m, d.logo(m).as_deref(), T_PERP_CARD + i as u32)).collect();
        let all = ui.bx(s().row().items_center().justify_center().w(10.0).self_stretch()).bg(tok::BUTTON_SECONDARY).radius(ui.r(0.5)).child(ui.text("All markets", T::new(Ty::Body).w(500)));
        cards.push(nav(tile(all, 1.06), T_ALL_MARKETS, None, None, false));
        pg = pg.child(rail(ui, s().mt(2.25), "tr-perps", Some("Perps"), Some("Long or short with leverage"), cards));
    } else if d.perps_error {
        let sec = ui.bx(s().col().mt(2.25)).child(section_head(ui, s().mb(0.9).px(2.5), "Perps", Some("Long or short with leverage"), None)).child(quiet(ui, "Perps markets are unavailable right now.", 2.0));
        pg = pg.child(sec);
    }
    pg
}

const PERP_COLS: [Track; 7] = [Track::Fr(1.0), Track::Rem(9.0), Track::Rem(6.0), Track::Rem(7.0), Track::Rem(8.0), Track::Rem(7.0), Track::Rem(4.0)];

/// Text in a trow cell: one line, clipped by the cell. (The ellipsis waits on a text-engine fix:
/// truncate currently ellipsizes text that fits its box exactly, see the swarm requests.)
fn cell_t(ui: &Ui, text: impl Into<String>, t: T) -> El {
    ui.text(text, t.tab())
}

/// A coin cell: logo, name over a muted line.
fn coin_cell(ui: &mut Ui, logo: Option<&str>, symbol: &str, sub: &str) -> El {
    let names = ui.bx(s().col().min0()).child(cell_t(ui, symbol, T::new(Ty::Body).w(600))).child(cell_t(ui, sub, T::new(Ty::Label).c(tok::MUTED)));
    let logo = logo_url(logo);
    cell(ui, false).child(ui.bx(s().row().items_center().gap(0.75).min0()).child(coin_logo(ui, logo.as_deref(), symbol, 2.0)).child(names))
}

fn text_cell(ui: &mut Ui, text: String, t: T) -> El {
    cell(ui, true).child(cell_t(ui, text, t))
}

fn num_cell(ui: &mut Ui, kind: NumKind, v: Option<f64>, o: NumOptions, t: T) -> El {
    let c = cell(ui, true);
    match num(ui, s(), kind, v, o, t, true) {
        Some(n) => c.child(n),
        None => c,
    }
}

fn head_cells(ui: &mut Ui, labels: &[&str]) -> Vec<El> {
    labels.iter().enumerate().map(|(i, l)| cell(ui, i > 0).child(cell_t(ui, *l, T::new(Ty::Label).w(500).c(tok::MUTED)))).collect()
}

/// Perps: the account line and every market as a table, biggest volume first.
fn perps(ui: &mut Ui, d: &RootData) -> El {
    let mut pg = page(ui, "tr-perps");
    if let Some(Some(a)) = &d.account {
        let o = NumOptions::default();
        let sl = stat_line(
            ui,
            s(),
            vec![
                ("Perps balance", Stat::Num(NumKind::Money, Some(a.equity_usd), o)),
                ("Available", Stat::Num(NumKind::Money, Some(a.available_usd), o)),
                ("Unrealized P/L", Stat::Num(NumKind::Pnl, Some(a.unrealized_pnl_usd), o)),
                ("Open positions", Stat::Text(a.positions.len().to_string())),
            ],
            true,
            false,
        );
        pg = pg.child(panel(ui, s().col().mx(2.5).mb(1.25).px(1.5).py(1.0)).maybe(sl));
    }
    let head = head_cells(ui, &["Market", "Price", "24h", "Volume", "Open interest", "Funding 1h", "Max"]);
    let mut table = panel(ui, s().col().mx(2.5).p(1.0)).child(head_row(ui, &PERP_COLS, head));
    let mut rows = Vec::new();
    for (i, m) in d.markets_by_volume().into_iter().enumerate() {
        let small = T::new(Ty::Small);
        let cells = vec![
            coin_cell(ui, d.logo(m).as_deref(), &m.symbol, &pair_label(&m.pair)),
            text_cell(ui, perp_px(Some(m.price)), small.w(600)),
            num_cell(ui, NumKind::Change, m.change_pct, NumOptions::default(), small),
            num_cell(ui, NumKind::Market, m.volume_usd, NumOptions::default(), small),
            num_cell(ui, NumKind::Market, m.open_interest_usd, NumOptions::default(), small),
            text_cell(ui, m.funding_rate.map(|f| funding_pct(f, 1.0)).unwrap_or_default(), small.c(tok::MUTED)),
            text_cell(ui, fmt_leverage(m.max_leverage.unwrap_or(1.0)), small),
        ];
        let r = row(ui, &PERP_COLS, T_PERP_ROW + i as u32, Some("Trade"), cells);
        // useLandingFocus: the section's first focusable, not the search above it.
        rows.push(if i == 0 { default_nav(r) } else { r });
    }
    let empty = rows.is_empty();
    table = table.child(ui.bx(s().col()).group(crate::kit::group("tr-perp-table", false)).kids(rows));
    if d.perps_error && empty {
        table = table.child(quiet(ui, "Perps markets are unavailable right now. This page refreshes on its own.", 2.0));
    }
    pg.child(table)
}

const COIN_COLS: [Track; 5] = [Track::Fr(1.0), Track::Rem(9.0), Track::Rem(9.0), Track::Rem(8.0), Track::Rem(6.0)];
const POS_COLS: [Track; 6] = [Track::Fr(1.0), Track::Rem(7.0), Track::Rem(7.0), Track::Rem(7.0), Track::Rem(7.0), Track::Rem(7.0)];

/// Positions: the wallet's coins and its open perps positions, each with a way out.
fn positions(ui: &mut Ui, d: &RootData, closing: Option<usize>) -> El {
    if d.ready && d.wallet.is_none() {
        let e = crate::kit::empty(ui, s(), "No wallet yet", Some("Add a Rare Wallet account on the Wallet tab to see your coins and positions here."), vec![]);
        return ui.bx(s().col().px(2.5)).child(e);
    }
    let pf = d.portfolio.as_ref();
    let acct = d.account.as_ref().and_then(|a| a.as_ref());
    let o = NumOptions::default();
    let sl = stat_line(
        ui,
        s(),
        vec![
            ("Coins", Stat::Num(NumKind::Money, pf.map(|p| p.total_usd), o)),
            ("Today", Stat::Num(NumKind::Pnl, pf.map(|p| p.change24h_usd), o)),
            ("Perps balance", Stat::Num(NumKind::Money, acct.map(|a| a.equity_usd), o)),
            ("Unrealized P/L", Stat::Num(NumKind::Pnl, acct.map(|a| a.unrealized_pnl_usd), o)),
        ],
        true,
        false,
    );
    let mut pg = page(ui, "tr-positions").child(panel(ui, s().col().mx(2.5).px(1.5).py(1.0)).maybe(sl));

    // Perps positions.
    let watch = d.wallet.as_ref().is_some_and(|w| w.watch);
    let right = watch.then(|| ui.text("Watch only", T::new(Ty::Small).c(tok::MUTED)));
    let mut pp = panel(ui, s().col().mx(2.5).mt(1.25).p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "Perps positions", None, right));
    let positions = d.positions();
    if !positions.is_empty() {
        let head = head_cells(ui, &["Market", "Size", "Entry", "Mark", "Liquidation", "P/L"]);
        pp = pp.child(ui.bx(s().mr(6.5)).child(head_row(ui, &POS_COLS, head)));
        for (i, p) in positions.iter().enumerate() {
            let small = T::new(Ty::Small);
            let cells = vec![
                coin_cell(ui, perp_logo(&d.logos, &p.market_id, &p.symbol), &p.symbol, &format!("{} {}", p.side.word(), fmt_leverage(p.leverage))),
                num_cell(ui, NumKind::Money, Some(p.notional_usd), NumOptions { compact: true, ..Default::default() }, small),
                text_cell(ui, perp_px(Some(p.entry_price)), small),
                text_cell(ui, perp_px(p.mark_price), small),
                text_cell(ui, liq_text(p.liquidation_price, p.entry_price), small.c(tok::MUTED)),
                num_cell(ui, NumKind::Pnl, Some(p.pnl_usd), NumOptions { live: true, ..Default::default() }, small.w(600)),
            ];
            let r = row(ui, &POS_COLS, T_POS_ROW + i as u32, Some("Open market"), cells);
            let r = if i == 0 { default_nav(r) } else { r };
            let r = r.with_style(s().grid(&POS_COLS).items_center().h(3.25).gap_xy(0.5, 0.0).px(0.75).flex(1.0).min0().to_style(ui.rem));
            let close = action(ui, s().w(6.0), Action { lg: false, hint: Some("Close position".into()), ..Action::new("Close", Tone::Secondary, T_POS_CLOSE + i as u32) });
            pp = pp.child(ui.bx(s().row().items_center().gap(0.5)).group(crate::kit::group(&format!("tr-pos-{}", p.market_id), true)).child(r).child(close));
        }
    } else {
        let msg = if d.account.is_some() {
            "No open perps positions."
        } else if d.account_error {
            "Could not load perps positions."
        } else {
            ""
        };
        // Quiet's own py-[2rem] wins over the className's py-[1.5rem] (both apply, 2rem sorts later).
        pp = pp.child(quiet(ui, msg, 2.0));
    }
    pg = pg.child(pp);

    // Coins.
    let coins = d.coins();
    let mut cp = panel(ui, s().col().mx(2.5).mt(1.25).p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "Coins", None, None));
    if !coins.is_empty() {
        let head = head_cells(ui, &["Coin", "Balance", "Value", "Price", "24h"]);
        cp = cp.child(head_row(ui, &COIN_COLS, head));
    }
    let mut rows = Vec::new();
    for (i, h) in coins.iter().enumerate() {
        let small = T::new(Ty::Small);
        let cells = vec![
            coin_cell(ui, h.logo.as_deref(), &h.symbol, &h.name),
            text_cell(ui, num_text(NumKind::Amount, Some(h.amount), NumOptions::default()), small),
            num_cell(ui, NumKind::Money, Some(h.value_usd), NumOptions::default(), small.w(600)),
            num_cell(ui, NumKind::Price, h.price_usd, NumOptions::default(), small),
            num_cell(ui, NumKind::Change, h.change24h, NumOptions::default(), small),
        ];
        let r = row(ui, &COIN_COLS, T_COIN_ROW + i as u32, Some("Trade"), cells);
        rows.push(if i == 0 && positions.is_empty() { default_nav(r) } else { r });
    }
    cp = cp.child(ui.bx(s().col()).group(crate::kit::group("tr-coins", false)).kids(rows));
    if pf.is_some() && coins.is_empty() {
        cp = cp.child(quiet(ui, "No coins in this wallet yet.", 2.0));
    }
    if d.portfolio_error && pf.is_none() {
        cp = cp.child(quiet(ui, "Could not load this wallet's coins.", 2.0));
    }
    pg = pg.child(cp);

    if let Some(p) = closing.and_then(|i| positions.get(i)) {
        let r = Review {
            title: format!("Close {} {}", p.symbol, p.side.lower()),
            sub: Some("Market order for the whole position".into()),
            action: "Close position".into(),
            tone: Tone::Primary,
            lines: vec![
                ("Position".into(), Val::t(format!("{} {}, {}", p.side.word(), fmt_leverage(p.leverage), num_text(NumKind::Money, Some(p.notional_usd), NumOptions::default()))), false),
                ("Entry".into(), Val::t(perp_px(Some(p.entry_price))), false),
                ("Mark".into(), Val::t(perp_px(p.mark_price)), false),
                ("P/L if closed now".into(), Val::n(NumKind::Pnl, Some(p.pnl_usd)), true),
                ("Margin back".into(), Val::n(NumKind::Money, Some((p.margin_usd + p.pnl_usd).max(0.0))), false),
            ],
        };
        pg = pg.child(review_sheet(ui, &r, d.wallet.is_some()));
    }
    pg
}

impl Page for RootPage {
    fn any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
    fn section(&self) -> Option<u8> {
        Some(match self.section {
            Section::Home => 0,
            Section::Perps => 1,
            Section::Traders => 2,
            Section::Positions => 3,
        })
    }
}

impl Screen for RootPage {
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

/// The demo watch-only wallet the web screenshots use.
pub fn demo_wallet() -> Wallet {
    Wallet { address: super::DEMO.into(), label: "Demo wallet".into(), watch: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_step_like_the_web() {
        let d = RootData::from_fixture(&trade_root(), Some(demo_wallet()));
        let p = |s_| RootPage { data: d.clone(), section: s_, host: Host { view: rare_render::view::View::new(rare_render::taffy::NodeId::from(0u64)), nav: Default::default(), arts: Default::default(), rem: 24.0, w: 1920.0, h: 1200.0 }, closing: None, board: (super::super::traders::Window::D7, super::super::traders::Sort::Pnl) };
        assert_eq!(p(Section::Home).step_section(1), Ok(Section::Perps));
        assert_eq!(p(Section::Home).step_section(-1), Err(Go::Collections));
        assert_eq!(p(Section::Positions).step_section(1), Ok(Section::Traders));
        assert_eq!(p(Section::Traders).step_section(1), Err(Go::Collections));
        assert_eq!(Go::Token("0x2::sui::SUI".into(), Some("buy")).act(), "token:0x2::sui::SUI:buy");
    }

    #[test]
    fn coins_keep_sui_and_drop_dust() {
        let d = RootData::from_fixture(&trade_root(), Some(demo_wallet()));
        let coins = d.coins();
        assert!(coins.iter().any(|h| h.symbol == "SUI"));
        assert!(coins.iter().all(|h| h.value_usd >= 0.01 || h.symbol == "SUI"));
        assert_eq!(d.markets_by_volume()[0].volume_usd.unwrap_or(0.0) >= d.markets_by_volume()[1].volume_usd.unwrap_or(0.0), true);
    }
}

