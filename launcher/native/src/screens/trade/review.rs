//! Review before signing (trade/ReviewSheet.tsx): what the trade does, then the unsigned transaction
//! is built and dry-run (nothing is signed or sent); Confirm hands it to Rare Wallet, whose own
//! review asks for the approval. B closes. The phase comes from the app (rare-trade builds,
//! rare-sui dry-runs, rare-wallet signs); fixtures set it directly.

use super::ui::{action, lines, Action, Tone, Val};
use crate::kit::{button, sheet, Btn};
use crate::screens::market::host::*;
use rare_render::view::El;

pub const T_CONFIRM: u32 = 9_100;
pub const T_CANCEL: u32 = 9_101;
pub const T_DONE: u32 = 9_102;
pub const T_PASSKEY: u32 = 9_103;
/// The focus scope a review sheet traps.
pub const SCOPE: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Checking,
    /// Dry run passed: the network fee in MIST, or covered by Rare's sponsor.
    Ready { gas_mist: Option<u64>, covered: bool },
    Failed(String),
    /// Rare Wallet's own sheet has the screen.
    Signing,
    Sent { simulated: bool },
}

/// One review: title, line, the summary lines, the confirm button's words and tone.
#[derive(Clone, Debug)]
pub struct Review {
    pub title: String,
    pub sub: Option<String>,
    pub action: String,
    pub tone: Tone,
    pub lines: Vec<(String, Val, bool)>,
}

/// What the sheet shows for a phase: the gas line and the sentence under the lines.
pub fn status(phase: &Phase, wallet: bool, signer: bool) -> (Option<String>, Option<(String, bool)>) {
    let gas = match phase {
        Phase::Ready { covered: true, .. } => Some("Covered by Rare".to_string()),
        Phase::Ready { gas_mist: Some(g), .. } => Some(format!("about {} SUI", super::fmt::trim_zeros(&format!("{}", super::fmt::precision_num(*g as f64 / 1e9, 2))))),
        _ => None,
    };
    let line = if !wallet {
        None
    } else {
        match phase {
            Phase::Checking => Some(("Checking the trade...".to_string(), false)),
            Phase::Ready { .. } if signer => Some(("Checked against the network just now. Rare Wallet shows the final details before anything is signed.".into(), false)),
            Phase::Ready { .. } => None,
            Phase::Failed(m) => Some((m.clone(), true)),
            Phase::Sent { simulated: true } => Some(("Signed as a test and checked against the network. Nothing was sent.".into(), false)),
            Phase::Sent { simulated: false } => Some(("Sent. It shows in Positions once the network has it.".into(), false)),
            Phase::Signing => None,
        }
    };
    (gas, line)
}

/// The sheet for `r` in `phase`. `wallet`: a Rare Wallet account is selected; `signer`: it can sign
/// (not watch-only). None while signing (Rare Wallet's sheet is up).
pub fn review_sheet_in(ui: &mut Ui, r: &Review, phase: &Phase, wallet: bool, signer: bool) -> Option<El> {
    if *phase == Phase::Signing {
        return None;
    }
    let ready = matches!(phase, Phase::Ready { .. });
    let (gas, line) = status(phase, wallet, signer);
    let mut items: Vec<(&str, Val, bool)> = r.lines.iter().map(|(l, v, s_)| (l.as_str(), v.clone(), *s_)).collect();
    if let Some(g) = &gas {
        items.push(("Network gas", Val::t(g.clone()), false));
    }
    let summary = lines(ui, &items);
    let mut note = ui.bx(s().col().mt(1.0).min_h(2.5));
    let passkey = !wallet || (ready && !signer);
    if passkey {
        // AddPasskey need="trade": the line and its button.
        let (text, label) = if wallet { ("Add a passkey to trade.", "Add a passkey") } else { ("Sign in with a passkey to trade.", "Sign in") };
        let b = button(ui, Btn { hint: Some(label.into()), ..Btn::new(label, T_PASSKEY).primary() });
        note = note.child(ui.bx(s().col().items_start().gap(0.75)).child(ui.text(text, T::new(Ty::Small).c(tok::MUTED))).child(b));
    } else if let Some((t, bad)) = line {
        note = note.child(ui.text(t, T::new(Ty::Small).c(if bad { tok::NEG } else { tok::MUTED }).wrap()));
    }
    let mut body_box = ui.bx(s().col());
    if *phase == Phase::Checking {
        // LoadBar -top-[1.25rem]: the 2 px bar along the body's top edge.
        body_box = body_box.child(ui.bx(s().abs(Some(-1.25), Some(-2.0), None, Some(-2.0)).h_px(2.0)).bg(tok::BRAND.alpha(0.6)));
    }
    let body = vec![body_box.child(summary).child(note)];
    let footer = if let Phase::Sent { .. } = phase {
        vec![action(ui, s().flex(1.0), Action { def: true, ..Action::new("Done", Tone::Secondary, T_DONE) })]
    } else {
        let can = ready && signer;
        vec![
            action(ui, s().flex(1.0), Action { def: can, disabled: !can, hint: can.then(|| "Confirm".to_string()), ..Action::new(r.action.clone(), r.tone, T_CONFIRM) }),
            action(ui, s(), Action { def: !can, ..Action::new("Cancel", Tone::Secondary, T_CANCEL) }),
        ]
    };
    let sh = sheet(ui, &r.title, r.sub.as_deref(), body, footer, false, SCOPE);
    Some(in_flow_sheet(ui, sh))
}

thread_local! {
    /// The live review's phase and whether the account can sign (set by `trade::live`); None: the
    /// fixtures' look.
    static LIVE: std::cell::RefCell<Option<(Phase, bool)>> = const { std::cell::RefCell::new(None) };
}

/// Sets the phase the open review sheet shows (one review is open at a time).
pub fn set_live(p: Option<(Phase, bool)>) {
    LIVE.with(|l| *l.borrow_mut() = p);
}

/// The sheet for the open review: the live phase, or as the fixtures show it (checked and ready
/// with a typical fee).
pub fn review_sheet(ui: &mut Ui, r: &Review, wallet: bool) -> El {
    let (phase, signer) = LIVE.with(|l| l.borrow().clone()).unwrap_or((Phase::Ready { gas_mist: Some(2_900_000), covered: false }, false));
    review_sheet_in(ui, r, &phase, wallet, signer).unwrap_or_else(|| ui.bx(s()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_words() {
        assert_eq!(status(&Phase::Checking, true, true).1.unwrap().0, "Checking the trade...");
        assert_eq!(status(&Phase::Ready { gas_mist: Some(2_900_000), covered: false }, true, true).0.unwrap(), "about 0.0029 SUI");
        assert_eq!(status(&Phase::Ready { gas_mist: None, covered: true }, true, true).0.unwrap(), "Covered by Rare");
        assert!(status(&Phase::Failed("The trade didn't go through: x".into()), true, true).1.unwrap().1);
        assert_eq!(status(&Phase::Sent { simulated: true }, true, true).1.unwrap().0, "Signed as a test and checked against the network. Nothing was sent.");
        assert!(status(&Phase::Ready { gas_mist: None, covered: false }, true, false).1.is_none());
    }
}
