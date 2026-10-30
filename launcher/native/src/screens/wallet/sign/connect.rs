//! "<game> wants to see your address": the web-game connect prompt
//! (launcher/src/wallet/ui/sign/ConnectSheet.tsx). Pick which account the game sees (the selected
//! one by default); Allow remembers the game's origin.

use super::review::Account;
use super::site::{host_of, site_risks};
use crate::screens::wallet::kit::{addr, btn, list_button, sheet, account_avatar, SheetSpec, Variant};
use crate::screens::wallet::markup::*;

pub struct Props<'a> {
    pub game: &'a str,
    pub origin: &'a str,
    pub accounts: &'a [Account],
    pub initial: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub pick: Option<String>,
}

impl State {
    pub fn new(p: &Props) -> State {
        let pick = match p.initial {
            Some(i) if p.accounts.iter().any(|a| a.address == i) => Some(i.to_string()),
            _ => p.accounts.first().map(|a| a.address.clone()),
        };
        State { pick }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Pick(String),
    /// Allow with the picked account (nothing when there is none)
    Allow,
    Deny,
}

pub fn view(p: &Props, st: &State) -> El<Msg> {
    let warn = site_risks(Some(p.origin));
    let mut body = vec![p_("text-sm text-secondary").text("It can also ask you to sign. Every request still shows up here for you to approve, and nothing is signed without it.")];
    if !warn.is_empty() {
        body.push(ul("mt-3 flex flex-col gap-1 text-sm text-negative").role("alert").kids(warn.iter().map(|w| li("").key(&w.id).text(&w.text))));
    }
    body.push(h3("mb-2 mt-5 text-sm text-secondary").text("Account to share"));
    body.push(div("rounded border border-border-primary p-1").kids(p.accounts.iter().map(|a| {
        let right = (st.pick.as_deref() == Some(a.address.as_str())).then(|| icon("check", "text-primary").attr("size", "1.25rem"));
        list_button(Some(account_avatar(&a.address, "2rem")), &a.label, Some(addr(&a.address, false, "")), right, Msg::Pick(a.address.clone()), false, "").key(&a.address)
    })));
    sheet(
        SheetSpec {
            title: format!("{} wants to see your address", p.game),
            subtitle: Some(host_of(Some(p.origin))),
            icon: None,
            footer: Some(vec![btn("Allow", Variant::Primary, "flex-1", Some(Msg::Allow)).def(true), btn("Don't allow", Variant::Secondary, "", Some(Msg::Deny))]),
            close: Some(Msg::Deny),
            width: "36rem",
        },
        body,
    )
}

fn p_(class: &str) -> El<Msg> {
    p(class)
}

/// Applies a message; Some(address) to allow with, or the request is denied (`Err`).
pub fn update(st: &mut State, msg: Msg) -> Option<Result<String, ()>> {
    match msg {
        Msg::Pick(a) => {
            st.pick = Some(a);
            None
        }
        Msg::Allow => st.pick.clone().map(Ok),
        Msg::Deny => Some(Err(())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::sign::fixtures::accounts;

    #[test]
    fn connect() {
        let accts = accounts();
        let pr = Props { game: "Word Game", origin: "https://wordgame.example.com", accounts: &accts, initial: Some(&accts[1].address) };
        let mut st = State::new(&pr);
        assert_eq!(st.pick.as_deref(), Some(accts[1].address.as_str()));
        let v = view(&pr, &st);
        assert_eq!(
            v.texts(),
            [
                "Word Game wants to see your address",
                "wordgame.example.com",
                "It can also ask you to sign. Every request still shows up here for you to approve, and nothing is signed without it.",
                "Account to share",
                "Main",
                "0x7d20...b58e",
                "Ledger",
                "0x5ab1...d2e1",
                "Allow",
                "Don't allow",
            ]
        );
        assert_eq!(v.landing().as_deref(), Some("Allow"));
        update(&mut st, Msg::Pick(accts[0].address.clone()));
        assert_eq!(update(&mut st, Msg::Allow), Some(Ok(accts[0].address.clone())));
        let bad = Props { game: "X", origin: "http://x.game", accounts: &accts, initial: Some("0xnope") };
        let st = State::new(&bad);
        assert_eq!(st.pick.as_deref(), Some(accts[0].address.as_str()));
        assert!(view(&bad, &st).contains_text("This site is not using HTTPS."));
        assert_eq!(crate::screens::wallet::kit::sheet_b(&view(&bad, &st)), Some(Msg::Deny));
    }
}
