//! First run: Epic Games and GOG, both optional. Each signs in in a web window (the same as
//! Settings > Accounts); their games join the Library. Continue moves on with either, both or none.

use super::kit::{done_line, step};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, panel, BSize, ButtonSpec, BVariant};

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// "epic" or "gog"
    SignIn(&'static str),
    Continue,
    Back,
}

/// One store: its key, name, the signed-in name (None not signed in).
pub struct Row<'a> {
    pub key: &'static str,
    pub name: &'a str,
    pub account: Option<&'a str>,
}

pub fn view(rows: &[Row], online: bool) -> El<Msg> {
    let any = rows.iter().any(|r| r.account.is_some());
    let list = panel(
        "max-w-[40rem]",
        rows.iter()
            .map(|r| {
                let mut row = div("flex items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[1rem] first:border-t-0").key(r.key);
                let info = span("min-w-0 flex-1").kid(span("block text-body font-semibold").text(r.name));
                match r.account {
                    Some(a) => row.kid(info).kid(span("shrink-0").kid(done_line(vec![text("Signed in as "), span("font-semibold").text(a)]))),
                    None => {
                        row = row.kid(info.kid(span("block text-small text-muted").text(format!("Your {} games in the Library, installed and started from Rare.", r.name))));
                        row.kid(button_el(ButtonSpec::new("Sign in").v(BVariant::Secondary).size(BSize::Md).disabled(!online), Some(Msg::SignIn(r.key))))
                    }
                }
            })
            .collect(),
    );
    let mut content = vec![list.attr("data-nav-group", "firstrun-stores")];
    if !online {
        content.push(div("mt-[1rem] text-small text-muted").text("You are offline. Sign in later from Settings > Accounts."));
    }
    let actions = vec![button_el(ButtonSpec::new(if any { "Continue" } else { "Skip for now" }).v(if any { BVariant::Primary } else { BVariant::Secondary }).size(BSize::Lg).def(true).class("min-w-[11rem]"), Some(Msg::Continue))];
    step(text("Epic Games and GOG"), Some(text("Optional. Sign in to either to play those games here too. You can do this later in Settings > Accounts.")), content, actions, false).on_pad("b", Msg::Back)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores() {
        let rows = [Row { key: "epic", name: "Epic Games", account: None }, Row { key: "gog", name: "GOG", account: None }];
        let v = view(&rows, true);
        assert_eq!(v.texts()[0], "Epic Games and GOG");
        assert_eq!(v.landing().as_deref(), Some("Skip for now"));
        let rows = [Row { key: "epic", name: "Epic Games", account: Some("player") }, Row { key: "gog", name: "GOG", account: None }];
        let v = view(&rows, true);
        assert!(v.contains_text("Signed in as "));
        assert_eq!(v.landing().as_deref(), Some("Continue"));
        assert!(view(&rows, false).contains_text("You are offline. Sign in later from Settings > Accounts."));
        assert_eq!(v.pad("b"), Some(&Msg::Back));
    }
}
