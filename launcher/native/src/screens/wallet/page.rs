//! The Wallet tab as an app page (WalletScreen.tsx): the page stack over the accounts this device
//! holds (`<data dir>/rare/wallet/accounts.json`, the list the web launcher and rare-wallet keep).
//! No accounts: Welcome. B pops a page; LT/RT switch home's tabs, Y opens Accounts, X selects.
//! Pages whose flows are not wired to rare-wallet yet say so in a toast instead of opening.

use super::home::fixtures as fx_home;
use super::home::lib::Hint as WHint;
use super::home::page::{self as home, ActivityLoad, PortfolioLoad};
use super::home::stack::{HomeTab, Stack, WalletView};
use super::live::{self, Answer};
use crate::screens::inventory::{sheet as inv_sheet, view as inv_view};
use rare_market::inventory::Inventory;
use std::sync::mpsc::Receiver;
use super::markup::{div, El as M};
use super::pages::{accounts, coin, receive, settings, watch, welcome};
use super::render::{build, Built};
use super::types::{HdInfo, RareAccount};
use crate::act::Act;
use crate::kit::Ui;
use crate::page::{hash, Acts, Btn, Fx, Page};
use crate::state::State;
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::Focusable;
use rare_render::view::El;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Home(home::Msg),
    Welcome(welcome::Msg),
    Receive(receive::Msg),
    Accounts(accounts::Msg),
    Settings(settings::Msg),
    Watch(watch::Msg),
    Coin(coin::Msg),
    Item(inv_sheet::Msg),
    /// The flows wired to rare-wallet (`flows/`).
    Flow(super::flows::Msg),
}

pub struct WalletTab {
    pub(super) accounts: Vec<RareAccount>,
    pub(super) selected: Option<String>,
    pub(super) pf: PortfolioLoad,
    pub(super) act: ActivityLoad,
    pub(super) stack: Stack,
    pub(super) home: home::State,
    pub(super) accts: accounts::State,
    pub(super) settings: settings::State,
    pub(super) watch: watch::State,
    pub(super) copied: bool,
    /// the cross-game inventory (Collectibles); None before it loaded
    pub(super) inv: Option<Inventory>,
    pub(super) inv_error: bool,
    /// the item sheet over home: item id and its state
    pub(super) item: Option<(String, inv_sheet::State)>,
    /// live reads in flight and the address they are for
    pub(super) rx: Vec<Receiver<Answer>>,
    /// portfolio total per account for the Accounts page, filled in as reads answer
    pub(super) acct_values: BTreeMap<String, Option<f64>>,
    pub(super) loaded_for: Option<String>,
    pub(super) cursor: Option<String>,
    /// The app-level wallet data's version this page last read (`crate::data::wallet`).
    pub(super) data_ver: u64,
    /// Older activity pages were added here past the first one the app keeps.
    pub(super) act_extra: bool,
    /// An older activity page is on its way.
    pub(super) more_flight: bool,
    pub(super) fixture: bool,
    pub(super) built: Built<Msg>,
    pub(super) ver: u64,
    /// the wallet service's version the account list was read at
    pub(super) wallet_ver: u64,
    pub(super) now: i64,
    /// Each flow's own state (`flows/`).
    pub(super) flows: super::flows::Flows,
    /// Receive's Verify on Ledger: the last answer, and the check on its way.
    pub(super) verify: Option<receive::Verify>,
    pub(super) verify_rx: Option<Receiver<Result<Result<bool, super::onboard::hardware::HwErr>, String>>>,
}

/// The accounts this device holds, from rare-wallet's keystore (public data only).
pub fn load_accounts() -> (Vec<RareAccount>, Option<String>) {
    if let Some(w) = super::service::wallet() {
        let (a, sel) = w.accounts();
        return (a.iter().map(RareAccount::from_wallet).collect(), sel);
    }
    load_accounts_file()
}

