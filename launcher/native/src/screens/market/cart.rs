//! Cart and sweep (rare.market's cart and Sweep, spec 5.3): listed items gathered from item pages
//! into a cart, or the N cheapest listings of a collection picked by rare.market's sweep quote
//! (`POST /v1/quotes/sweep`, every venue, all-in price). One sheet over the Marketplace shows the
//! lines and the total; Buy builds them with `rare_market::venues::build_cart_buy` in transactions
//! sized to Sui's limits (the quote's chunks, or 40 lines) and hands each to Rare Wallet.

use super::host::*;
use super::parts::*;
use crate::kit::{button, Btn};
use rare_market::api::{ItemCard, Listing};
use rare_market::format::{self as mf, SuiOpts};
use rare_market::venues::{build_cart_buy, load_trade_context, BuyLine};
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::{Id, Node};
use serde_json::Value;
use std::cell::RefCell;
use std::sync::mpsc::{channel, Receiver, Sender};

/// Lines per transaction for a cart (rare.market's SWEEP_CHUNK).
pub const CHUNK: usize = 40;
/// The most a sweep takes (SWEEP_MAX).
pub const SWEEP_MAX: u32 = 200;

thread_local! {
    static CART: RefCell<Vec<ItemCard>> = const { RefCell::new(Vec::new()) };
}

/// Items in the cart (item pages read it for their button).
pub fn count() -> usize {
    CART.with(|c| c.borrow().len())
}

pub fn contains(id: &str) -> bool {
    CART.with(|c| c.borrow().iter().any(|i| i.id == id))
}

/// Adds a listed item, or takes it out when it is in. True when it is in the cart now.
pub fn toggle(item: &ItemCard) -> bool {
    CART.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(i) = c.iter().position(|x| x.id == item.id) {
            c.remove(i);
            false
        } else if item.listing.is_some() {
            c.push(item.clone());
            true
        } else {
            false
        }
    })
}

fn lines_of_cart() -> Vec<ItemCard> {
    CART.with(|c| c.borrow().clone())
}

fn remove(ids: &[String]) {
    CART.with(|c| c.borrow_mut().retain(|i| !ids.contains(&i.id)));
}

fn all_in(i: &ItemCard) -> i128 {
    i.listing.as_ref().map(|l| l.all_in_mist.as_deref().unwrap_or(&l.price_mist).parse::<i128>().unwrap_or(0)).unwrap_or(0)
}

/// The total of `lines`, MIST.
pub fn total_mist(lines: &[ItemCard]) -> i128 {
    lines.iter().map(all_in).sum()
}

/// Chunk sizes for `n` lines: the quote's, else `CHUNK` at a time.
pub fn chunks(n: usize, quoted: &[usize]) -> Vec<usize> {
    if !quoted.is_empty() && quoted.iter().sum::<usize>() == n {
        return quoted.to_vec();
    }
    (0..n).step_by(CHUNK).map(|i| CHUNK.min(n - i)).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// Building transaction i of n.
    Building(usize, usize),
    /// Rare Wallet has transaction i of n.
    Signing(usize, usize),
    Done(usize),
    Error(String),
}

#[derive(Clone, Debug, Default)]
struct Sweep {
    slug: String,
    name: String,
    count: u32,
    lines: Vec<ItemCard>,
    available: Option<u32>,
    chunks: Vec<usize>,
    loading: bool,
    /// The count the shown quote is for; a newer count is asked 200 ms after it settles.
    quoted: Option<u32>,
    due: Option<f64>,
    /// "Spend up to" instead of a count.
    budget_mode: bool,
    /// SUI; 0 is none yet.
    budget: f64,
    /// SUI; 0 is no limit.
    max_price: f64,
}

/// "Spend up to" steps, SUI.
const BUDGET_LADDER: [f64; 14] = [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 50000.0];
/// "Max price" steps, SUI (0 is no limit).
const MAX_LADDER: [f64; 14] = [0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0];

