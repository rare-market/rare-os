//! Swap any coin for any coin through the router (trade/SwapPage.tsx): pay and receive boxes (A
//! picks the coin from the picker sheet), LT/RT step the amount, X flips the pair, A on the button
//! opens the review.

use super::fmt::*;
use super::model::*;
use super::review::{review_sheet, Review};
use super::ticket::*;
use super::ui::{action, chip, coin_logo, line_el, Action, Tone, Val};
use crate::kit::format::{num_text, NumKind, NumOptions};
use crate::kit::icon::svg_tinted;
use crate::kit::{glyph, num, panel, sheet, GSize, Pb};
use crate::screens::market::host::*;
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::Id;
use std::collections::HashMap;

pub const USDC_TYPE: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";

/// lucide's arrow-down-up.
const ARROW_DOWN_UP: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="m21 8-4-4-4 4"/><path d="M17 4v16"/></svg>"#;

#[derive(Clone, Debug, Default)]
pub struct SwapData {
    pub now_ms: f64,
    /// The pick list (`/ext/tokens?limit=60`).
    pub tokens: Vec<TokenRow>,
    pub portfolio: Option<Portfolio>,
    pub wallet: Option<String>,
    pub balances: HashMap<String, u128>,
    pub quote: Option<Quote>,
    pub quote_error: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapFixture {
    pub now: f64,
    pub tokens: Vec<TokenRow>,
    pub portfolio: Option<Portfolio>,
    pub quote: Option<QuoteAnswer>,
    pub balances: HashMap<String, String>,
}

pub fn swap_fixture() -> SwapFixture {
    serde_json::from_str(include_str!("fixtures/swap.json")).expect("swap fixture")
}

impl SwapData {
    pub fn from_fixture(f: &SwapFixture, wallet: Option<&str>) -> SwapData {
        SwapData {
            now_ms: f.now,
            tokens: f.tokens.clone(),
            portfolio: if wallet.is_some() { f.portfolio.clone() } else { None },
            wallet: wallet.map(str::to_string),
            balances: if wallet.is_some() { f.balances.iter().map(|(k, v)| (normalize_type(k), v.parse().unwrap_or(0))).collect() } else { HashMap::new() },
            quote: f.quote.as_ref().map(|q| Quote { amount_in: q.amount_in.parse().unwrap_or(0), amount_out: q.amount_out.parse().unwrap_or(0), fee_bps: q.fee_bps, hops: q.hops, venues: q.venues.clone(), expires_at: q.expires_at }),
            quote_error: None,
        }
    }

    pub fn coin(&self, coin_type: &str) -> Option<Coin> {
        let t = normalize_type(coin_type);
        self.tokens.iter().find(|x| normalize_type(&x.coin_type) == t).map(|x| Coin { coin_type: t.clone(), symbol: x.symbol.clone(), decimals: x.decimals, logo: x.logo.clone(), price_usd: x.price_usd })
    }

