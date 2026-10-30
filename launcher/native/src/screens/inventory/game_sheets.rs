//! The sheets a game (Local API, rare:platform) or a `rare://` link opens (launcher/src/localapi/
//! GameSheets.tsx): sell, buy and claim. Sell reuses the overlay's item detail (workstream 13) and
//! buy the Marketplace's BuySheet (workstream 15), mounted as slots; claim is a one-line confirm.
//! Anything signed goes through Rare Wallet's review as always. B (or Not now) answers the game
//! "cancelled". While a sheet is up the launcher's tab and panel buttons do nothing.

use crate::screens::wallet::kit::{sheet, SheetSpec};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, dot, BSize, ButtonSpec, BVariant, Tone};

/// The game that asked (or none: a link the player opened).
#[derive(Clone, Debug, PartialEq)]
pub struct Asker {
    /// a Local API request id; None for a link
    pub req_id: Option<u64>,
    pub game_title: Option<String>,
}

/// `askedBy`: "<game> asked Rare", or None for a link.
pub fn asked_by(a: &Asker) -> Option<String> {
    match (&a.req_id, &a.game_title) {
        (Some(_), Some(t)) if !t.is_empty() => Some(format!("{t} asked Rare")),
        _ => None,
    }
}

/// localapi voucher (`ClaimSheetData.voucher`).
#[derive(Clone, Debug, PartialEq)]
pub enum Voucher {
    Trophy { trophy_id: u64 },
    Item { kind: u64, keys: Vec<String>, vals: Vec<String> },
    Stats { keys: Vec<String>, vals: Vec<String> },
}