/// The next step up (+1) or down (-1) a ladder; below its first step is 0 (unset).
fn ladder_step(ladder: &[f64], cur: f64, dir: i32) -> f64 {
    if dir > 0 {
        ladder.iter().copied().find(|v| *v > cur + 1e-9).unwrap_or(cur)
    } else {
        ladder.iter().rev().copied().find(|v| *v < cur - 1e-9).unwrap_or(0.0)
    }
}

fn sui_to_mist(v: f64) -> String {
    format!("{}", (v * 1e9).round() as u128)
}

/// rare.market's `POST /v1/quotes/sweep` body for the sweep as set now: a count or a budget, under
/// the max price when there is one.
fn quote_body(sw: &Sweep, buyer: Option<&str>) -> Value {
    let mut body = serde_json::json!({ "collection": sw.slug });
    if sw.budget_mode {
        body["budgetMist"] = Value::String(sui_to_mist(sw.budget));
    } else {
        body["count"] = Value::from(sw.count);
    }
    if sw.max_price > 0.0 {
        body["maxPriceMist"] = Value::String(sui_to_mist(sw.max_price));
    }
    if let Some(b) = buyer {
        body["buyer"] = Value::String(b.to_string());
    }
    body
}

enum Msg {
    Quote(u32, Result<Value, String>),
    Phase(Phase),
    /// Ids bought (out of the cart).
    Bought(Vec<String>),
}

const T_MINUS: u32 = 40_000;
const T_PLUS: u32 = 40_001;
const T_BUY: u32 = 40_002;
const T_CLEAR: u32 = 40_003;
const T_COUNT_MODE: u32 = 40_004;
const T_BUDGET_MODE: u32 = 40_005;
const T_MAX_MINUS: u32 = 40_006;
const T_MAX_PLUS: u32 = 40_007;
const T_ROW: u32 = 40_100;
const SCOPE: u32 = 8;
/// Rows shown before "and N more".
const SHOWN: usize = 4;

pub struct CartSheet {
    node: Id,
    host: Host,
    /// None: closed; Some(None): the cart; Some(Some(_)): a sweep.
    open: Option<Option<Sweep>>,
    phase: Phase,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    account: Option<super::data::Account>,
}

impl CartSheet {
    /// Mounts over `parent` (the Market tab's area), hidden until opened.
    pub fn new(cx: &mut Cx, parent: Id) -> CartSheet {
        let r = cx.r();
        let node = cx.scene.add(parent, s().abs(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).to_style(r), Node::default().z(50));
        cx.scene.n(node).hidden = true;
        let (tx, rx) = channel();
        CartSheet { node, host: Host::new(cx, node), open: None, phase: Phase::Idle, tx, rx, account: None }
    }

    /// Touch: focus on what is under the finger in the sheet.
    pub fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    fn lines(&self) -> Vec<ItemCard> {
        match &self.open {
            Some(Some(sw)) => sw.lines.clone(),
            _ => lines_of_cart(),
        }
    }

    pub fn open_cart(&mut self, cx: &mut Cx, account: Option<super::data::Account>, now: f64) {
        self.account = account;
        self.open = Some(None);
        self.phase = Phase::Idle;
        self.show(cx, now);
    }

    pub fn open_sweep(&mut self, cx: &mut Cx, slug: &str, name: &str, account: Option<super::data::Account>, now: f64) {
        self.account = account;
        self.open = Some(Some(Sweep { slug: slug.to_string(), name: name.to_string(), count: 5, due: Some(now), ..Default::default() }));
        self.phase = Phase::Idle;
        self.show(cx, now);
    }

    fn close(&mut self, cx: &mut Cx, now: f64) {
        self.open = None;
        self.render(cx, now);
        cx.scene.n(self.node).hidden = true;
        cx.scene.mark_dirty();
    }

    fn show(&mut self, cx: &mut Cx, now: f64) {
        cx.scene.n(self.node).hidden = false;
        self.render(cx, now);
        let tag = if matches!(self.open, Some(Some(_))) { T_PLUS } else { T_BUY };
        self.host.focus_tag(cx, tag, now);
    }