/// The account list file, read directly (when the wallet service is not up).
pub(crate) fn load_accounts_file() -> (Vec<RareAccount>, Option<String>) {
    let path = rare_core::paths::data_dir().join("wallet").join("accounts.json");
    let Ok(raw) = std::fs::read(&path) else { return (Vec::new(), None) };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) else { return (Vec::new(), None) };
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
    let list = v.get("accounts").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let accounts = list
        .iter()
        .filter(|a| a.get("address").is_some())
        .map(|a| {
            let mut r = RareAccount::new(&s(a, "address"), &s(a, "label"), &s(a, "kind"));
            r.hd = a.get("hd").filter(|h| h.is_object()).map(|h| HdInfo { index: h.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32, phrase_id: s(h, "phraseId") });
            r
        })
        .collect();
    (accounts, v.get("selected").and_then(|x| x.as_str()).map(str::to_string))
}

thread_local! {
    /// A page another screen asked the Wallet tab to open (Sign in's choices), taken on its next build.
    static OPEN_ON: std::cell::RefCell<Option<WalletView>> = const { std::cell::RefCell::new(None) };
    static OPEN_SEQ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Settings > Accounts' Sign out: the Settings page opens with its sign-out sheet up.
    static SIGN_OUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// An offer notice's item and the price (MIST) it announced: sell into it once the sheet has its quote.
    static ACCEPT: std::cell::RefCell<Option<(String, Option<String>)>> = const { std::cell::RefCell::new(None) };
    /// A page is open over the wallet's home (set on every build).
    static FLOW_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A page is open over the wallet's home, or one was asked for and not built yet: first run waits
/// for this to end (the account was added, or B left the flow) before it comes back.
pub fn flow_open() -> bool {
    OPEN_ON.with(|o| o.borrow().is_some()) || FLOW_OPEN.with(|f| f.get())
}

/// Opens the item's sheet and, when the best offer is still the announced one, sells into it
/// (the wallet's review sheet comes up). Otherwise the sheet stays with what is best now.
pub fn open_accept(id: String, announced_raw: Option<String>) {
    ACCEPT.with(|a| *a.borrow_mut() = Some((id.clone(), announced_raw)));
    open_on(WalletView::Nft { id });
}

pub(super) fn pending_accept(id: &str) -> Option<Option<String>> {
    ACCEPT.with(|a| {
        let mut a = a.borrow_mut();
        if a.as_ref().is_some_and(|(i, _)| i == id) {
            a.take().map(|(_, r)| r)
        } else {
            None
        }
    })
}

pub(super) fn has_accept(id: &str) -> bool {
    ACCEPT.with(|a| a.borrow().as_ref().is_some_and(|(i, _)| i == id))
}

/// Opens the wallet's Settings with the sign-out sheet for the selected account.
pub fn open_sign_out() {
    SIGN_OUT.with(|s| s.set(true));
    open_on(WalletView::Settings);
}

/// Opens the Wallet tab's stack on `v` (over home) the next time it builds.
pub fn open_on(v: WalletView) {
    OPEN_ON.with(|o| *o.borrow_mut() = Some(v));
    OPEN_SEQ.with(|s| s.set(s.get() + 1));
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl Default for WalletTab {
    fn default() -> Self {
        let fixture = std::env::var("RR_FIXTURE").is_ok();
        let inv = fixture.then(crate::screens::inventory::fixtures::inventory);
        let (accounts, selected, pf, act, now) = if fixture {
            let a = fx_home::account();
            // RR_WALLET_EMPTY: the Welcome page (no accounts yet)
            let empty = std::env::var("RR_WALLET_EMPTY").is_ok();
            (
                if empty { Vec::new() } else { vec![a.clone()] },
                (!empty).then_some(a.address),
                PortfolioLoad { data: Some(fx_home::portfolio()), ..Default::default() },
                ActivityLoad { items: fx_home::activity(), ready: true, done: true, ..Default::default() },
                fx_home::NOW,
            )
        } else {
            let (a, s) = load_accounts();
            (a, s, PortfolioLoad::default(), ActivityLoad::default(), now_ms())
        };
        let mut stack = Stack::default();
        if let Ok(wv) = std::env::var("RR_WALLET") {
            stack.reset(Stack::from_query(&wv));
        }
        WalletTab {
            accounts,
            selected,
            pf,
            act,
            stack,
            home: home::State::new(None),
            accts: accounts::State::default(),
            settings: settings::State { open: None, auto: settings::auto_lock(None) },
            watch: watch::State::default(),
            copied: false,
            inv,
            inv_error: false,
            item: None,
            rx: Vec::new(),
            acct_values: BTreeMap::new(),
            loaded_for: None,
            cursor: None,
            data_ver: 0,
            act_extra: false,
            more_flight: false,
            fixture,
            built: Built::default(),
            ver: 0,
            wallet_ver: 0,
            now,
            flows: Default::default(),
            verify: None,
            verify_rx: None,
        }
    }
}

fn key_of(b: &str) -> Option<Key> {
    Some(match b {
        "a" => Key::A,
        "b" => Key::B,
        "start" => Key::Start,
        "qam" => Key::Quick,
        "x" => Key::X,
        "y" => Key::Y,
        "lt" => Key::Lt,
        "rt" => Key::Rt,
        _ => return None,
    })
}

impl WalletTab {
    /// Re-reads the accounts when the wallet changed (added, removed, renamed, selected).
    pub(super) fn sync_wallet(&mut self) {
        if self.fixture {
            return;
        }
        let Some(w) = super::service::wallet() else { return };
        let v = w.version();
        if v == self.wallet_ver {
            return;
        }
        self.wallet_ver = v;
        let (a, s) = load_accounts();
        if a != self.accounts || s != self.selected {
            self.accounts = a;
            self.selected = s;
            self.ver += 1;
        }
    }

    pub(super) fn current(&self) -> Option<&RareAccount> {
        self.selected.as_ref().and_then(|s| self.accounts.iter().find(|a| &a.address == s)).or(self.accounts.first())
    }

    pub(super) fn has_phrase(&self) -> bool {
        self.current().is_some_and(|c| c.hd.is_some())
    }

    pub(super) fn markup(&self) -> M<Msg> {
        let cur = self.current();
        let can_sign = cur.is_some_and(|a| a.kind != "watch");
        match &self.stack.top().view {
            WalletView::Home { .. } if self.accounts.is_empty() => welcome::welcome_view().map(&Msg::Welcome),
            WalletView::Home { .. } => {
                let pr = home::Props { account: cur, can_sign, pf: &self.pf, act: &self.act, now: self.now, tz_min: 0 };
                let inv = || {
                    let loading = self.inv.is_none() && !self.inv_error;
                    inv_view::view(self.inv.as_ref(), loading, self.inv_error, self.home.selecting, &self.home.selected).map(&|m| match m {
                        inv_view::Msg::Open(id) => home::Msg::Push(WalletView::Nft { id }),
                        inv_view::Msg::Toggle(id) => home::Msg::Toggle(id),
                        inv_view::Msg::Nothing => home::Msg::Tab(HomeTab::Collectibles),
                    })
                };
                let page = home::view(&pr, &self.home, inv).map(|m| m.map(&Msg::Home)).unwrap_or_else(|| welcome::welcome_view().map(&Msg::Welcome));
                match &self.item {
                    Some((id, st)) => {
                        let item = self.find_item(id);
                        let sp = inv_sheet::Props { item: Some(item), quote: self.item_quote(id), can_sign, signed_out: self.accounts.is_empty() };
                        div("").kid(page).kid(inv_sheet::view(&sp, st).map(&Msg::Item))
                    }
                    None => page,
                }
            }
            WalletView::Add => {
                let (busy, error) = self.derive_state();
                welcome::add_view(&welcome::AddProps { none: self.accounts.is_empty(), has_phrase: self.has_phrase(), busy, error })
            }.map(&Msg::Welcome),
            WalletView::Receive => receive::view(cur, self.copied, self.verify.as_ref()).map(|m| m.map(&Msg::Receive)).unwrap_or_else(|| div("")),
            WalletView::Accounts => {
                let page = accounts::view(&accounts::Props { accounts: &self.accounts, current: cur, values: &self.acct_values, has_phrase: self.has_phrase() }, &self.accts).map(&Msg::Accounts);
                match self.manage_remove_sheet() {
                    Some(sheet) => div("").kid(page).kid(sheet),
                    None => page,
                }
            }
            WalletView::Settings => {
                let pr = settings::Props { account: cur, vault: self.flows.manage.vault.as_ref(), has_phrase: self.has_phrase(), origins: &self.flows.manage.origins, keys_here: self.accounts.iter().any(|a| a.kind == "native" || a.kind == "passkey") };
                settings::view(&pr, &self.settings).map(&Msg::Settings)
            }
            WalletView::Watch => watch::view(&self.watch).map(&Msg::Watch),
            WalletView::Coin { coin_type } => coin::view(&coin::Props { coin_type, can_sign, pf: &self.pf, act: &self.act, tz_min: 0 }).map(&Msg::Coin),
            v => self.flows_markup(v).map(|m| m.map(&Msg::Flow)).unwrap_or_else(|| div("")),
        }
    }

    pub(super) fn find_item(&self, id: &str) -> Option<&rare_market::inventory::InvItem> {
        self.inv.as_ref()?.groups.iter().flat_map(|g| g.items.iter()).find(|i| i.id == id)
    }

    /// Shows the current account's data from the app-level store (`crate::data::wallet`): the last
    /// values at once, whatever the background refresh brings in place. Nothing is cleared on the
    /// way in, so coming back to the wallet never blanks or reloads what it showed.
    pub(super) fn ensure_loaded(&mut self) {
        if self.fixture {
            return;
        }
        let Some(addr) = self.current().map(|a| a.address.clone()) else { return };
        if self.loaded_for.as_deref() != Some(addr.as_str()) {
            self.loaded_for = Some(addr.clone());
            self.data_ver = 0;
            self.act = ActivityLoad::default();
            self.cursor = None;
            self.act_extra = false;
            self.more_flight = false;
            crate::data::wallet::tick(&addr, rare_render::clock::secs());
        }
        self.pull(&addr);
    }

    /// Takes what the app-level store holds for `addr` when it changed.
    fn pull(&mut self, addr: &str) {
        let v = crate::data::wallet::version();
        if v == self.data_ver {
            return;
        }
        self.data_ver = v;
        let h = crate::data::wallet::get(addr);
        self.pf = PortfolioLoad { loading: h.pf.is_none() && !h.pf_err, error: h.pf_err, data: h.pf };
        match h.act {
            Some(first) => {
                if self.act_extra {
                    // Older pages stay below the fresh first page.
                    let older: Vec<_> = self.act.items.drain(..).filter(|i| !first.iter().any(|f| f.digest == i.digest)).collect();
                    self.act.items = first;
                    self.act.items.extend(older);
                } else {
                    self.act.items = first;
                    self.cursor = h.cursor;
                }
                self.act.ready = true;
                self.act.done = self.cursor.is_none();
                self.act.error = false;
                self.act.loading = self.more_flight;
            }
            None => {
                self.act.loading = !h.act_err || self.more_flight;
                self.act.error = h.act_err;
            }
        }
        self.inv = h.inv;
        self.inv_error = h.inv_err;
        self.ver += 1;
    }

    pub(super) fn take(&mut self, a: Answer) {
        let cur = self.loaded_for.clone().unwrap_or_default();
        match a {
            // Older activity pages (`more`); the first page comes from the app-level store.
            Answer::Activity(addr, r) if addr == cur => {
                self.more_flight = false;
                self.act.loading = false;
                match r {
                    Ok((items, cursor)) => {
                        self.act.items.extend(items.into_iter().filter(|i| !self.act.items.iter().any(|x| x.digest == i.digest)).collect::<Vec<_>>());
                        self.act.ready = true;
                        self.act.done = cursor.is_none();
                        self.act.error = false;
                        self.cursor = cursor;
                        self.act_extra = true;
                    }
                    Err(e) => {
                        log::warn!("wallet activity: {e}");
                        self.act.error = true;
                    }
                }
            }
            Answer::AccountValue(addr, v) => {
                crate::data::wallet::take_value(&addr, v);
                if v.is_some() || !self.acct_values.contains_key(&addr) {
                    self.acct_values.insert(addr, v);
                }
            }
            _ => {}
        }
        self.ver += 1;
    }

    pub(super) fn more(&mut self) {
        if self.fixture || self.act.loading {
            return;
        }
        let (Some(addr), Some(c)) = (self.loaded_for.clone(), self.cursor.clone()) else { return };
        self.act.loading = true;
        self.more_flight = true;
        self.rx.push(live::more(&addr, &c));
    }

    pub(super) fn wired(v: &WalletView) -> bool {
        matches!(v, WalletView::Home { .. } | WalletView::Add | WalletView::Receive | WalletView::Accounts | WalletView::Settings | WalletView::Watch | WalletView::Coin { .. }) || Self::flows_wired(v)
    }

    pub(super) fn push(&mut self, st: &mut State, v: WalletView) {
        if let WalletView::Nft { id } = &v {
            if matches!(self.stack.top().view, WalletView::Home { .. }) {
                self.item = Some((id.clone(), inv_sheet::State::default()));
                return;
            }
        }
        if !Self::wired(&v) {
            st.toast("This part of the wallet is not in the native launcher yet", crate::state::Tone::Info, rare_render::clock::secs());
            return;
        }
        match v {
            WalletView::Accounts => {
                self.accts = accounts::State::default();
                if !self.fixture {
                    // The last totals at once; the reads below change them in place.
                    for a in &self.accounts {
                        if let Some(v) = crate::data::wallet::get(&a.address).total_usd() {
                            self.acct_values.insert(a.address.clone(), Some(v));
                        }
                    }
                    let addrs: Vec<String> = self.accounts.iter().map(|a| a.address.clone()).collect();
                    self.rx.push(live::values(&addrs));
                }
            }
            WalletView::Settings => self.settings.open = None,
            WalletView::Watch => self.watch = watch::State::default(),
            WalletView::CreatePhrase | WalletView::Import { .. } => self.onboard_reset(),
            WalletView::Receive => {
                self.copied = false;
                self.verify = None;
            }
            _ => {}
        }
        self.stack.push(v, None);
    }

    pub(super) fn page_hints(&self) -> Vec<WHint> {
        let cur = self.current();
        if self.item.is_some() {
            return vec![WHint { b: "a", label: "Select".into() }, WHint { b: "b", label: "Close".into() }];
        }
        match &self.stack.top().view {
            WalletView::Home { .. } if self.accounts.is_empty() => welcome::welcome_hints(),
            WalletView::Home { .. } => {
                let pr = home::Props { account: cur, can_sign: cur.is_some_and(|a| a.kind != "watch"), pf: &self.pf, act: &self.act, now: self.now, tz_min: 0 };
                home::hints(&pr, &self.home)
            }
            WalletView::Accounts => accounts::hints(),
            WalletView::Watch => watch::hints(),
            WalletView::Receive => receive::hints(),
            WalletView::Settings => settings::hints(),
            WalletView::Coin { .. } => coin::hints(),
            WalletView::Nft { .. } => super::pages::nft::hints(),
            WalletView::Activity { .. } => super::pages::activity::hints(),
            WalletView::SendNfts { .. } => super::pages::send_nfts::hints(),
            WalletView::Send { .. } => self.send_hints().unwrap_or_default(),
            WalletView::Passkey { .. } => self.passkey_hints(),
            WalletView::Ledger | WalletView::Keystone => self.hardware_hints(),
            WalletView::Playtron => self.playtron_hints(),
            WalletView::PinAccount => self.pin_account_hints(),
            WalletView::MoveAll { .. } => self.move_all_hints(),
            v => self.onboard_hints(v).unwrap_or_default(),
        }
    }

    pub(super) fn handle(&mut self, st: &mut State, fx: &mut Fx, m: Msg) {
        let now = rare_render::clock::secs();
        match m {
            Msg::Home(m) => match home::update(&mut self.home, m) {
                Some(home::Effect::Push(v)) => self.push(st, v),
                Some(home::Effect::Copy(a)) => {
                    super::flows::manage::copy_text(&a);
                    st.toast("Address copied", crate::state::Tone::Info, now);
                }
                Some(home::Effect::More) => self.more(),
                Some(home::Effect::Retry) => {
                    if self.act.ready {
                        self.more();
                    } else if let Some(a) = self.loaded_for.clone() {
                        crate::data::wallet::refresh(&a);
                    }
                }
                None => {}
            },
            Msg::Item(m) => self.items_sheet(st, m),
            Msg::Welcome(welcome::Msg::Push(v)) => self.push(st, v),
            Msg::Welcome(welcome::Msg::Derive) => self.derive_next_account(st),
            Msg::Receive(receive::Msg::Copy(a)) => {
                super::flows::manage::copy_text(&a);
                self.copied = true;
                st.toast("Address copied", crate::state::Tone::Info, now);
            }
            Msg::Receive(receive::Msg::Verify) => {
                let cur = self.current().cloned();
                if let (Some(a), None) = (cur.filter(|a| a.kind == "ledger"), self.verify_rx.as_ref()) {
                    match a.ledger.as_ref() {
                        Some(l) => {
                            self.verify = Some(receive::Verify::Busy);
                            self.verify_rx = Some(super::flows::hardware::verify_receive(a.address.clone(), l.path.clone(), l.device.clone()));
                        }
                        None => self.verify = Some(receive::Verify::Error { message: "This account has no Ledger path. Remove it and add it again.".into(), fix: None }),
                    }
                    self.ver += 1;
                }
            }
            Msg::Accounts(m) => self.manage_accounts(st, m),
            Msg::Settings(m) => self.manage_settings(st, m),
            Msg::Watch(m) => self.watch_msg(st, m),
            Msg::Coin(coin::Msg::Push(v)) => self.push(st, v),
            Msg::Flow(m) => self.flows_handle(st, m),
        }
        if let Some(u) = self.flows.detail.web.take() {
            fx.run(Act::OpenWeb(u));
        }
        self.ver += 1;
    }
}

impl Page for WalletTab {
    fn stamp(&self, _st: &State) -> u64 {
        hash((self.ver, self.stack.top().key, OPEN_SEQ.with(|s| s.get()), crate::data::live::PIC_VER.load(std::sync::atomic::Ordering::Relaxed)))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        if let Some(v) = OPEN_ON.with(|o| o.borrow_mut().take()) {
            self.item = None;
            self.stack.reset(Vec::new());
            if let WalletView::Nft { id } = &v {
                // The item sheet over home (the Marketplace's "List or sell").
                self.home.tab = HomeTab::Collectibles;
                self.item = Some((id.clone(), inv_sheet::State::default()));
            } else if Self::wired(&v) {
                self.stack.push(v, None);
            }
            if SIGN_OUT.with(|s| s.replace(false)) {
                self.settings.open = Some(settings::Open::Remove(crate::screens::wallet::home::sheets::Remove { sign_out: true, ..Default::default() }));
            }
        }
        self.ensure_loaded();
        FLOW_OPEN.with(|f| f.set(self.stack.frames.len() > 1));
        let m = self.markup();
        let (el, built) = build(ui, acts, &m);
        self.built = built;
        el
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        self.manage_touch();
        // Accounts: X and Y act on the focused row
        if matches!(b, Btn::X | Btn::Y) && matches!(self.stack.top().view, WalletView::Accounts) {
            let code = cur.and_then(|(_, a)| a).and_then(|a| if let Act::Local(c) = a { Some(*c) } else { None });
            self.flows.manage.focused = code.and_then(|c| self.built.msg(c)).and_then(|m| if let Msg::Accounts(accounts::Msg::Select(a)) = m { Some(a) } else { None });
        }
        if b == Btn::B {
            // a sheet over Accounts or Settings closes before the page goes back
            if matches!(self.stack.top().view, WalletView::Accounts | WalletView::Settings) {
                if let Some(m) = self.built.pad("b") {
                    self.handle(st, fx, m);
                    return true;
                }
            }
            if self.item.take().is_some() {
                self.ver += 1;
                return true;
            }
            // a flow page's own B (step back, cancel a ceremony) before leaving the page
            if let Some(m) = self.built.pad("b") {
                self.handle(st, fx, m);
                return true;
            }
            if self.stack.pop() {
                self.ver += 1;
                return true;
            }
            return false;
        }
        let name = match b {
            Btn::X => "x",
            Btn::Y => "y",
            Btn::Lt => "lt",
            Btn::Rt => "rt",
            _ => return false,
        };
        // home's LT/RT and Y are the page's own (Home.tsx usePadHandler)
        let on_home = matches!(self.stack.top().view, WalletView::Home { .. }) && !self.accounts.is_empty();
        let fallback = match (on_home, name) {
            (true, "lt") => Some(Msg::Home(home::Msg::TabStep(-1))),
            (true, "rt") => Some(Msg::Home(home::Msg::TabStep(1))),
            (true, "y") => Some(Msg::Home(home::Msg::Push(WalletView::Accounts))),
            _ => None,
        };
        let m = self.built.pad(name).or(fallback);
        match m {
            Some(m) => {
                self.handle(st, fx, m);
                true
            }
            None => false,
        }
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        self.sync_wallet();
        let flows_busy = self.flows_tick(_st);
        let mut got = Vec::new();
        self.rx.retain(|rx| loop {
            match rx.try_recv() {
                Ok(a) => got.push(a),
                Err(std::sync::mpsc::TryRecvError::Empty) => break true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break false,
            }
        });
        for a in got {
            self.take(a);
        }
        if let Some(r) = self.verify_rx.as_ref().map(|rx| rx.try_recv()) {
            let done = match r {
                Ok(Ok(Ok(same))) => Some(if same { receive::Verify::Ok } else { receive::Verify::Different }),
                Ok(Ok(Err(e))) => Some(receive::Verify::Error { message: e.message, fix: e.fix }),
                Ok(Err(e)) => Some(receive::Verify::Error { message: e, fix: None }),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(receive::Verify::Error { message: "The Ledger check stopped. Try again.".into(), fix: None }),
            };
            if let Some(v) = done {
                self.verify = Some(v);
                self.verify_rx = None;
                self.ver += 1;
            }
        }
        if let Some(a) = self.loaded_for.clone() {
            crate::data::wallet::tick(&a, now);
            self.pull(&a);
        }
        if home::wants_more(&self.home, &self.act) && self.cursor.is_some() {
            self.more();
        }
        Some(now + if flows_busy || self.verify_rx.is_some() { 0.1 } else if self.rx.is_empty() { 0.5 } else { 0.25 })
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        self.manage_touch();
        if let Some(m) = self.built.msg(code) {
            self.handle(st, fx, m);
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        self.page_hints().into_iter().filter_map(|h| key_of(h.b).map(|k| hint(k, &h.label))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_per_view() {
        let mut t = WalletTab { accounts: vec![fx_home::account()], selected: None, pf: PortfolioLoad { data: Some(fx_home::portfolio()), ..Default::default() }, fixture: true, ..WalletTab::default() };
        assert!(t.markup().texts().iter().any(|s| s.contains("Coins")));
        t.stack.push(WalletView::Receive, None);
        assert!(t.markup().texts().iter().any(|s| s == "Your address"));
        t.accounts.clear();
        t.stack.pop();
        assert!(t.markup().texts().iter().any(|s| s == "Rare Wallet"));
    }
}
