//! First run: Steam (launcher/src/onboarding/steps/Steam.tsx). Steam opens in its big-screen mode
//! to sign in; coming back (the Rare button) looks again.

use super::kit::{done_line, step};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, BSize, ButtonSpec, BVariant};

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Continue,
    Open,
    Back,
}

/// `account`: the signed-in Steam account (None not signed in); `steam_games` in the library.
pub fn view(account: Option<&str>, opened: bool, online: bool, steam_games: usize) -> El<Msg> {
    let (title, body) = if account.is_some() {
        ("Steam is ready", "Your Steam games show up in Library and on Home, and they start straight from Rare.")
    } else {
        ("Sign in to Steam", "Steam opens in its big-screen mode. Sign in there (your phone's Steam app can scan the code), then press the Rare button to come back here.")
    };
    let actions = if account.is_some() {
        vec![
            button_el(ButtonSpec::new("Continue").v(BVariant::Primary).size(BSize::Lg).def(true), Some(Msg::Continue)),
            button_el(ButtonSpec::new("Open Steam").v(BVariant::Ghost).size(BSize::Lg).icon("steam", ""), Some(Msg::Open)),
        ]
    } else {
        vec![
            button_el(ButtonSpec::new(if opened { "Open Steam again" } else { "Open Steam" }).v(BVariant::Primary).size(BSize::Lg).def(true).icon("steam", ""), Some(Msg::Open)),
            button_el(ButtonSpec::new("Skip Steam for now").v(BVariant::Ghost).size(BSize::Lg), Some(Msg::Continue)),
        ]
    };
    let mut c = div("flex flex-col gap-[0.9rem]");
    if let Some(a) = account {
        let mut kids = vec![text("Signed in as "), span("font-semibold").text(a)];
        if steam_games > 0 {
            kids.push(span("text-muted").text(format!(", {steam_games} games found")));
        }
        c = c.kid(done_line(kids));
    } else if opened {
        c = c.kid(div("text-body text-muted").text("Waiting for you to sign in to Steam."));
    }
    if account.is_none() && !online {
        c = c.kid(div("text-small text-muted").text("You are offline, so Steam cannot sign in yet. Skip this and sign in to Steam later from Library."));
    }
    c = c.kid(
        ul("flex max-w-[40rem] flex-col gap-[0.4rem] text-small leading-relaxed text-muted")
            .kid(li("").text("Games you already installed on this device are kept and show up by themselves."))
            .kid(li("").text("Games you installed with Heroic (Epic and GOG) or itch.io are listed and start from Rare too.")),
    );
    step(text(title), Some(text(body)), vec![c], actions, false).on_pad("b", Msg::Back)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam() {
        let v = view(None, false, true, 0);
        assert_eq!(v.texts()[0], "Sign in to Steam");
        assert_eq!(v.landing().as_deref(), Some("Open Steam"));
        assert!(view(None, true, false, 0).contains_text("Waiting for you to sign in to Steam."));
        assert!(view(None, true, false, 0).contains_text("You are offline, so Steam cannot sign in yet. Skip this and sign in to Steam later from Library."));
        let v = view(Some("playerone"), true, true, 42);
        assert_eq!(v.texts()[..5], ["Steam is ready", "Your Steam games show up in Library and on Home, and they start straight from Rare.", "Signed in as ", "playerone", ", 42 games found"]);
        assert_eq!(v.landing().as_deref(), Some("Continue"));
    }
}
