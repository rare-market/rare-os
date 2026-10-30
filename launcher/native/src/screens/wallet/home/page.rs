//! Rare Wallet home (launcher/src/wallet/ui/pages/Home.tsx): account header with value, actions,
//! then Coins / Collectibles / Activity tabs (LT and RT switch them; LB/RB stay the launcher's).
//! Modelled on rare.market's profile page.

use super::header::{account_header, activity_groups, coin_head, coin_row};
use super::lib::{hint, load_bar, Hint};
use super::stack::{HomeTab, WalletView};
use crate::screens::wallet::kit::{btn, btn_full, btn_icon, empty, page, tabs, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::{ActivityItem, Portfolio, RareAccount};
use std::collections::BTreeSet;

const TABS: [HomeTab; 3] = [HomeTab::Coins, HomeTab::Collectibles, HomeTab::Activity];

/// `usePortfolio`: the last answer at once (cached), refreshed when the wallet's epoch bumps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortfolioLoad {
    pub data: Option<Portfolio>,
    pub loading: bool,
    pub error: bool,
}

/// `useActivity`: the paged feed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActivityLoad {
    pub items: Vec<ActivityItem>,
    /// the first page arrived
    pub ready: bool,
    /// no more pages
    pub done: bool,
    pub loading: bool,
    pub error: bool,
}

pub struct Props<'a> {
    pub account: Option<&'a RareAccount>,
    /// `canSignAccount`: the account can sign (not watch only)
    pub can_sign: bool,
    pub pf: &'a PortfolioLoad,
    pub act: &'a ActivityLoad,
    /// ms since the epoch and the device's UTC offset in minutes (day headings, times)
    pub now: i64,
    pub tz_min: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tab: HomeTab,
    pub selecting: bool,
    pub selected: BTreeSet<String>,
    /// "Copied" on the address for 1.5 s
    pub copied: bool,
}

