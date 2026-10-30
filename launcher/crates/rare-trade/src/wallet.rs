//! The Trade tab's hand-off to Rare Wallet, and the review sheet's flow before it. Every trade is
//! built as an unsigned transaction and checked with a dry run; confirming hands it to
//! `rare-wallet` (its own review, PIN or device, sign, submit) as a [`SignRequest`]. Nothing here
//! signs.

use crate::amount::from_base;
use crate::chain::{plain_chain_error, DryRun};
use crate::jsnum::{js_number, to_precision};
use crate::tx::UnsignedTx;

/// Who asks Rare Wallet to sign.
pub const ORIGIN_NAME: &str = "Rare Trade";
pub const ORIGIN_KIND: &str = "market";

/// The kind of Rare Wallet account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    /// Passkey, PIN or device accounts that can approve.
    Signer,
    /// Watch-only: never signs.
    Watch,
}

/// Whether this account could approve a trade (watch-only accounts never can).
pub fn can_sign(account: Option<AccountKind>) -> bool {
    account == Some(AccountKind::Signer)
}

/// What Rare Wallet is asked to review, sign and submit.
#[derive(Clone, Debug, PartialEq)]
pub struct SignRequest {
    pub tx: UnsignedTx,
    pub account: String,
    pub title: String,
    pub origin_name: &'static str,
    pub origin_kind: &'static str,
    /// Rare covers the fee: the sponsor's signed transaction (from rare.market's sponsor), opaque.
    pub sponsorship: Option<String>,
}

/// What came back from Rare Wallet.
#[derive(Clone, Debug, PartialEq)]
pub enum SignOutcome {
    /// On the network (or, for a test signature, only checked against it).
    Sent { digest: Option<String>, dry_run: bool },
    /// The person declined in Rare Wallet's review: say nothing, back to the review.
    Rejected,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Checking,
    Ready { tx: UnsignedTx, run: DryRun, sponsorship: Option<String> },
    Failed { message: String },
    Signing,
    Sent { digest: Option<String>, simulated: bool },
}

/// The review sheet: what the trade does, then the unsigned transaction is built and dry-run;
/// Confirm hands it to Rare Wallet.
#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    pub title: String,
    pub phase: Phase,
    ready: Option<Phase>,
}

impl Review {
    pub fn new(title: &str) -> Review {
        Review { title: title.to_string(), phase: Phase::Checking, ready: None }
    }

    /// The build and its dry run are back.
    pub fn checked(&mut self, tx: UnsignedTx, run: DryRun, sponsorship: Option<String>) {
        self.phase = if run.ok {
            Phase::Ready { tx, run, sponsorship }
        } else {
            Phase::Failed { message: format!("The trade didn't go through: {}", run.error.as_deref().unwrap_or("it would fail right now")) }
        };
    }

    /// The build itself failed (the reason goes to the log, not the screen).
    pub fn build_failed(&mut self) {
        self.phase = Phase::Failed { message: "Couldn't build this trade. Try again.".into() };
    }

    /// A sponsored attempt is only offered when its dry run passes; otherwise the wallet pays.
    pub fn sponsored_usable(run: &DryRun) -> bool {
        run.ok
    }

    /// Confirm: the request for Rare Wallet, when the trade is ready and the account can sign.
    pub fn confirm(&mut self, account: &str, signer: bool) -> Option<SignRequest> {
        if !signer {
            return None;
        }
        let Phase::Ready { tx, sponsorship, .. } = &self.phase else { return None };
        let req = SignRequest { tx: tx.clone(), account: account.to_string(), title: self.title.clone(), origin_name: ORIGIN_NAME, origin_kind: ORIGIN_KIND, sponsorship: sponsorship.clone() };
        self.ready = Some(std::mem::replace(&mut self.phase, Phase::Signing));
        Some(req)
    }

    /// Rare Wallet's answer. True when the trade went out (refresh the portfolio or perps account).
    pub fn signed(&mut self, r: SignOutcome) -> bool {
        match r {
            SignOutcome::Sent { digest, dry_run } => {
                self.phase = Phase::Sent { digest, simulated: dry_run };
                !dry_run
            }
            SignOutcome::Rejected => {
                self.phase = self.ready.take().unwrap_or(Phase::Checking);
                false
            }
            SignOutcome::Failed(e) => {
                self.phase = Phase::Failed { message: format!("The trade didn't go through: {}", plain_chain_error(&e)) };
                false
            }
        }
    }