    /// The picker's rows: what the wallet holds (worth a cent) first, then the most traded.
    pub fn picker_rows(&self) -> Vec<TokenRow> {
        let mine: Vec<TokenRow> = self
            .portfolio
            .as_ref()
            .map(|p| {
                p.holdings
                    .iter()
                    .filter(|h| h.value_usd >= 0.01)
                    .map(|h| TokenRow { coin_type: h.coin_type.clone(), symbol: h.symbol.clone(), name: h.name.clone(), decimals: h.decimals, logo: h.logo.clone(), price_usd: h.price_usd, change24h: h.change24h, ..Default::default() })
                    .collect()
            })
            .unwrap_or_default();
        let seen: Vec<String> = mine.iter().map(|m| normalize_type(&m.coin_type)).collect();
        mine.into_iter().chain(self.tokens.iter().filter(|t| !seen.contains(&normalize_type(&t.coin_type))).cloned()).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picking {
    From,
    To,
}

pub struct SwapState {
    pub from: String,
    pub to: String,
    pub picking: Option<Picking>,
    pub reviewing: bool,
    pub ticket: Ticket,
}

const T_FROM: u32 = 10;
const T_TO: u32 = 11;
const T_PRESET: u32 = 300;
const T_MAX: u32 = 350;
const T_FLIP: u32 = 360;
const T_REVIEW: u32 = 500;
const T_PICK: u32 = 2000;
const T_PICK_SEARCH: u32 = 1990;
/// The picker sheet's focus scope.
const PICK_SCOPE: u32 = 4;

pub struct SwapPage {
    pub data: SwapData,
    pub st: SwapState,
    pub host: Host,
}

/// The swap panel's derived words (SwapPage's `out`, `inUsd`, `blocked`, `why`).
#[derive(Clone, Debug, PartialEq)]
pub struct SwapView {
    pub value: String,
    pub balance: String,
    pub in_usd: String,
    pub out: Option<f64>,
    pub out_text: String,
    pub out_usd: String,
    pub blocked: bool,
    pub why: Option<String>,
    pub button: String,
}

pub fn swap_view(d: &SwapData, st: &SwapState) -> SwapView {
    let from = d.coin(&st.from);
    let to = d.coin(&st.to);
    let t = &st.ticket;
    let amount_in = t.amount_in(from.as_ref());
    let out = match (&t.quote, &to) {
        (Some(q), Some(c)) => Some(base_to_number(q.amount_out, c.decimals)),
        _ => None,
    };
    let in_usd = from.as_ref().and_then(|c| c.price_usd.filter(|p| *p != 0.0)).map(|p| base_to_number(amount_in, from.as_ref().map(|c| c.decimals).unwrap_or(9)) * p);
    let out_usd = match (out, to.as_ref().and_then(|c| c.price_usd.filter(|p| *p != 0.0))) {
        (Some(o), Some(p)) => Some(o * p),
        _ => None,
    };
    let blocked = from.is_none() || to.is_none() || amount_in == 0 || t.short(from.as_ref()) || t.quote.is_none();
    let why = if d.wallet.is_none() {
        Some("Add a wallet first".to_string())
    } else if t.short(from.as_ref()) {
        Some("Not enough balance".into())
    } else {
        t.quote_error.clone()
    };
    SwapView {
        value: super::token::trim6(t.value),
        balance: match (&from, t.balance) {
            (Some(c), Some(b)) => format!("Balance {} {}", num_text(NumKind::Amount, Some(base_to_number(b, c.decimals)), NumOptions::default()), c.symbol),
            _ => String::new(),
        },
        in_usd: in_usd.map(|u| num_text(NumKind::Money, Some(u), NumOptions::default())).unwrap_or_default(),
        out,
        out_text: match out {
            Some(o) => num_text(NumKind::Amount, Some(o), NumOptions::default()),
            None if t.quoting => "...".into(),
            None => String::new(),
        },
        out_usd: match out_usd {
            Some(u) => num_text(NumKind::Money, Some(u), NumOptions::default()),
            None if t.quote_error.is_some() => "No route for this amount".into(),
            None => String::new(),
        },
        blocked,
        why,
        button: if from.is_some() && to.is_some() { "Review swap".into() } else { "Swap".into() },
    }
}

impl SwapPage {
    pub fn new(cx: &mut Cx, parent: Id, data: SwapData, from: Option<&str>, to: Option<&str>, now: f64) -> SwapPage {
        let from = normalize_type(from.unwrap_or(SUI_TYPE));
        let to = normalize_type(to.unwrap_or(USDC_TYPE));
        let initial = if from == SUI_TYPE { 5.0 } else { 10.0 };
        let mut st = SwapState { from, to, picking: None, reviewing: false, ticket: Ticket::new(Mode::Units, initial) };
        st.ticket.balance = if data.wallet.is_some() { Some(data.balances.get(&st.from).copied().unwrap_or(0)) } else { None };
        st.ticket.quote = data.quote.clone();
        let mut p = SwapPage { data, st, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let (d, st) = (&self.data, &self.st);
        self.host.render(cx, now, |ui| view(ui, d, st));
    }

    /// New answers (live data): the pair and ticket stay, the balance follows.
    pub fn set_data(&mut self, cx: &mut Cx, data: SwapData, now: f64) {
        self.data = data;
        if self.data.wallet.is_some() {
            self.st.ticket.balance = Some(self.data.balances.get(&self.st.from).copied().unwrap_or(0));
        }
        self.render(cx, now);
    }

    fn flip(&mut self) {
        std::mem::swap(&mut self.st.from, &mut self.st.to);
        self.new_ticket();
    }

    /// A new pair starts the ticket over at its default.
    fn new_ticket(&mut self) {
        let initial = if self.st.from == SUI_TYPE { 5.0 } else { 10.0 };
        self.st.ticket = Ticket::new(Mode::Units, initial);
        self.st.ticket.balance = if self.data.wallet.is_some() { Some(self.data.balances.get(&self.st.from).copied().unwrap_or(0)) } else { None };
        self.st.ticket.quoting = true;
    }

    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        if self.st.picking.is_some() {
            return vec![("a", "Choose".into()), ("b", "Close".into())];
        }
        if self.st.reviewing {
            return vec![("a", "Select".into()), ("b", "Cancel".into())];
        }
        vec![("a", "Select".into()), ("lt", "Less".into()), ("rt", "More".into()), ("x", "Flip".into()), ("b", "Back".into())]
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        if let Some(which) = self.st.picking {
            return match b {
                Pad::B => {
                    self.st.picking = None;
                    self.render(cx, now);
                    self.host.focus_tag(cx, if which == Picking::From { T_FROM } else { T_TO }, now);
                    Out::Moved
                }
                Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
                Pad::A => match self.host.tag() {
                    Some(T_PICK_SEARCH) => Out::Act("osk:search-coin".into()),
                    Some(t) if t >= T_PICK => {
                        let rows = self.data.picker_rows();
                        if let Some(r) = rows.get((t - T_PICK) as usize) {
                            let c = normalize_type(&r.coin_type);
                            let other = if which == Picking::From { &self.st.to } else { &self.st.from };
                            if &c == other {
                                self.flip();
                            } else {
                                match which {
                                    Picking::From => self.st.from = c,
                                    Picking::To => self.st.to = c,
                                }
                                self.new_ticket();
                            }
                        }
                        self.st.picking = None;
                        self.render(cx, now);
                        self.host.focus_tag(cx, if which == Picking::From { T_FROM } else { T_TO }, now);
                        Out::Act("quote".into())
                    }
                    _ => Out::None,
                },
                _ => Out::None,
            };
        }
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
        match b {
            Pad::Lt | Pad::Rt => {
                let from = self.data.coin(&self.st.from);
                self.st.ticket.bump(if b == Pad::Rt { 1 } else { -1 }, from.as_ref());
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
                self.render(cx, now);
                Out::Act("quote".into())
            }
            Pad::X => {
                self.flip();
                self.render(cx, now);
                Out::Act("quote".into())
            }
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => step_out(self.host.step(cx, b, now)),
            Pad::A => self.activate(cx, now),
            _ => Out::None,
        }
    }

    fn activate(&mut self, cx: &mut Cx, now: f64) -> Out {
        let Some(tag) = self.host.tag() else { return Out::None };
        let from = self.data.coin(&self.st.from);
        match tag {
            T_FROM => self.st.picking = Some(Picking::From),
            T_TO => self.st.picking = Some(Picking::To),
            T_FLIP => self.flip(),
            t if (T_PRESET..T_PRESET + 10).contains(&t) => {
                self.st.ticket.value = presets_for(Mode::Units, from.as_ref())[(t - T_PRESET) as usize];
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
            }
            T_MAX => {
                self.st.ticket.value = precision_num(self.st.ticket.max(from.as_ref()), 8);
                self.st.ticket.quote = None;
                self.st.ticket.quoting = true;
            }
            T_REVIEW => {
                if swap_view(&self.data, &self.st).blocked {
                    return Out::None;
                }
                self.st.reviewing = true;
            }
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

/// `CoinButton`: a round pill with the coin's logo and symbol.
fn coin_button(ui: &mut Ui, coin: Option<&Coin>, tag: u32) -> El {
    let mut b = ui.bx(s().row().items_center().gap(0.5).h(3.0).pr(1.0).pl(if coin.is_some() { 0.375 } else { 1.0 })).bg(tok::L3).radius(ui.r(1.5));
    if let Some(c) = coin {
        let logo = logo_url(c.logo.as_deref());
        b = b.child(coin_logo(ui, logo.as_deref(), &c.symbol, 2.25));
    }
    b = b.child(ui.text(coin.map(|c| c.symbol.clone()).unwrap_or_else(|| "Choose a coin".into()), T::new(Ty::Body).w(600)).focus_color(tok::FG));
    fill_nav(b, tag, Some("Change coin"), None, false)
}

pub fn view(ui: &mut Ui, d: &SwapData, st: &SwapState) -> El {
    let sv = swap_view(d, st);
    let from = d.coin(&st.from);
    let to = d.coin(&st.to);
    let t = &st.ticket;

    // You pay.
    let lt = glyph(ui, Pb::Lt, GSize::Md);
    let rt = glyph(ui, Pb::Rt, GSize::Md);
    let amount = right_text(ui, s().flex(1.0).min0(), sv.value.clone(), T::new(Ty::Display).w(600).tab());
    let pay_row = ui.bx(s().row().items_center().gap(0.75).mt(0.5)).child(coin_button(ui, from.as_ref(), T_FROM)).child(lt).child(amount).child(rt);
    let under = ui.bx(s().row().justify_between().mt(0.25)).child(ui.text(sv.balance.clone(), T::new(Ty::Small).c(tok::MUTED))).child(ui.text(sv.in_usd.clone(), T::new(Ty::Small).c(tok::MUTED)));
    let mut chips = ui.bx(s().row().gap(0.5).mt(0.75)).group(crate::kit::group("tr-swap-presets", true));
    for (i, v) in presets_for(Mode::Units, from.as_ref()).iter().enumerate() {
        let c = chip(ui, &super::token::trim6(*v), (t.value - v).abs() < 1e-9, T_PRESET + i as u32);
        chips = chips.child(c);
    }
    let max = t.max(from.as_ref());
    if max.is_finite() && max > 0.0 {
        let c = chip(ui, "Max", (t.value - max).abs() < 1e-9, T_MAX);
        chips = chips.child(c);
    }
    let pay = panel(ui, s().col().mt(1.0).p(1.25)).child(ui.text("You pay", T::new(Ty::Small).c(tok::MUTED))).child(pay_row).child(under).child(chips);

    // The flip button between the two boxes.
    let arrow = svg_tinted(ui, "arrow-down-up", ARROW_DOWN_UP.to_string(), 1.1, 1.1, tok::FG);
    let flip_btn = fill_nav(ui.bx(s().row().items_center().justify_center().size(2.75)).bg(tok::L3).radius(ui.r(1.375)).border(ui.r(0.25), tok::BG).child(arrow), T_FLIP, Some("Flip"), None, false);
    let flip = ui.bx(s().row().justify_center().my(-0.75)).look(rare_render::view::Look::default().z(10)).child(flip_btn);

    // You get about.
    let out = right_text(ui, s().flex(1.0).min0(), sv.out_text.clone(), T::new(Ty::Display).w(600).tab().c(if sv.out.is_some() { tok::FG } else { tok::MUTED }));
    let get_row = ui.bx(s().row().items_center().gap(0.75).mt(0.5)).child(coin_button(ui, to.as_ref(), T_TO)).child(out);
    let get = panel(ui, s().col().p(1.25)).child(ui.text("You get about", T::new(Ty::Small).c(tok::MUTED))).child(get_row).child(right_text(ui, s().mt(0.25).wp(1.0), sv.out_usd.clone(), T::new(Ty::Small).c(tok::MUTED)));

    let mut col = ui.bx(s().col().w(34.0).mx_auto()).child(ui.text("Swap", T::new(Ty::Title).w(600))).child(pay).child(flip).child(get);
    if let (Some(q), Some(c)) = (&t.quote, &to) {
        let min = format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(min_out(q.amount_out), c.decimals)), NumOptions::default()), c.symbol);
        let v1 = Some(ui.text(min, T::new(Ty::Small).tab()));
        let v2 = Some(ui.text(route_text(q), T::new(Ty::Small).tab()));
        let v3 = Some(ui.text(format!("{:.2}%", q.fee_bps as f64 / 100.0), T::new(Ty::Small).tab()));
        let l1 = line_el(ui, "Minimum received", v1, false, false);
        let l2 = line_el(ui, "Route", v2, false, false);
        let l3 = line_el(ui, "Fee", v3, false, true);
        col = col.child(ui.bx(s().col().mt(1.0).px(0.25)).child(l1).child(l2).child(l3));
    }
    let hint = if sv.blocked { sv.why.clone() } else { Some("Review".into()) };
    col = col.child(action(ui, s().mt(1.0), Action { def: true, full: true, disabled: sv.blocked, hint, ..Action::new(sv.button.clone(), Tone::Primary, T_REVIEW) }));
    if let Some(w) = sv.why.as_ref().filter(|_| !t.quoting) {
        col = col.child(center_text(ui, s().mt(0.5).wp(1.0), w.clone(), T::new(Ty::Label).c(tok::MUTED)));
    }
    let mut page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(rare_render::view::key("tr-swap-scroll")).child(col);

    if let Some(which) = st.picking {
        page = page.child(picker(ui, d, which));
    }
    if st.reviewing {
        if let (Some(f), Some(tc)) = (&from, &to) {
            let amt = base_to_number(t.amount_in(Some(f)), f.decimals);
            let mut lines = vec![("You pay".to_string(), Val::t(format!("{} {}", num_text(NumKind::Amount, Some(amt), NumOptions::default()), f.symbol)), false)];
            if let Some(q) = &t.quote {
                lines.push(("You get about".into(), Val::t(format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(q.amount_out, tc.decimals)), NumOptions::default()), tc.symbol)), true));
                lines.push(("Minimum received".into(), Val::t(format!("{} {}", num_text(NumKind::Amount, Some(base_to_number(min_out(q.amount_out), tc.decimals)), NumOptions::default()), tc.symbol)), false));
                lines.push(("Fee".into(), Val::t(format!("{:.2}%", q.fee_bps as f64 / 100.0)), false));
                lines.push(("Slippage".into(), Val::t("1%"), false));
            }
            let r = Review { title: format!("Swap {} to {}", f.symbol, tc.symbol), sub: None, action: "Confirm swap".into(), tone: Tone::Primary, lines };
            page = page.child(review_sheet(ui, &r, d.wallet.is_some()));
        }
    }
    page
}

