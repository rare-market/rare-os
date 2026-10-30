//! Accounts (launcher/src/wallet/ui/pages/Accounts.tsx): the switcher as a page, grouped by how each
//! account signs. A selects and goes back, X renames, Y opens the account's actions.

use crate::screens::wallet::fmt::fmt_usd;
use crate::screens::wallet::home::lib::{hint, kind_group, kind_label, Hint, KindGroup};
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::kit::{btn_icon, list_button, page, sheet, account_avatar, SheetSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::RareAccount;
use std::collections::BTreeMap;

const GROUPS: [(KindGroup, &str); 3] = [(KindGroup::Rare, "Rare accounts"), (KindGroup::Hardware, "Hardware"), (KindGroup::Watch, "Watch")];

pub struct Props<'a> {
    pub accounts: &'a [RareAccount],
    pub current: Option<&'a RareAccount>,
    /// the Trade API's portfolio value per account, filled in as answers arrive
    pub values: &'a BTreeMap<String, Option<f64>>,
    /// the current account came from a recovery phrase this device holds
    pub has_phrase: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// the actions sheet for this account
    pub menu: Option<String>,
    /// the remove sheet for this account
    pub removing: Option<String>,
    pub busy: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Select(String),
    /// X on the focused row (the host passes the focused row's data-addr)
    RenameFocused,
    /// Y on the focused row
    MenuFocused,
    CloseMenu,
    Rename(String),
    CopyAddress(String),
    Remove(String),
    AddFromPhrase,
    Push(WalletView),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// select it, then pop back to home
    Select(String),
    /// keyboard "Account name" with the label, then `keystore.rename`
    Rename(String),
    /// copy and toast "Address copied"
    Copy(String),
    /// unlock "Enter your PIN to add an account", derive the next from the current phrase, select, toast "Account added"
    AddFromPhrase,
    Push(WalletView),
}

fn row_el(a: &RareAccount, current: bool, value: Option<f64>) -> El<Msg> {
    let c: Vec<char> = a.address.chars().collect();
    let short = if c.len() >= 14 { format!("{}...{}", c[..8].iter().collect::<String>(), c[c.len() - 6..].iter().collect::<String>()) } else { a.address.clone() };
    let mut b = button("flex h-[4.5rem] w-full items-center gap-4 rounded-lg px-4 text-left transition-colors focus-visible:bg-layer-03", Some(Msg::Select(a.address.clone())))
        .def(current)
        .attr("data-addr", a.address.clone())
        .key(&a.address)
        .kid(account_avatar(&a.address, "2.75rem"))
        .kid(
            span("min-w-0 flex-1")
                .kid(span("flex items-baseline gap-3").kid(span("truncate text-base font-semibold").text(&a.label)).kid(span("shrink-0 text-sm text-secondary").text(kind_label(a))))
                .kid(span("block font-mono text-sm text-secondary").text(short)),
        )
        .kid(span("shrink-0 text-right tabular-nums").kid(span("block text-base font-semibold").text(value.map(|v| fmt_usd(Some(v))).unwrap_or_default())));
    b = if current { b.kid(icon("check", "shrink-0 text-icon-primary").attr("size", "1.2rem").label("Current account")) } else { b.kid(span("w-[1.2rem] shrink-0")) };
    b
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let n = pr.accounts.len();
    let mut right = div("");
    if pr.has_phrase {
        right = right.kid(btn_icon("plus", if st.busy { "Adding" } else { "Add another from this phrase" }, Variant::Secondary, Size::Lg, "", Some(Msg::AddFromPhrase)));
    }
    right = right.kid(btn_icon("plus", "Add account", Variant::Primary, Size::Lg, "", Some(Msg::Push(WalletView::Add))));
    // the right side is a fragment on the web: its buttons sit straight in the page's right slot
    let right_kids = right.kids;
    let mut groups = div("flex flex-col gap-6");
    for (g, t) in GROUPS {
        let list: Vec<&RareAccount> = pr.accounts.iter().filter(|a| kind_group(a) == g).collect();
        if list.is_empty() {
            continue;
        }
        groups = groups.kid(
            section("")
                .key(t)
                .kid(h2("mb-1 px-4 text-sm font-semibold text-secondary").text(t))
                .kid(div("rounded-lg border border-border-primary bg-layer-01 p-1").kids(list.iter().map(|a| row_el(a, pr.current.is_some_and(|c| c.address == a.address), pr.values.get(&a.address).copied().flatten())))),
        );
    }
    if n == 0 {
        groups = groups.kid(div("flex flex-col items-center gap-3 py-16 text-secondary").kid(icon("users", "").attr("size", "1.5rem")).kid(p("text-sm").text("No accounts yet")));
    }
    let mut body = vec![groups];
    if let Some(a) = st.menu.as_ref().and_then(|m| pr.accounts.iter().find(|a| a.address == *m)) {
        body.push(sheet(
            SheetSpec { title: a.label.clone(), subtitle: Some(kind_label(a).into()), icon: None, footer: None, close: Some(Msg::CloseMenu), width: "34rem" },
            vec![div("-mx-2 flex flex-col")
                .kid(list_button(Some(icon("check", "").attr("size", "1.2rem")), "Use this account", None, None, Msg::Select(a.address.clone()), true, ""))
                .kid(list_button(Some(icon("pencil", "").attr("size", "1.2rem")), "Rename", None, None, Msg::Rename(a.address.clone()), false, ""))
                .kid(list_button(Some(icon("copy", "").attr("size", "1.2rem")), "Copy address", None, None, Msg::CopyAddress(a.address.clone()), false, ""))
                .kid(list_button(Some(icon("arrow-right-left", "").attr("size", "1.2rem")), "Move everything to this account", Some(text("From another account on this SuiPlay")), None, Msg::Push(WalletView::MoveAll { to: a.address.clone() }), false, ""))
                .kid(list_button(Some(icon("trash-2", "").attr("size", "1.2rem")), "Remove from this device", None, None, Msg::Remove(a.address.clone()), false, ""))],
        ));
    }
    let mut pg = page(Some("Accounts"), Some(text(format!("{n} {} on this device", if n == 1 { "account" } else { "accounts" }))), None, false, "", body).attr("data-ready", n.to_string());
    // put the fragment's buttons into the right slot
    if let Some(head_row) = pg.kids.get_mut(0).and_then(|inner| inner.kids.get_mut(0)) {
        head_row.kids.push(div("flex shrink-0 items-center gap-2").kids(right_kids));
    }
    if st.menu.is_none() && st.removing.is_none() {
        pg = pg.on_pad("x", Msg::RenameFocused).on_pad("y", Msg::MenuFocused);
    }
    pg
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Use"), hint("x", "Rename"), hint("y", "Actions"), hint("b", "Back")]
}