    /// The network gas line, when there is one to show.
    pub fn gas_line(&self) -> Option<String> {
        let Phase::Ready { run, sponsorship, .. } = &self.phase else { return None };
        if sponsorship.is_some() {
            return Some("Covered by Rare".into());
        }
        let gas = run.gas_mist?;
        let mist = if gas > 0 { gas as u128 } else { 0 };
        Some(format!("about {} SUI", to_precision(js_number(&from_base(mist, 9)), 2)))
    }

    /// The line under the summary.
    pub fn status(&self, has_sender: bool, signer: bool) -> Status {
        if !has_sender {
            return Status::AddPasskey;
        }
        match &self.phase {
            Phase::Checking => Status::Text("Checking the trade..."),
            Phase::Ready { .. } if signer => Status::Text("Checked against the network just now. Rare Wallet shows the final details before anything is signed."),
            Phase::Ready { .. } => Status::AddPasskey,
            Phase::Failed { message } => Status::Error(message.clone()),
            Phase::Sent { simulated: true, .. } => Status::Text("Signed as a test and checked against the network. Nothing was sent."),
            Phase::Sent { .. } => Status::Text("Sent. It shows in Positions once the network has it."),
            Phase::Signing => Status::Hidden,
        }
    }

    /// The confirm button is live.
    pub fn can_confirm(&self, signer: bool) -> bool {
        signer && matches!(self.phase, Phase::Ready { .. })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Text(&'static str),
    Error(String),
    /// The account cannot sign yet: offer to add a passkey.
    AddPasskey,
    /// Rare Wallet's own sheet has the screen.
    Hidden,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ok: bool, gas: Option<i128>) -> DryRun {
        DryRun { ok, error: (!ok).then(|| "Not enough balance for this trade.".to_string()), changes: vec![], gas_mist: gas, raw: None }
    }
    fn tx() -> UnsignedTx {
        UnsignedTx::Kind { kind: "AA==".into(), sender: "0x1".into(), gas_from_spent_sui: true }
    }

    #[test]
    fn review_flow() {
        let mut r = Review::new("Buy MEME");
        assert_eq!(r.status(true, true), Status::Text("Checking the trade..."));
        r.checked(tx(), run(false, None), None);
        assert_eq!(r.status(true, true), Status::Error("The trade didn't go through: Not enough balance for this trade.".into()));
        r.checked(tx(), run(true, Some(4_723_677)), None);
        assert_eq!(r.gas_line().as_deref(), Some("about 0.0047 SUI"));
        assert!(r.confirm("0x1", false).is_none());
        let req = r.confirm("0x1", true).unwrap();
        assert_eq!((req.title.as_str(), req.origin_name), ("Buy MEME", "Rare Trade"));
        assert_eq!(r.status(true, true), Status::Hidden);
        assert!(!r.signed(SignOutcome::Rejected));
        assert!(r.can_confirm(true));
        r.confirm("0x1", true).unwrap();
        assert!(r.signed(SignOutcome::Sent { digest: Some("D".into()), dry_run: false }));
        assert_eq!(r.status(true, true), Status::Text("Sent. It shows in Positions once the network has it."));
        let mut r = Review::new("x");
        r.checked(tx(), run(true, None), Some("sponsored".into()));
        assert_eq!(r.gas_line().as_deref(), Some("Covered by Rare"));
        r.confirm("0x1", true);
        assert!(!r.signed(SignOutcome::Failed("MoveAbort abort code: 30".into())));
        assert_eq!(r.phase, Phase::Failed { message: "The trade didn't go through: Not enough margin for this order. Add funds or lower the size.".into() });
        r.build_failed();
        assert_eq!(r.status(false, false), Status::AddPasskey);
        assert!(can_sign(Some(AccountKind::Signer)) && !can_sign(Some(AccountKind::Watch)) && !can_sign(None));
    }
}