/// TokenPicker: coins the wallet holds first, then the most traded; a search field on top.
fn picker(ui: &mut Ui, d: &SwapData, which: Picking) -> El {
    let held: HashMap<String, &Holding> = d.portfolio.as_ref().map(|p| p.holdings.iter().map(|h| (normalize_type(&h.coin_type), h)).collect()).unwrap_or_default();
    let search_icon = crate::kit::icon(ui, "search", 1.2, tok::ICON2);
    let search = fill_nav(
        ui.bx(s().row().items_center().gap(0.6).h(3.0).wp(1.0).px(0.9)).bg(tok::L1).border(1.0, tok::BORDER_LIGHT).radius(ui.r(0.25)).child(search_icon).child(ui.text("Search any coin by name, ticker or coin type", T::new(Ty::Body).c(tok::MUTED))),
        T_PICK_SEARCH,
        Some("Search coins"),
        None,
        false,
    );
    let mut list = ui.bx(s().col().mx(-0.75)).group(crate::kit::group("tr-picker", false));
    for (i, r) in d.picker_rows().iter().enumerate() {
        let logo = logo_url(r.logo.as_deref());
        let names = ui.bx(s().col().flex(1.0).min0()).child(ui.text(r.symbol.clone(), T::new(Ty::Body).w(600).trunc())).child(ui.text(r.name.clone(), T::new(Ty::Label).c(tok::MUTED).trunc()));
        let mut right = ui.bx(s().col().items_end());
        match held.get(&normalize_type(&r.coin_type)) {
            Some(h) => {
                right = right.child(ui.text(num_text(NumKind::Amount, Some(h.amount), NumOptions::default()), T::new(Ty::Small).tab()));
                right = right.maybe(num(ui, s(), NumKind::Money, Some(h.value_usd), NumOptions::default(), T::new(Ty::Label).c(tok::MUTED), true));
            }
            None => right = right.maybe(num(ui, s(), NumKind::Price, r.price_usd, NumOptions::default(), T::new(Ty::Small).c(tok::MUTED), true)),
        }
        let row = ui.bx(s().row().items_center().gap(0.75).h(3.5).wp(1.0).px(0.75)).radius(ui.r(0.5)).child(coin_logo(ui, logo.as_deref(), &r.symbol, 2.5)).child(names).child(right);
        let row = crate::kit::nav(crate::kit::row_look(row), T_PICK + i as u32, None, None, i == 0).key(rare_render::view::key(&r.coin_type));
        list = list.child(row);
    }
    let body = vec![ui.bx(s().col().mb(0.75)).group(crate::kit::group("tr-picker-search", false)).child(search), list];
    let sh = sheet(ui, if which == Picking::From { "Pay with" } else { "Receive" }, None, body, vec![], false, PICK_SCOPE);
    in_flow_sheet(ui, sh)
}

impl Page for SwapPage {
    fn any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for SwapPage {
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

    #[test]
    fn swap_words_match_the_web() {
        let d = SwapData::from_fixture(&swap_fixture(), Some(super::super::DEMO));
        let mut st = SwapState { from: SUI_TYPE.into(), to: normalize_type(USDC_TYPE), picking: None, reviewing: false, ticket: Ticket::new(Mode::Units, 5.0) };
        st.ticket.balance = d.balances.get(SUI_TYPE).copied();
        st.ticket.quote = d.quote.clone();
        let v = swap_view(&d, &st);
        assert_eq!(v.value, "5");
        assert_eq!(v.balance, "Balance 57.5 SUI");
        assert_eq!(v.in_usd, "$4.62");
        assert_eq!(v.out_text, "4.61");
        assert_eq!(v.out_usd, "$4.61");
        assert_eq!(v.button, "Review swap");
        assert!(!v.blocked);
        let rows = d.picker_rows();
        assert_eq!(rows[0].symbol, "SUI");
    }
}