    fn busy(&self) -> bool {
        matches!(self.phase, Phase::Building(..) | Phase::Signing(..))
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let Some(open) = self.open.clone() else {
            self.host.render(cx, now, |ui| ui.bx(s()));
            return;
        };
        let lines = self.lines();
        let (phase, signer) = (self.phase.clone(), self.account.as_ref().is_some_and(|a| a.signer));
        self.host.render(cx, now, |ui| view(ui, open.as_ref(), &lines, &phase, signer));
    }

    pub fn hints(&self) -> Vec<(&'static str, String)> {
        let mut h = vec![("a", "Select".to_string())];
        if matches!(self.open, Some(Some(_))) {
            h.push(("ltrt", "Fewer / more".into()));
        }
        h.push(("b", "Close".into()));
        h
    }

    /// The sweep's main value one step: the count, or the budget in budget mode.
    fn step_count(&mut self, d: i32, now: f64) {
        if let Some(Some(sw)) = self.open.as_mut() {
            if sw.budget_mode {
                let b = ladder_step(&BUDGET_LADDER, sw.budget, d);
                if b != sw.budget {
                    sw.budget = b;
                    sw.due = Some(now + 0.2);
                }
                return;
            }
            let max = sw.available.unwrap_or(SWEEP_MAX).min(SWEEP_MAX);
            let n = (sw.count as i64 + d as i64).clamp(1, max.max(1) as i64) as u32;
            if n != sw.count {
                sw.count = n;
                sw.due = Some(now + 0.2);
            }
        }
    }

    fn step_max(&mut self, d: i32, now: f64) {
        if let Some(Some(sw)) = self.open.as_mut() {
            let m = ladder_step(&MAX_LADDER, sw.max_price, d);
            if m != sw.max_price {
                sw.max_price = m;
                sw.due = Some(now + 0.2);
            }
        }
    }