/// What the claim sheet says it gives.
pub fn voucher_line(v: &Voucher) -> String {
    match v {
        Voucher::Trophy { trophy_id } => format!("Trophy {trophy_id}"),
        Voucher::Item { kind, keys, vals } => {
            let with = if keys.is_empty() { String::new() } else { format!(" with {}", keys.iter().zip(vals).map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ")) };
            format!("Item kind {kind}{with}")
        }
        Voucher::Stats { keys, vals } => keys.iter().zip(vals).map(|(k, v)| format!("{k} becomes {v}")).collect::<Vec<_>>().join(", "),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClaimPhase {
    Idle,
    Working,
    Error(String),
    Done(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Claim,
    /// B or Not now: the game hears "cancelled" (or "claimed" once done)
    Close,
    /// the launcher's buttons while the sheet is up
    Swallow,
}

/// The game's answer for the sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Cancelled,
    Claimed(String),
    Unavailable(String),
}

const SWALLOWED: [&str; 9] = ["lb", "rb", "lt", "rt", "x", "y", "select", "start", "qam"];

fn swallow(mut e: El<Msg>) -> El<Msg> {
    for b in SWALLOWED {
        e = e.on_pad(b, Msg::Swallow);
    }
    e
}

/// `ClaimSheet`. `has_account`: a Rare Wallet account exists.
pub fn claim_view(title: &str, asker: &Asker, v: &Voucher, phase: &ClaimPhase, has_account: bool) -> El<Msg> {
    let done = matches!(phase, ClaimPhase::Done(_));
    let footer = (!done).then(|| {
        vec![div("flex gap-[0.6rem]")
            .kid(button_el(
                ButtonSpec::new(match phase {
                    ClaimPhase::Working => "Waiting for approval",
                    ClaimPhase::Error(_) => "Try again",
                    _ => "Claim",
                })
                .v(BVariant::Primary)
                .size(BSize::Md)
                .def(true)
                .class("flex-1")
                .disabled(*phase == ClaimPhase::Working || !has_account)
                .hint("Claim"),
                Some(Msg::Claim),
            ))
            .kid(button_el(ButtonSpec::new("Not now").size(BSize::Md), Some(Msg::Close)))]
    });
    let mut body = div("flex flex-col gap-[0.8rem] px-8 py-6 text-small").kid(p("text-body").text(voucher_line(v))).kid(p("text-secondary").text(if matches!(v, Voucher::Trophy { .. }) {
        "Goes to your Rare profile. Rare covers the network fee when your wallet is short."
    } else {
        "Goes to your Rare Wallet. You review it before anything is signed."
    }));
    if done {
        body = body.kid(p("flex items-center gap-[0.6rem]").kid(dot(Tone::Positive, false, "")).kid(text(" Done. It can take a minute to show.")));
    }
    if let ClaimPhase::Error(m) = phase {
        body = body.kid(p("text-negative").text(m));
    }
    if !has_account {
        body = body.kid(p("text-secondary").text("Add a Rare Wallet account to claim."));
    }
    let s = sheet(
        SheetSpec {
            title: if done { "Claimed".into() } else { title.into() },
            subtitle: Some(asked_by(asker).unwrap_or_else(|| "From a Rare link".into())),
            icon: None,
            footer,
            close: Some(Msg::Close),
            width: "34rem",
        },
        vec![body],
    )
    .attr("data-ready", match phase {
        ClaimPhase::Idle => "idle",
        ClaimPhase::Working => "working",
        ClaimPhase::Error(_) => "error",
        ClaimPhase::Done(_) => "done",
    });
    swallow(s)
}

/// Claim pressed: Some(true) starts the claim (game's gas budget or Rare pays).
pub fn claim_press(phase: &mut ClaimPhase, has_account: bool) -> bool {
    if !has_account || matches!(phase, ClaimPhase::Working | ClaimPhase::Done(_)) {
        return false;
    }
    *phase = ClaimPhase::Working;
    true
}

/// The claim's answer: the digest, declined in the wallet, unavailable (the sheet closes with
/// that answer), or the error.
pub enum ClaimResult {
    Done(String),
    Rejected,
    Unavailable(String),
    Failed(Option<String>),
}

pub fn claimed(phase: &mut ClaimPhase, r: ClaimResult) -> Option<Answer> {
    match r {
        ClaimResult::Done(d) => *phase = ClaimPhase::Done(d),
        ClaimResult::Rejected => *phase = ClaimPhase::Idle,
        ClaimResult::Unavailable(m) => return Some(Answer::Unavailable(m)),
        ClaimResult::Failed(m) => *phase = ClaimPhase::Error(m.filter(|s| !s.is_empty()).unwrap_or_else(|| "The claim didn't go through".into())),
    }
    None
}

/// Close: what the game hears.
pub fn close_answer(phase: &ClaimPhase) -> Answer {
    match phase {
        ClaimPhase::Done(d) => Answer::Claimed(d.clone()),
        _ => Answer::Cancelled,
    }
}

/// `SellSheet`: "Sell <short name>" over the overlay's item detail (slot `overlay-item-detail`).
pub fn sell_view(short_name: &str, asker: &Asker, collection: Option<&str>, game_title: &str, done: bool) -> El<Msg> {
    let sub = asked_by(asker).or(collection.map(str::to_string)).unwrap_or_else(|| game_title.to_string());
    swallow(
        sheet(
            SheetSpec { title: format!("Sell {short_name}"), subtitle: Some(sub), icon: None, footer: None, close: Some(Msg::Close), width: "34rem" },
            vec![div("py-[0.6rem]").kid(slot("overlay-item-detail"))],
        )
        .attr("data-ready", if done { "done" } else { "open" }),
    )
}

/// `BuyGameSheet`: the Marketplace's BuySheet (slot `marketplace-buy-sheet`); B answers bought or cancelled.
pub fn buy_view() -> El<Msg> {
    swallow(div("").kid(slot("marketplace-buy-sheet")).on_pad("b", Msg::Close))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim() {
        let asker = Asker { req_id: Some(7), game_title: Some("ACT".into()) };
        let v = Voucher::Item { kind: 3, keys: vec!["level".into()], vals: vec!["2".into()] };
        let mut ph = ClaimPhase::Idle;
        let s = claim_view("Claim your reward", &asker, &v, &ph, true);
        assert_eq!(s.texts(), ["Claim your reward", "ACT asked Rare", "Item kind 3 with level 2", "Goes to your Rare Wallet. You review it before anything is signed.", "Claim", "Not now"]);
        assert_eq!(s.landing().as_deref(), Some("Claim"));
        assert_eq!(s.pad("lb"), Some(&Msg::Swallow));
        assert!(claim_press(&mut ph, true));
        assert!(claim_view("Claim your reward", &asker, &v, &ph, true).find_nav("Waiting for approval").unwrap().disabled);
        assert_eq!(claimed(&mut ph, ClaimResult::Failed(None)), None);
        assert!(claim_view("x", &asker, &v, &ph, true).contains_text("The claim didn't go through"));
        claimed(&mut ph, ClaimResult::Done("0xd".into()));
        let s = claim_view("x", &asker, &v, &ph, true);
        assert_eq!(s.texts()[0], "Claimed");
        assert!(s.contains_text(" Done. It can take a minute to show."));
        assert_eq!(close_answer(&ph), Answer::Claimed("0xd".into()));
        let link = Asker { req_id: None, game_title: Some("ACT".into()) };
        let s = claim_view("Trophy", &link, &Voucher::Trophy { trophy_id: 12 }, &ClaimPhase::Idle, false);
        assert_eq!(s.texts()[1], "From a Rare link");
        assert!(s.contains_text("Goes to your Rare profile. Rare covers the network fee when your wallet is short."));
        assert!(s.contains_text("Add a Rare Wallet account to claim."));
        assert_eq!(voucher_line(&Voucher::Stats { keys: vec!["hp".into(), "xp".into()], vals: vec!["10".into(), "5".into()] }), "hp becomes 10, xp becomes 5");
    }

    #[test]
    fn sell_and_buy() {
        let s = sell_view("#1204", &Asker { req_id: Some(1), game_title: Some("Pawtato".into()) }, Some("Pawtato Heroes"), "Pawtato", false);
        assert_eq!(s.texts()[..2], ["Sell #1204", "Pawtato asked Rare"]);
        assert!(s.dump().contains("slot overlay-item-detail"));
        assert!(buy_view().dump().contains("slot marketplace-buy-sheet"));
    }
}
