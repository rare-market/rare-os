//! The Rare menu's Items, Market and Trophies tabs and the item page (overlay/panels.tsx, data.ts,
//! price.ts, trade.ts, trophies/model.ts). The menu shell calls these to fill the tab body.
//!
//! Hooks the app wires when the backends land:
//! - `set_data(PanelData)` replaces the fixtures with what rare-overlay loaded (items, floors,
//!   market, trophies, the account kind, the item page's offer and phase). The shell rebuilds the
//!   tab after it changes.
//! - `trade::TradeBackend` builds and signs the sale and listing; its result goes back through
//!   `DetailView::phase`.
//!
//! Row tags (every action comes back through the shell as `Out::Act("<tab>:<tag or sid>")`):
//! - items: `tag` = index in `PanelData::items` (sorted by `data::sort_items`), hint "Sell or list";
//!   the shell opens `item_detail` for it.
//! - market: `tag` = index in `PanelData::market`, hint "Buy".
//! - trophies: Rare trophy rows `tag` = index in the sorted rows (no hint), game objects `tag` =
//!   1000 + index in `PanelData::trophies`.
//! - item page (sids): `sell`, `list`, `price:floor`, `price:floor+10`, `price:current`,
//!   `price:other` (type a price), and `add-passkey` (AddPasskey's button).

pub mod data;
mod fixtures;
pub mod money;
pub mod trade;
pub mod trophies;
mod view;

use super::common::{Cx, Nav};
use data::{Item, Trophy};
use rare_render::{Id, Src};
use std::cell::RefCell;
use std::collections::HashMap;
use trade::Offer;
use trophies::{GameSummary, TrophyRow};

pub use fixtures::{fixture_menu, fixture_tab};
pub use view::price_choices;

/// What kind of wallet account is signed in (wallet canSignWithWallet).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Account {
    /// A passkey account: can sign.
    Signer,
    /// Watch-only: asks for a passkey.
    WatchOnly,
    SignedOut,
}

/// The best offer for the open item.
#[derive(Clone, Debug)]
pub enum OfferState {
    /// Still looking (`offer === undefined`).
    Looking,
    Nothing,
    /// An offer Rare cannot fill, with the plain reason.
    Blocked(Offer, String),
    Ready(Offer),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Building(String),
    Signing,
    Error(String),
    Done(String),
}

/// The item page's view state (ItemDetail's useState).
#[derive(Clone, Debug)]
pub struct DetailView {
    pub account: Account,
    pub offer: OfferState,
    /// The chosen listing price; defaults to the floor.
    pub price: Option<i128>,
}

#[derive(Clone, Debug)]
pub struct RareGame {
    pub rows: Vec<TrophyRow>,
    pub sum: GameSummary,
    /// Icons by row key (fixtures; the app resolves URLs through rare-images).
    pub art: HashMap<String, Src>,
}

/// Everything the three tabs and the item page show (the web `Data` plus the hooks' flags).
#[derive(Clone, Debug)]
pub struct PanelData {
    /// Your items, sorted; None until loaded.
    pub items: Option<Vec<Item>>,
    pub market: Option<Vec<Item>>,
    /// The game's own objects (badges, soulbound rewards).
    pub trophies: Vec<Trophy>,
    pub rare: Option<RareGame>,
    /// The Trophies tab's `ready`.
    pub trophies_ready: bool,
    pub game: String,
    pub detail: DetailView,
    pub phase: Phase,
}

thread_local! {
    static DATA: RefCell<PanelData> = RefCell::new(fixtures::scenario("items"));
}

/// Replace what the panels show (call before the shell rebuilds the tab).
pub fn set_data(d: PanelData) {
    DATA.with(|c| *c.borrow_mut() = d);
}

type Pending = Box<dyn Fn() -> Option<PanelData>>;

thread_local! {
    static PENDING: RefCell<Option<Pending>> = RefCell::new(None);
}

/// Data still on its way (a Steam game's achievements): `poll_pending` asks `f` until it answers.
pub fn set_pending(f: Option<Pending>) {
    PENDING.with(|p| *p.borrow_mut() = f);
}

pub fn pending() -> bool {
    PENDING.with(|p| p.borrow().is_some())
}

/// True when the pending data landed (it is the panels' data now).
pub fn poll_pending() -> bool {
    let got = PENDING.with(|p| p.borrow().as_ref().and_then(|f| f()));
    match got {
        Some(d) => {
            set_data(d);
            set_pending(None);
            true
        }
        None => false,
    }
}

/// What the Items and Market tabs show before and after the game's collections load (`None` = still
/// loading). The item page has no offer lookup yet, so it shows none.
pub fn live_data(game: &str, account: Account, items: Option<Vec<Item>>, market: Option<Vec<Item>>) -> PanelData {
    PanelData { items, market, trophies: vec![], rare: None, trophies_ready: true, game: game.into(), detail: DetailView { account, offer: OfferState::Nothing, price: None }, phase: Phase::Idle }
}

pub fn data() -> PanelData {
    DATA.with(|c| c.borrow().clone())
}

pub fn items_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, now: f64) {
    view::items(cx, nav, parent, &data(), now);
}
pub fn market_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, now: f64) {
    view::market(cx, nav, parent, &data(), now);
}
pub fn trophies_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, now: f64) {
    view::trophies(cx, nav, parent, &data(), now);
}
/// The item page for the item at `index` in the fixture list.
pub fn item_detail(cx: &mut Cx, nav: &mut Nav, parent: Id, index: usize, now: f64) {
    view::detail(cx, nav, parent, &data(), index, now);
}