    fn set_mode(&mut self, budget: bool, now: f64) {
        if let Some(Some(sw)) = self.open.as_mut() {
            if sw.budget_mode != budget {
                sw.budget_mode = budget;
                sw.lines.clear();
                sw.chunks.clear();
                sw.available = None;
                sw.due = Some(now + 0.2);
            }
        }
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        match b {
            Pad::B => {
                if !self.busy() {
                    self.close(cx, now);
                }
                Out::Moved
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.host.step(cx, b, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::Lt | Pad::Rt if !self.busy() => {
                self.step_count(if b == Pad::Rt { 1 } else { -1 }, now);
                self.render(cx, now);
                Out::Moved
            }
            Pad::A => {
                let Some(tag) = self.host.tag() else { return Out::None };
                if self.busy() {
                    return Out::None;
                }
                match tag {
                    T_MINUS | T_PLUS => self.step_count(if tag == T_PLUS { 1 } else { -1 }, now),
                    T_MAX_MINUS | T_MAX_PLUS => self.step_max(if tag == T_MAX_PLUS { 1 } else { -1 }, now),
                    T_COUNT_MODE | T_BUDGET_MODE => self.set_mode(tag == T_BUDGET_MODE, now),
                    T_CLEAR => {
                        if let Phase::Done(_) = self.phase {
                            self.close(cx, now);
                            return Out::Moved;
                        }
                        CART.with(|c| c.borrow_mut().clear());
                        self.close(cx, now);
                        return Out::Moved;
                    }
                    T_BUY => {
                        if let Phase::Done(_) = self.phase {
                            self.close(cx, now);
                            return Out::Moved;
                        }
                        if !self.account.as_ref().is_some_and(|a| a.signer) {
                            self.close(cx, now);
                            return Out::Act("wallet:passkey:buy".into());
                        }
                        self.buy();
                    }
                    t if t >= T_ROW => {
                        if matches!(self.open, Some(None)) {
                            if let Some(i) = lines_of_cart().get((t - T_ROW) as usize) {
                                remove(&[i.id.clone()]);
                            }
                            if count() == 0 {
                                self.close(cx, now);
                                return Out::Moved;
                            }
                        }
                    }
                    _ => return Out::None,
                }
                self.render(cx, now);
                Out::Moved
            }
            _ => Out::None,
        }
    }

    fn buy(&mut self) {
        let lines = self.lines();
        let Some(acct) = self.account.clone() else { return };
        if lines.is_empty() {
            return;
        }
        let quoted = match &self.open {
            Some(Some(sw)) => sw.chunks.clone(),
            _ => Vec::new(),
        };
        let sizes = chunks(lines.len(), &quoted);
        self.phase = Phase::Building(1, sizes.len());
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name("rare-market-cart".into())
            .spawn(move || {
                let n = super::feed::net();
                let total = sizes.len();
                let mut at = 0usize;
                let mut bought = 0usize;
                for (i, size) in sizes.into_iter().enumerate() {
                    let chunk = &lines[at..(at + size).min(lines.len())];
                    at += size;
                    let _ = tx.send(Msg::Phase(Phase::Building(i + 1, total)));
                    let types: Vec<&str> = chunk.iter().filter_map(|c| c.listing.as_ref().and_then(|l| l.nft_type.as_deref())).collect();
                    let buy: Vec<BuyLine> = chunk.iter().filter_map(|c| c.listing.clone().map(|l: Listing| BuyLine { item_id: c.id.clone(), listing: l, resale_game: None })).collect();
                    let built = load_trade_context(&*n, &acct.address, &types, None).and_then(|ctx| build_cart_buy(&Default::default(), &buy, &ctx));
                    let t = match built {
                        Ok(t) => t,
                        Err(e) => {
                            let m = if e.0.is_empty() { "Some of these items can't be bought here.".to_string() } else { e.0 };
                            let _ = tx.send(Msg::Phase(if bought > 0 { Phase::Error(format!("Bought {bought}. {m}")) } else { Phase::Error(m) }));
                            return;
                        }
                    };
                    let _ = tx.send(Msg::Phase(Phase::Signing(i + 1, total)));
                    let title = if chunk.len() == 1 { format!("Buy {}", chunk[0].name) } else { format!("Buy {} items", chunk.len()) };
                    match crate::sign::sign_and_execute(t, &acct.address, &title) {
                        Ok(_) => {
                            bought += chunk.len();
                            let _ = tx.send(Msg::Bought(chunk.iter().map(|c| c.id.clone()).collect()));
                        }
                        Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => {
                            let _ = tx.send(Msg::Phase(if bought > 0 { Phase::Done(bought) } else { Phase::Idle }));
                            return;
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Phase(Phase::Error(if bought > 0 { format!("Bought {bought}. Then: {e}") } else { e })));
                            return;
                        }
                    }
                }
                let _ = tx.send(Msg::Phase(Phase::Done(bought)));
            })
            .ok();
    }

    /// Quotes and purchases that moved on. True when items were bought (re-read the pages).
    pub fn tick(&mut self, cx: &mut Cx, now: f64) -> bool {
        if self.open.is_none() {
            return false;
        }
        let mut redraw = self.host.tick(cx, now);
        let mut bought = false;
        // A sweep asks for its quote once the count settles.
        if let Some(Some(sw)) = self.open.as_mut() {
            if sw.due.is_some_and(|d| now >= d) && !sw.loading {
                sw.due = None;
                redraw = true;
                if sw.budget_mode && sw.budget <= 0.0 {
                    // Nothing to ask for until a budget is set.
                    sw.lines.clear();
                    sw.chunks.clear();
                    sw.quoted = Some(0);
                } else {
                let buyer = self.account.as_ref().map(|a| a.address.clone());
                let body = quote_body(sw, buyer.as_deref());
                sw.loading = true;
                let (tx, count) = (self.tx.clone(), sw.count);
                std::thread::Builder::new()
                    .name("rare-market-sweep".into())
                    .spawn(move || {
                        let n = super::feed::net();
                        let url = format!("{}/v1/quotes/sweep", n.market_origin().trim_end_matches('/'));
                        let r = n.call(&rare_market::net::Call::post(url, body.to_string())).map_err(|e| e.to_string()).and_then(|r| if r.ok() { Ok(r.json()) } else { Err(format!("rare.market {}", r.status)) });
                        let _ = tx.send(Msg::Quote(count, r));
                    })
                    .ok();
                }
            }
        }
        while let Ok(m) = self.rx.try_recv() {
            redraw = true;
            match m {
                Msg::Quote(count, r) => {
                    if let Some(Some(sw)) = self.open.as_mut() {
                        sw.loading = false;
                        match r {
                            Ok(v) => {
                                sw.lines = v.get("lines").cloned().and_then(|l| serde_json::from_value(l).ok()).unwrap_or_default();
                                sw.available = v.get("available").and_then(Value::as_u64).map(|a| a as u32);
                                sw.chunks = v.get("chunks").cloned().and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
                                if let Some(name) = v.pointer("/collection/name").and_then(Value::as_str) {
                                    sw.name = name.to_string();
                                }
                                sw.quoted = Some(count);
                                if self.phase != Phase::Idle && !matches!(self.phase, Phase::Building(..) | Phase::Signing(..)) {
                                    self.phase = Phase::Idle;
                                }
                            }
                            Err(e) => {
                                log::warn!("sweep quote: {e}");
                                self.phase = Phase::Error("Couldn't price this sweep right now. Try again.".into());
                            }
                        }
                    }
                }
                Msg::Phase(p) => self.phase = p,
                Msg::Bought(ids) => {
                    remove(&ids);
                    if let Some(Some(sw)) = self.open.as_mut() {
                        sw.lines.retain(|l| !ids.contains(&l.id));
                    }
                    bought = true;
                }
            }
        }
        if redraw {
            self.render(cx, now);
        }
        bought
    }

    pub fn working(&self) -> bool {
        self.busy() || matches!(&self.open, Some(Some(sw)) if sw.loading || sw.due.is_some())
    }
}

fn sui_text(mist: i128) -> String {
    format!("{} SUI", mf::fmt_sui_num(mf::big_to_sui(mist), SuiOpts { dp: 4, ..Default::default() }))
}

fn view(ui: &mut Ui, sweep: Option<&Sweep>, lines: &[ItemCard], phase: &Phase, signer: bool) -> El {
    let total = total_mist(lines);
    let mut body: Vec<El> = Vec::new();
    if let Some(sw) = sweep {
        let pick = |on: bool| if on { V::Primary } else { V::Secondary };
        let modes = ui.bx(s().row().gap(0.5)).child(btn(ui, s(), "Count", T_COUNT_MODE, pick(!sw.budget_mode), Sz::Md, false, false)).child(btn(ui, s(), "Spend up to", T_BUDGET_MODE, pick(sw.budget_mode), Sz::Md, false, false));
        let minus = btn(ui, s(), "-", T_MINUS, V::Secondary, Sz::Md, false, false);
        let plus = btn(ui, s(), "+", T_PLUS, V::Secondary, Sz::Md, true, false);
        let label = if sw.budget_mode {
            if sw.budget > 0.0 {
                format!("Up to {} SUI", mf::fmt_sui_num(sw.budget, SuiOpts { dp: 2, trim: true, ..Default::default() }))
            } else {
                "Set a budget".to_string()
            }
        } else {
            match sw.available {
                Some(a) => format!("{} of {} listed", sw.count, a.min(SWEEP_MAX)),
                None => format!("{} items", sw.count),
            }
        };
        body.push(
            ui.bx(s().row().items_center().gap(0.5))
                .child(modes)
                .child(ui.bx(s().grow()))
                .child(minus)
                .child(ui.bx(s().row().justify_center().w(10.0)).child(ui.text(label, t(BASE).w(600).tab())))
                .child(plus),
        );
        let max_label = if sw.max_price > 0.0 { format!("{} SUI", mf::fmt_sui_num(sw.max_price, SuiOpts { dp: 2, trim: true, ..Default::default() })) } else { "No limit".to_string() };
        let max_minus = btn(ui, s(), "-", T_MAX_MINUS, V::Secondary, Sz::Sm, false, false);
        let max_plus = btn(ui, s(), "+", T_MAX_PLUS, V::Secondary, Sz::Sm, false, false);
        body.push(
            ui.bx(s().row().items_center().gap(0.75).mt(0.75))
                .child(ui.bx(s().w(7.0)).child(ui.text("Max price each", t(SM).c(tok::MUTED))))
                .child(max_minus)
                .child(ui.bx(s().row().justify_center().w(8.0)).child(ui.text(max_label, t(SM).w(600).tab())))
                .child(max_plus),
        );
        body.push(ui.text_s(s().mt(0.5), "The cheapest listings on every marketplace, at their all-in price.", t(SM).c(tok::MUTED).wrap()));
    }
    let mut list = ui.bx(s().col().gap(0.25).mt(if sweep.is_some() { 1.0 } else { 0.0 }));
    for (i, it) in lines.iter().take(SHOWN).enumerate() {
        let art = mimg(ui, s().fill(), it.image_url.as_deref(), &it.name, 2.5, 2.5, Img::w(96.0));
        let thumb = ui.bx(s().size(2.5)).bg(tok::L3).radius(ui.r(0.375)).clip().child(art);
        let coll = it.collection.as_ref().map(|c| c.name.as_str());
        let row = ui
            .bx(s().row().items_center().gap(0.75).px(0.5).py(0.375))
            .radius(ui.r(0.375))
            .child(thumb)
            .child(ui.bx(s().col().grow().min0()).child(ui.text(mf::short_name(&it.name, coll).to_string(), t(SM).w(500).trunc())).child(ui.text(coll.unwrap_or("").to_string(), t(XS).c(tok::MUTED).trunc())))
            .child(ui.text(sui_text(all_in(it)), t(SM).mono()));
        list = list.child(if sweep.is_none() { fill_nav(row, T_ROW + i as u32, Some("Remove"), None, false) } else { row });
    }
    if lines.len() > SHOWN {
        list = list.child(ui.text_s(s().px(0.5).pt(0.25), format!("and {} more", lines.len() - SHOWN), t(SM).c(tok::MUTED)));
    }
    if lines.is_empty() {
        let msg = if sweep.is_some_and(|s| s.budget_mode && s.budget <= 0.0) {
            "Pick a budget to see what it buys."
        } else if sweep.is_some_and(|s| s.loading || s.quoted.is_none()) {
            "Finding the cheapest listings..."
        } else {
            "Nothing listed to buy."
        };
        list = list.child(ui.text(msg, t(SM).c(tok::MUTED)));
    }
    body.push(list);
    body.push(ui.bx(s().row().justify_between().mt(1.0)).child(ui.text("Total", t(BASE).w(600))).child(ui.text(sui_text(total), t(BASE).w(600).mono())));
    body.push(ui.text_s(s().mt(0.25), "Network gas is extra. Items that sell first are left out.", t(XS).c(tok::MUTED)));
    let (line, bad) = match phase {
        Phase::Idle => (None, false),
        Phase::Building(i, n) if *n > 1 => (Some(format!("Getting purchase {i} of {n} ready...")), false),
        Phase::Building(..) => (Some("Getting the purchase ready...".into()), false),
        Phase::Signing(i, n) if *n > 1 => (Some(format!("Review and approve purchase {i} of {n} in Rare Wallet.")), false),
        Phase::Signing(..) => (Some("Review and approve in Rare Wallet.".into()), false),
        Phase::Done(n) => (Some(format!("Bought {n} {}. They're in your Rare Wallet.", if *n == 1 { "item" } else { "items" })), false),
        Phase::Error(e) => (Some(e.clone()), true),
    };
    if let Some(l) = line {
        body.push(ui.text_s(s().mt(0.75), l, t(SM).c(if bad { tok::NEG } else { tok::MUTED }).wrap()));
    }
    let n = lines.len();
    let main_label = match phase {
        Phase::Done(_) => "Done".to_string(),
        Phase::Building(..) => "Getting ready...".into(),
        Phase::Signing(..) => "Waiting for approval".into(),
        _ if !signer => "Add a passkey to buy".into(),
        _ if n == 0 => "Nothing to buy".into(),
        _ => format!("Buy {n} {} for {}", if n == 1 { "item" } else { "items" }, sui_text(total)),
    };
    let main = button(ui, Btn { hint: Some("Buy".into()), grow: true, ..Btn::new(&main_label, T_BUY).primary() });
    let second = button(ui, Btn::new(if sweep.is_some() || matches!(phase, Phase::Done(_)) { "Close" } else { "Clear cart" }, T_CLEAR));
    let title = match sweep {
        Some(sw) => format!("Sweep {}", if sw.name.is_empty() { "this collection" } else { &sw.name }),
        None => "Cart".into(),
    };
    let sub = if sweep.is_none() { Some("A on an item takes it out.") } else { None };
    let sh = crate::kit::sheet(ui, &title, sub, body, vec![main, second], false, SCOPE);
    in_flow_sheet(ui, sh)
}

/// The sweep sheet over fixture listings, for the golden harness (`market-sweep`, and
/// `market-sweep-budget` with the pad script: Right+A flips to budget, Rt steps).
pub struct SweepShot {
    sheet: CartSheet,
}

impl SweepShot {
    pub fn new(cx: &mut Cx, parent: Id, budget: bool, now: f64) -> SweepShot {
        let mut sheet = CartSheet::new(cx, parent);
        sheet.open_sweep(cx, "popkins", "Popkins", Some(super::data::Account { address: "0x1".into(), signer: true, ..Default::default() }), now);
        let lines = super::data::collection().rows::<ItemCard>(&rare_market::api::paths::items("popkins", rare_market::api::Sort::PriceAsc, "")).items;
        if let Some(Some(sw)) = sheet.open.as_mut() {
            sw.lines = lines.into_iter().filter(|l| l.listing.is_some()).take(4).collect();
            sw.due = None;
            sw.quoted = Some(sw.count);
            sw.available = Some(31);
            sw.max_price = 100.0;
            if budget {
                sw.budget_mode = true;
                sw.budget = 200.0;
            }
        }
        sheet.render(cx, now);
        SweepShot { sheet }
    }
}

impl Screen for SweepShot {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.sheet.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.sheet.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        self.sheet.tick(cx, now);
    }
}