impl State {
    pub fn new(tab: Option<HomeTab>) -> State {
        State { tab: tab.unwrap_or(HomeTab::Coins), selecting: false, selected: BTreeSet::new(), copied: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Tab(HomeTab),
    Push(WalletView),
    Select,
    StopSelecting,
    SendNfts,
    Toggle(String),
    CopyAddress(String),
    More,
    Retry,
    /// LT / RT
    TabStep(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Push(WalletView),
    Copy(String),
    /// load the next activity page (`act.more`)
    More,
    /// retry: the next page, or the first again when none arrived (`act.retry`)
    Retry,
}

fn tab_value(t: HomeTab) -> &'static str {
    match t {
        HomeTab::Coins => "coins",
        HomeTab::Collectibles => "collectibles",
        HomeTab::Activity => "activity",
    }
}

fn tab_of(v: &str) -> HomeTab {
    match v {
        "collectibles" => HomeTab::Collectibles,
        "activity" => HomeTab::Activity,
        _ => HomeTab::Coins,
    }
}

/// The page. `inventory` builds the Collectibles tab (InventoryView over this account).
pub fn view(pr: &Props, st: &State, inventory: impl FnOnce() -> El<Msg>) -> Option<El<Msg>> {
    let account = pr.account?;
    let loading = match st.tab {
        HomeTab::Coins => pr.pf.loading,
        HomeTab::Collectibles => false,
        HomeTab::Activity => pr.act.loading,
    };
    let mut body = vec![account_header(account, pr.pf.data.as_ref(), st.copied, Msg::CopyAddress(account.address.clone()))];

    let main = if pr.can_sign {
        btn_icon("arrow-up-right", "Send", Variant::Primary, Size::Lg, "", Some(Msg::Push(WalletView::Send { coin_type: None, to: None }))).def(true)
    } else {
        btn_icon("user-plus", "Add a signing account", Variant::Primary, Size::Lg, "", Some(Msg::Push(WalletView::Add))).def(true)
    };
    body.push(
        div("mb-6 flex flex-wrap items-center gap-2")
            .kid(main)
            .kid(btn_icon("arrow-down-left", "Receive", Variant::Secondary, Size::Lg, "", Some(Msg::Push(WalletView::Receive))))
            .kid(btn_icon("users", "Accounts", Variant::Secondary, Size::Lg, "", Some(Msg::Push(WalletView::Accounts))))
            .kid(btn_icon("settings", "Settings", Variant::Secondary, Size::Lg, "", Some(Msg::Push(WalletView::Settings))))
            .kid(btn_icon("arrow-right-left", "Move from your old SuiPlay wallet", Variant::Ghost, Size::Lg, "ml-auto", Some(Msg::Push(WalletView::Playtron)))),
    );

    let mut bar = div("mb-2 flex items-end justify-between border-b border-border-primary").kid(tabs(
        &[("coins", "Coins"), ("collectibles", "Collectibles"), ("activity", "Activity")],
        tab_value(st.tab),
        |v| Msg::Tab(tab_of(v)),
        "",
    ));
    if st.tab == HomeTab::Collectibles && pr.can_sign {
        let mut right = div("mb-2 flex items-center gap-2");
        if st.selecting {
            let n = st.selected.len();
            right = right.kid(btn_full(&if n > 0 { format!("Send {n}") } else { "Pick items to send".into() }, Variant::Primary, Size::Md, false, "", Some(Msg::SendNfts)));
        }
        right = right.kid(btn_full(if st.selecting { "Done" } else { "Select" }, Variant::Secondary, Size::Md, false, "", Some(if st.selecting { Msg::StopSelecting } else { Msg::Select })));
        bar = bar.kid(right);
    }
    body.push(bar);
    body.push(load_bar(loading));

    match st.tab {
        HomeTab::Coins => {
            let mut c = div("pt-2");
            if let Some(d) = pr.pf.data.as_ref() {
                if !d.tokens.is_empty() {
                    c = c.kid(coin_head()).kid(
                        div("")
                            .attr("data-nav-group", "wallet-coins")
                            .kids(d.tokens.iter().enumerate().map(|(i, t)| coin_row(t, i, Msg::Push(WalletView::Coin { coin_type: t.coin_type.clone() })))),
                    );
                } else {
                    c = c.kid(empty(
                        "No coins yet",
                        Some(text("Send Sui assets to this address and they show up here.")),
                        Some(btn("Receive", Variant::Secondary, "", Some(Msg::Push(WalletView::Receive)))),
                        false,
                    ));
                }
            }
            if pr.pf.error && pr.pf.data.is_none() {
                c = c.kid(empty("Could not load balances", Some(text("Check the connection and try again.")), None, false));
            }
            body.push(c);
        }
        HomeTab::Collectibles => body.push(inventory()),
        HomeTab::Activity => {
            let a = pr.act;
            let mut c = div("pt-2").kid(div("").attr("data-nav-group", "wallet-activity").kid(activity_groups(&a.items, pr.now, pr.tz_min, |x| Msg::Push(WalletView::Activity { digest: x.digest.clone() }))));
            if !a.items.is_empty() && !a.done && !a.error {
                c = c.kid(div("flex justify-center py-3").kid(btn_full("Show more", Variant::Secondary, Size::Md, false, "", Some(Msg::More))));
            }
            if a.ready && a.items.is_empty() {
                c = c.kid(empty("No activity yet", Some(text("Your transactions show up here.")), None, false));
            }
            if a.error {
                c = c.kid(empty(
                    if a.ready { "Couldn't load older activity" } else { "Couldn't load activity" },
                    None,
                    Some(btn_full("Retry", Variant::Secondary, Size::Md, false, "", Some(Msg::Retry))),
                    true,
                ));
            }
            body.push(c);
        }
    }

    let mut el = page(None, None, None, false, "", body)
        .attr("data-ready", format!("{}:{}", tab_value(st.tab), ready(pr, st)))
        .on_pad("lt", Msg::TabStep(-1))
        .on_pad("rt", Msg::TabStep(1))
        .on_pad("y", if st.selecting { Msg::SendNfts } else { Msg::Push(WalletView::Accounts) });
    if st.tab == HomeTab::Collectibles && pr.can_sign {
        el = el.on_pad("x", if st.selecting { Msg::StopSelecting } else { Msg::Select });
    }
    if st.selecting {
        el = el.on_pad("b", Msg::StopSelecting);
    }
    Some(el)
}

fn ready(pr: &Props, st: &State) -> bool {
    match st.tab {
        HomeTab::Coins => pr.pf.data.is_some(),
        HomeTab::Collectibles => true,
        HomeTab::Activity => pr.act.ready,
    }
}

/// The bottom bar while home is on top.
pub fn hints(pr: &Props, st: &State) -> Vec<Hint> {
    if st.selecting {
        let n = st.selected.len();
        return vec![hint("a", "Toggle"), hint("y", &if n > 0 { format!("Send {n}") } else { "Send".into() }), hint("x", "Done")];
    }
    let mut h = vec![hint("a", "Open")];
    if st.tab == HomeTab::Collectibles && pr.can_sign {
        h.push(hint("x", "Select"));
    }
    h.push(hint("y", "Accounts"));
    h.push(hint("qam", "Quick settings"));
    h
}

fn set_tab(st: &mut State, t: HomeTab) {
    if t != HomeTab::Collectibles {
        st.selecting = false;
        st.selected.clear();
    }
    st.tab = t;
}

pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Tab(t) => set_tab(st, t),
        Msg::TabStep(d) => {
            let i = TABS.iter().position(|t| *t == st.tab).unwrap_or(0) as i32 + d;
            set_tab(st, TABS[i.rem_euclid(3) as usize]);
        }
        Msg::Push(v) => return Some(Effect::Push(v)),
        Msg::Select => st.selecting = true,
        Msg::StopSelecting => {
            st.selecting = false;
            st.selected.clear();
        }
        Msg::SendNfts => {
            if !st.selected.is_empty() {
                return Some(Effect::Push(WalletView::SendNfts { ids: st.selected.iter().cloned().collect(), to: None }));
            }
        }
        Msg::Toggle(id) => {
            if !st.selected.remove(&id) {
                st.selected.insert(id);
            }
        }
        Msg::CopyAddress(a) => {
            st.copied = true;
            return Some(Effect::Copy(a));
        }
        Msg::More => return Some(Effect::More),
        Msg::Retry => return Some(Effect::Retry),
    }
    None
}