/// `focused`: the data-addr of the focused row, for X and Y.
pub fn update(pr: &Props, st: &mut State, msg: Msg, focused: Option<&str>) -> Option<Effect> {
    match msg {
        Msg::Select(a) => {
            st.menu = None;
            Some(Effect::Select(a))
        }
        Msg::RenameFocused => focused.map(|a| Effect::Rename(a.to_string())),
        Msg::MenuFocused => {
            st.menu = focused.map(str::to_string);
            None
        }
        Msg::CloseMenu => {
            st.menu = None;
            None
        }
        Msg::Rename(a) => {
            st.menu = None;
            Some(Effect::Rename(a))
        }
        Msg::CopyAddress(a) => {
            st.menu = None;
            Some(Effect::Copy(a))
        }
        Msg::Remove(a) => {
            st.menu = None;
            st.removing = Some(a);
            None
        }
        Msg::AddFromPhrase => {
            if pr.current.is_some_and(|c| c.hd.is_some()) && !st.busy {
                st.busy = true;
                Some(Effect::AddFromPhrase)
            } else {
                None
            }
        }
        Msg::Push(v) => {
            st.menu = None;
            Some(Effect::Push(v))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::sign::fixtures::accounts;

    #[test]
    fn accounts_page() {
        let mut list = accounts();
        list.push(RareAccount::new("0x00000000000000000000000000000000000000000000000000000000000000aa", "Demo wallet", "watch"));
        let mut values = BTreeMap::new();
        values.insert(list[0].address.clone(), Some(1284.07));
        let pr = Props { accounts: &list, current: Some(&list[0]), values: &values, has_phrase: false };
        let mut st = State::default();
        let v = view(&pr, &st);
        assert_eq!(
            v.texts(),
            ["Accounts", "3 accounts on this device", "Add account", "Rare accounts", "Main", "Passkey", "0x7d20dc...f0b58e", "$1,284", "Hardware", "Ledger", "Ledger", "0x5ab1e8...c3d2e1", "", "Watch", "Demo wallet", "Watch only", "0x000000...0000aa", ""]
        );
        assert_eq!(v.landing().as_deref(), Some("MainPasskey0x7d20dc...f0b58e$1,284"));
        assert_eq!(update(&pr, &mut st, Msg::RenameFocused, Some(&list[1].address)), Some(Effect::Rename(list[1].address.clone())));
        update(&pr, &mut st, Msg::MenuFocused, Some(&list[1].address));
        let v = view(&pr, &st);
        assert!(v.contains_text("Remove from this device"));
        assert_eq!(v.pad("x"), None);
        update(&pr, &mut st, Msg::Remove(list[1].address.clone()), None);
        assert_eq!(st.removing.as_deref(), Some(list[1].address.as_str()));
    }
}