impl Page for SweepShot {
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.sheet.hints()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, mist: &str) -> ItemCard {
        ItemCard { id: id.into(), listing: Some(Listing { price_mist: mist.into(), all_in_mist: Some(mist.into()), ..Default::default() }), ..Default::default() }
    }

    #[test]
    fn cart_totals_and_chunks() {
        assert!(toggle(&card("a", "1000000000")));
        assert!(toggle(&card("b", "2500000000")));
        assert!(!toggle(&ItemCard { id: "c".into(), ..Default::default() }), "unlisted items stay out");
        assert_eq!((count(), total_mist(&lines_of_cart())), (2, 3_500_000_000));
        assert!(!toggle(&card("a", "1")));
        assert_eq!(count(), 1);
        assert_eq!(chunks(95, &[]), vec![40, 40, 15]);
        assert_eq!(chunks(5, &[3, 2]), vec![3, 2]);
        assert_eq!(chunks(5, &[3]), vec![5]);
    }

    #[test]
    fn sweep_quote_bodies() {
        let mut sw = Sweep { slug: "rarelings".into(), count: 5, ..Default::default() };
        assert_eq!(quote_body(&sw, None), serde_json::json!({ "collection": "rarelings", "count": 5 }));
        sw.budget_mode = true;
        sw.budget = 25.0;
        sw.max_price = 2.5;
        assert_eq!(quote_body(&sw, Some("0xa")), serde_json::json!({ "collection": "rarelings", "budgetMist": "25000000000", "maxPriceMist": "2500000000", "buyer": "0xa" }));
    }

    #[test]
    fn ladders_step_and_unset() {
        assert_eq!(ladder_step(&BUDGET_LADDER, 0.0, 1), 1.0);
        assert_eq!(ladder_step(&BUDGET_LADDER, 5.0, 1), 10.0);
        assert_eq!(ladder_step(&BUDGET_LADDER, 1.0, -1), 0.0);
        assert_eq!(ladder_step(&MAX_LADDER, 0.5, -1), 0.0);
        assert_eq!(ladder_step(&MAX_LADDER, 10000.0, 1), 10000.0);
    }
}
