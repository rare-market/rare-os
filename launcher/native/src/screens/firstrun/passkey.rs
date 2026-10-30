//! First run: sign in (launcher/src/wallet/ui/onboard/firstrun.tsx, the passkey step Rare Wallet
//! registers): the start page with the three ways, the passkey flow, "Other ways" as the wallet's
//! own pages inside setup, and "You're @name" with a free username suggestion.

use crate::screens::wallet::kit::account_avatar;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::onboard::passkey_flow::Step as FlowStep;
use crate::screens::wallet::ui::{button_el, panel, BSize, ButtonSpec, BVariant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Start,
    Signin,
    Create,
    Other,
    Named,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Mode(Mode),
    /// "Create account": a recovery-phrase account on this device (the wallet's Create page)
    Phrase,
    /// open the wallet's Add page inside setup
    Other,
    Next,
    /// "Connect to Wi-Fi" when offline: back a step
    Back,
    /// "You're @name": save it with one signature, then next
    Continue,
    /// pick another username (friends' picker)
    Change,
}

fn copy(title: &str, body: Option<&str>, kids: Vec<El<Msg>>) -> El<Msg> {
    div("flex flex-col gap-[1rem]")
        .kid(h1("text-[2.5rem] font-semibold leading-[1.1] tracking-[-0.03em]").text(title))
        .kid_opt(body.map(|b| p("text-row font-normal leading-relaxed text-muted").text(b)))
        .kids(kids)
}

fn way(ic: &'static str, title: &str, body: &str) -> El<Msg> {
    div("flex items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[1rem] first:border-t-0")
        .key(title)
        .kid(span("flex h-[2.75rem] w-[2.75rem] shrink-0 items-center justify-center rounded-full bg-l3 text-fg").kid(icon(ic, "").attr("size", "1.25rem").attr("stroke-width", "1.75")))
        .kid(span("").kid(span("block text-body font-semibold").text(title)).kid(span("block text-small text-muted").text(body)))
}

/// The step. `flow` is the passkey flow's own markup (signin / create), `wallet` the wallet's page
/// stack for Other ways; `flow_step` is where the flow is (for the body line).
pub fn view(mode: Mode, flow_step: FlowStep, online: bool, flow: Option<El<Msg>>, wallet: Option<El<Msg>>) -> El<Msg> {
    match mode {
        Mode::Other => return div("mx-auto h-full w-full max-w-[56rem]").kid_opt(wallet),
        Mode::Named => return div("h-full"),
        _ => {}
    }
    let create = mode == Mode::Create;
    let title = if mode == Mode::Start {
        "Create your account"
    } else if create {
        "Create account"
    } else {
        "Sign in with a passkey"
    };
    let body = if mode == Mode::Start {
        Some("One account for Rare Wallet, Friends and Rare Marketplace. It is kept on this SuiPlay and signs with A. You write down 12 words once as a backup.")
    } else if flow_step == FlowStep::Pin {
        Some("This device signs everyday transactions with the PIN. Your passkey can always sign too.")
    } else if flow_step == FlowStep::Run {
        None
    } else if create {
        Some("Make a new wallet protected by a passkey.")
    } else {
        Some("Use the passkey you made before.")
    };
    let mut kids = Vec::new();
    if mode == Mode::Start && !online {
        kids.push(
            div("flex max-w-[24rem] flex-col items-start gap-[0.6rem] text-small text-muted")
                .kid(text("No internet. You can create your account now; Friends and the Marketplace need it later."))
                .kid(button_el(ButtonSpec::new("Connect to Wi-Fi").size(BSize::Lg).v(BVariant::Secondary), Some(Msg::Back))),
        );
    }
    if mode == Mode::Start {
        kids.push(
            div("mt-[0.75rem] flex flex-col items-start gap-[0.75rem]")
                .attr("data-nav-group", "firstrun-actions")
                .attr("data-nav-remember", "")
                .kid(
                    div("flex w-full max-w-[22rem] flex-col gap-[0.75rem]")
                        .kid(button_el(ButtonSpec::new("Create account").v(BVariant::Primary).size(BSize::Lg).def(true).class("w-full"), Some(Msg::Phrase)))
                        .kid(button_el(ButtonSpec::new("Sign in with a passkey").size(BSize::Lg).icon("key-round", "").class("w-full"), Some(Msg::Mode(Mode::Signin)))),
                )
                .kid(
                    div("-ml-[1.75rem] flex flex-wrap gap-[0.25rem]")
                        .kid(button_el(ButtonSpec::new("Other ways to sign in").size(BSize::Lg).v(BVariant::Ghost), Some(Msg::Other)))
                        .kid(button_el(ButtonSpec::new("Not now").size(BSize::Lg).v(BVariant::Ghost), Some(Msg::Next))),
                ),
        );
    }
    let right = if mode == Mode::Start {
        panel(
            "max-w-[30rem]",
            vec![
                way("gamepad-2", "This SuiPlay", "Signs with A. Back up 12 words once"),
                way("smartphone", "A passkey on your phone", "Scan a code and confirm with your face or fingerprint"),
                way("usb", "A passkey on a security key", "Plug it into the USB-C port and touch it"),
            ],
        )
    } else {
        div("max-w-[34rem]").key(if create { "create" } else { "signin" }).kid_opt(flow)
    };
    div("grid h-full grid-cols-[minmax(0,26rem)_minmax(0,1fr)] items-center gap-[4rem]").kid(copy(title, body, kids)).kid(div("min-w-0").kid(right))
}