/// A different account starts clean (selection off).
pub fn account_changed(st: &mut State) {
    st.selecting = false;
    st.selected.clear();
}

/// "the Trade API can answer a page with only a few rows; keep going until the list fills the screen".
pub fn wants_more(st: &State, act: &ActivityLoad) -> bool {
    st.tab == HomeTab::Activity && act.ready && !act.done && !act.loading && !act.error && act.items.len() < 12
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::*;

    fn inv() -> El<Msg> {
        div("").attr("data-testid", "inventory")
    }

    #[test]
    fn coins_tab() {
        let acct = account();
        let pf = PortfolioLoad { data: Some(portfolio()), loading: false, error: false };
        let act = ActivityLoad::default();
        let pr = Props { account: Some(&acct), can_sign: true, pf: &pf, act: &act, now: NOW, tz_min: 0 };
        let st = State::new(None);
        let v = view(&pr, &st, inv).unwrap();
        assert_eq!(
            v.texts(),
            [
                "Main", "0x7d20...b58e", "Passkey", "$1,284", "+$12.40", "0.98%", "24h", "1,204.5 SUI", "Send", "Receive", "Accounts", "Settings", "Move from your old SuiPlay wallet", "Coins", "Collectibles", "Activity", "Coin", "Amount", "Value",
                "24h", "SUI", "Sui", "1,204.5", "$1,091", "1.6%", "USDC", "USD Coin", "192.79", "$192.79", "0.0%",
            ]
        );
        assert_eq!(v.landing().as_deref(), Some("Send"));
        assert_eq!(hints(&pr, &st).iter().map(|h| h.label.as_str()).collect::<Vec<_>>(), ["Open", "Accounts", "Quick settings"]);
        assert_eq!(v.pad("y"), Some(&Msg::Push(WalletView::Accounts)));
    }

    #[test]
    fn watch_only_and_empty() {
        let acct = crate::screens::wallet::types::RareAccount::new(super::super::fixtures::ME, "Demo wallet", "watch");
        let pf = PortfolioLoad { data: Some(Portfolio { total_usd: None, change_24h_usd: None, tokens: vec![], priced: false }), loading: false, error: false };
        let act = ActivityLoad::default();
        let pr = Props { account: Some(&acct), can_sign: false, pf: &pf, act: &act, now: NOW, tz_min: 0 };
        let v = view(&pr, &State::new(None), inv).unwrap();
        let t = v.texts();
        assert!(t.contains(&"Watch only".to_string()));
        assert!(t.contains(&"$0.00".to_string()));
        assert!(t.contains(&"Prices unavailable".to_string()));
        assert_eq!(v.landing().as_deref(), Some("Add a signing account"));
        assert!(v.contains_text("No coins yet"));
        assert!(v.contains_text("Send Sui assets to this address and they show up here."));
        let none = PortfolioLoad { data: None, loading: false, error: true };
        let pr = Props { pf: &none, ..pr };
        let v = view(&pr, &State::new(None), inv).unwrap();
        assert!(v.contains_text("--"));
        assert!(v.contains_text("Could not load balances"));
        assert!(view(&Props { account: None, ..pr }, &State::new(None), inv).is_none());
    }

    #[test]
    fn collectibles_select_and_send() {
        let acct = account();
        let pf = PortfolioLoad { data: Some(portfolio()), ..Default::default() };
        let act = ActivityLoad::default();
        let pr = Props { account: Some(&acct), can_sign: true, pf: &pf, act: &act, now: NOW, tz_min: 0 };
        let mut st = State::new(Some(HomeTab::Collectibles));
        let v = view(&pr, &st, inv).unwrap();
        assert!(v.find_by_attr("data-testid", "inventory").is_some());
        assert_eq!(v.pad("x"), Some(&Msg::Select));
        assert_eq!(hints(&pr, &st).iter().map(|h| h.label.as_str()).collect::<Vec<_>>(), ["Open", "Select", "Accounts", "Quick settings"]);
        update(&mut st, Msg::Select);
        let v = view(&pr, &st, inv).unwrap();
        assert!(v.find_nav("Pick items to send").is_some());
        assert_eq!(update(&mut st, Msg::SendNfts), None);
        update(&mut st, Msg::Toggle("0xa".into()));
        update(&mut st, Msg::Toggle("0xb".into()));
        update(&mut st, Msg::Toggle("0xa".into()));
        assert!(view(&pr, &st, inv).unwrap().find_nav("Send 1").is_some());
        assert_eq!(hints(&pr, &st).iter().map(|h| h.label.as_str()).collect::<Vec<_>>(), ["Toggle", "Send 1", "Done"]);
        assert_eq!(update(&mut st, Msg::SendNfts), Some(Effect::Push(WalletView::SendNfts { ids: vec!["0xb".into()], to: None })));
        // leaving the tab stops selecting
        update(&mut st, Msg::TabStep(1));
        assert_eq!(st.tab, HomeTab::Activity);
        assert!(!st.selecting && st.selected.is_empty());
        update(&mut st, Msg::TabStep(1));
        assert_eq!(st.tab, HomeTab::Coins);
        update(&mut st, Msg::TabStep(-1));
        assert_eq!(st.tab, HomeTab::Activity);
    }

    #[test]
    fn activity_tab() {
        let acct = account();
        let pf = PortfolioLoad::default();
        let act = ActivityLoad { items: activity(), ready: true, done: false, loading: false, error: false };
        let pr = Props { account: Some(&acct), can_sign: true, pf: &pf, act: &act, now: NOW, tz_min: 0 };
        let st = State::new(Some(HomeTab::Activity));
        let t = view(&pr, &st, inv).unwrap().texts();
        let i = t.iter().position(|x| x == "Today").unwrap();
        assert_eq!(&t[i..i + 6], ["Today", "Received", "25 SUI", " from ", "0xabab...abab", "2:12 PM"]);
        assert!(t.contains(&"Yesterday".to_string()));
        assert!(t.contains(&"Failed".to_string()));
        assert!(t.contains(&"Show more".to_string()));
        assert!(wants_more(&st, &act));
        let empty_act = ActivityLoad { ready: true, done: true, ..Default::default() };
        let v = view(&Props { act: &empty_act, ..pr }, &st, inv).unwrap();
        assert!(v.contains_text("No activity yet"));
        let err = ActivityLoad { error: true, ..Default::default() };
        let v = view(&Props { act: &err, ..pr }, &st, inv).unwrap();
        assert!(v.contains_text("Couldn't load activity"));
        assert_eq!(v.find_nav("Retry").unwrap().on, Some(Msg::Retry));
    }
}