/// "You're @name" (`Named`): `name` is the saved username or a free suggestion.
pub fn named_view(name: Option<&str>, address: Option<&str>, busy: bool) -> El<Msg> {
    let mut actions = div("mt-[0.75rem] flex flex-wrap gap-[0.75rem]")
        .attr("data-nav-group", "firstrun-actions")
        .attr("data-nav-remember", "")
        .kid(button_el(ButtonSpec::new("Continue").v(BVariant::Primary).size(BSize::Lg).def(true).disabled(busy).class("min-w-[10rem]"), Some(Msg::Continue)));
    if address.is_some() {
        actions = actions.kid(button_el(ButtonSpec::new("Change").size(BSize::Lg).v(BVariant::Ghost).disabled(busy), Some(Msg::Change)));
    }
    let title = name.map(|n| format!("You're @{n}")).unwrap_or_else(|| "You're signed in".into());
    let mut right = div("min-w-0");
    if let Some(a) = address {
        right = right.kid(panel(
            "flex max-w-[30rem] items-center gap-[1.25rem] px-[1.6rem] py-[1.5rem]",
            vec![
                account_avatar(a, "4.5rem"),
                span("min-w-0").kid(span("block text-label text-muted").text("How friends see you")).kid(span("block truncate text-title font-semibold").text(name.map(|n| format!("@{n}")).unwrap_or_else(|| "Your username".into()))),
            ],
        ));
    }
    div("grid h-full grid-cols-[minmax(0,26rem)_minmax(0,1fr)] items-center gap-[4rem]")
        .kid(copy(&title, Some("Friends find you by your username. It is free, and you can change it from your profile."), vec![actions]))
        .kid(right)
        // signed in already: nothing to go back to
        .on_pad("b", Msg::Mode(Mode::Named))
}

/// `suggest`: player + four hex digits of the address, the first of four tries that is free.
pub fn suggestions(address: &str) -> Vec<String> {
    let hex = address.trim_start_matches("0x").to_lowercase();
    (0..4).filter_map(|k| hex.get(k * 4..k * 4 + 4).map(|s| format!("player{s}"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::ME;

    #[test]
    fn start() {
        let v = view(Mode::Start, FlowStep::Choose, true, None, None);
        let t = v.texts();
        assert_eq!(&t[..2], ["Create your account", "One account for Rare Wallet, Friends and Rare Marketplace. It is kept on this SuiPlay and signs with A. You write down 12 words once as a backup."]);
        assert!(t.contains(&"Signs with A. Back up 12 words once".to_string()));
        assert_eq!(v.landing().as_deref(), Some("Create account"));
        let v = view(Mode::Start, FlowStep::Choose, false, None, None);
        assert!(v.contains_text("No internet. You can create your account now; Friends and the Marketplace need it later."));
        let v = view(Mode::Create, FlowStep::Pin, true, Some(div("").text("flow")), None);
        assert_eq!(v.texts()[..3], ["Create account", "This device signs everyday transactions with the PIN. Your passkey can always sign too.", "flow"]);
    }

    #[test]
    fn named() {
        let v = named_view(Some("player7d20"), Some(ME), false);
        assert_eq!(v.texts(), ["You're @player7d20", "Friends find you by your username. It is free, and you can change it from your profile.", "Continue", "Change", "How friends see you", "@player7d20"]);
        assert_eq!(suggestions(ME), ["player7d20", "playerdcdb", "player2bca", "player4f50"]);
    }
}
