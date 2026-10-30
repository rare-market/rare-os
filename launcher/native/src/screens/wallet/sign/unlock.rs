//! The PIN prompt as a sheet (launcher/src/wallet/ui/sign/UnlockSheet.tsx, unlockCopy.ts,
//! ui/unlock.ts): shows why, takes the PIN on the kit pad (or a passphrase on the keyboard when the
//! vault uses one), and counts wrong tries and waits in plain words. The PIN never shows, only dots.

use crate::screens::wallet::kit::{btn, field, pin_pad, pin_press, sheet, FieldSpec, PinPadSpec, SheetSpec, Variant};
use crate::screens::wallet::markup::*;

const MAX_TRIES: u32 = 5;

/// `wrongMessage`.
pub fn wrong_message(failures: Option<u32>, what: &str) -> String {
    let n = failures.unwrap_or(0);
    let left = MAX_TRIES.saturating_sub(n).max(1);
    format!("Wrong {what}. {left} {} before a wait.", if left == 1 { "try" } else { "tries" })
}

/// `waitText`: "12 s", "2 min".
pub fn wait_text(ms: u64) -> String {
    let s = ms.div_ceil(1000).max(1);
    if s >= 60 {
        format!("{} min", s.div_ceil(60))
    } else {
        format!("{s} s")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultKind {
    Pin,
    Passphrase,
}

/// A pending `requestUnlock(reason)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub reason: String,
    /// A passkey account: "Use my phone instead" signs with the passkey (QR) with no PIN.
    pub phone: bool,
    /// The sheet's title ("Unlock Rare Wallet", "Enter your PIN").
    pub title: Option<String>,
    /// The other way's button ("Use my phone instead" when None).
    pub phone_label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub value: String,
    pub error: Option<String>,
    pub busy: bool,
    /// monotonic ms until which tries wait (0 = none)
    pub until: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Key(&'static str),
    /// A opened the keyboard on the passphrase field
    Type,
    /// the keyboard's answer
    Typed(String),
    Cancel,
    /// sign with the passkey on the phone instead of the PIN
    Phone,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// `keystore.unlock(secret)`; answer with `on_result`
    Unlock(String),
    /// open the keyboard: title "Passphrase", secret
    AskPassphrase,
    /// `finishUnlock(ok)`: close the sheet and resolve the waiting request
    Finish(bool),
    /// close the sheet and sign with the passkey on the phone
    Phone,
}

/// What `keystore.unlock` answered.
#[derive(Clone, Debug, PartialEq)]
pub enum UnlockError {
    /// NativeError "wrong": failures so far, and a wait when the vault imposes one
    Wrong { failures: Option<u32>, retry_after_ms: u64 },
    Throttled { retry_after_ms: Option<u64> },
    Other(Option<String>),
}

pub fn view(pending: &Pending, kind: VaultKind, st: &State, now: u64) -> El<Msg> {
    let wait = st.until.saturating_sub(now);
    let waiting = st.until > 0 && wait > 0;
    let entry = match kind {
        VaultKind::Passphrase => div("w-full").kid(field(FieldSpec { label: Some("Passphrase"), value: &st.value, secret: true, def: true, ..Default::default() }, Msg::Type)),
        VaultKind::Pin => pin_pad(PinPadSpec { value: &st.value, disabled: st.busy || waiting, ..Default::default() }, Msg::Key).on_pad("x", Msg::Key("back")).on_pad("start", Msg::Key("ok")),
    };
    let status = if waiting {
        Some(p("max-w-[26rem] text-center text-sm text-secondary").role("status").text(format!("Too many tries. Try again in {}.", wait_text(wait))))
    } else {
        st.error.as_ref().map(|e| p("max-w-[26rem] text-center text-sm text-negative").role("status").text(e))
    };
    let mut footer = Vec::new();
    if pending.phone {
        footer.push(btn(pending.phone_label.as_deref().unwrap_or("Use my phone instead"), Variant::Secondary, "flex-1", Some(Msg::Phone)));
    }
    footer.push(btn("Cancel", Variant::Secondary, "flex-1", Some(Msg::Cancel)));
    sheet(
        SheetSpec {
            title: pending.title.clone().unwrap_or_else(|| "Unlock Rare Wallet".into()),
            subtitle: None,
            icon: None,
            footer: Some(footer),
            close: Some(Msg::Cancel),
            width: "28rem",
        },
        vec![div("flex flex-col items-center gap-5")
            .kid(p("text-center text-base text-secondary").text(&pending.reason))
            .kid(entry)
            .kid(div("flex min-h-5 justify-center").kid_opt(status))],
    )
}

/// Applies a message. `now` is monotonic ms.
pub fn update(st: &mut State, msg: Msg, now: u64) -> Option<Effect> {
    let waiting = st.until > now;
    match msg {
        Msg::Cancel => Some(Effect::Finish(false)),
        Msg::Phone => Some(Effect::Phone),
        Msg::Type => Some(Effect::AskPassphrase),
        Msg::Typed(v) => {
            st.value = v.clone();
            if v.is_empty() {
                None
            } else {
                submit(st, v, waiting)
            }
        }
        Msg::Key(k) => {
            if st.busy || waiting {
                return None;
            }
            let (next, go) = pin_press(&st.value, k, None, 6, 12);
            st.value = next.clone();
            if go {
                submit(st, next, waiting)
            } else {
                None
            }
        }
    }
}

fn submit(st: &mut State, secret: String, waiting: bool) -> Option<Effect> {
    if st.busy || waiting {
        return None;
    }
    st.busy = true;
    st.error = None;
    Some(Effect::Unlock(secret))
}

/// The answer to `Effect::Unlock`.
pub fn on_result(st: &mut State, kind: VaultKind, res: Result<(), UnlockError>, now: u64) -> Option<Effect> {
    st.busy = false;
    match res {
        Ok(()) => return Some(Effect::Finish(true)),
        Err(e) => {
            st.value.clear();
            match e {
                UnlockError::Wrong { retry_after_ms, .. } if retry_after_ms > 0 => st.until = now + retry_after_ms,
                UnlockError::Wrong { failures, .. } => st.error = Some(wrong_message(failures, if kind == VaultKind::Passphrase { "passphrase" } else { "PIN" })),
                UnlockError::Throttled { retry_after_ms } => st.until = now + retry_after_ms.unwrap_or(5000),
                UnlockError::Other(m) => st.error = Some(m.unwrap_or_else(|| "Could not unlock".into())),
            }
        }
    }
    None
}

/// The 250 ms tick while waiting: when the wait runs out, the error clears.
pub fn tick(st: &mut State, now: u64) {
    if st.until > 0 && now >= st.until {
        st.until = 0;
        st.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending() -> Pending {
        Pending { reason: "Enter your PIN to approve".into(), phone: false, title: None, phone_label: None }
    }

    #[test]
    fn copy() {
        assert_eq!(wrong_message(Some(1), "PIN"), "Wrong PIN. 4 tries before a wait.");
        assert_eq!(wrong_message(Some(4), "PIN"), "Wrong PIN. 1 try before a wait.");
        assert_eq!(wrong_message(Some(9), "passphrase"), "Wrong passphrase. 1 try before a wait.");
        assert_eq!(wait_text(1), "1 s");
        assert_eq!(wait_text(30_000), "30 s");
        assert_eq!(wait_text(61_000), "2 min");
    }

    #[test]
    fn pin_flow() {
        let mut st = State::default();
        let v = view(&pending(), VaultKind::Pin, &st, 0);
        assert_eq!(v.texts()[0], "Unlock Rare Wallet");
        assert_eq!(v.texts()[1], "Enter your PIN to approve");
        assert_eq!(v.landing().as_deref(), Some("5"));
        for k in ["1", "2", "3", "4", "5"] {
            assert_eq!(update(&mut st, Msg::Key(k), 0), None);
        }
        assert_eq!(update(&mut st, Msg::Key("6"), 0), None);
        assert_eq!(update(&mut st, Msg::Key("ok"), 0), Some(Effect::Unlock("123456".into())));
        // the typed PIN is never in the tree
        assert!(!view(&pending(), VaultKind::Pin, &st, 0).texts().iter().any(|t| t.contains("123456")));
        assert_eq!(on_result(&mut st, VaultKind::Pin, Err(UnlockError::Wrong { failures: Some(2), retry_after_ms: 0 }), 10), None);
        assert_eq!(st.value, "");
        assert!(view(&pending(), VaultKind::Pin, &st, 10).contains_text("Wrong PIN. 3 tries before a wait."));
        on_result(&mut st, VaultKind::Pin, Err(UnlockError::Wrong { failures: Some(5), retry_after_ms: 30_000 }), 100);
        let v = view(&pending(), VaultKind::Pin, &st, 100);
        assert!(v.contains_text("Too many tries. Try again in 30 s."));
        assert!(v.find_nav("5").unwrap().disabled);
        assert_eq!(update(&mut st, Msg::Key("1"), 200), None);
        tick(&mut st, 30_100);
        assert_eq!(st.until, 0);
        assert_eq!(on_result(&mut st, VaultKind::Pin, Ok(()), 0), Some(Effect::Finish(true)));
        assert_eq!(kit_b(&v), Some(Msg::Cancel));
    }

    fn kit_b(v: &El<Msg>) -> Option<Msg> {
        crate::screens::wallet::kit::sheet_b(v)
    }

    #[test]
    fn passphrase_flow() {
        let mut st = State::default();
        let v = view(&pending(), VaultKind::Passphrase, &st, 0);
        assert_eq!(v.landing().as_deref(), Some(""));
        assert_eq!(update(&mut st, Msg::Type, 0), Some(Effect::AskPassphrase));
        assert_eq!(update(&mut st, Msg::Typed("correct horse".into()), 0), Some(Effect::Unlock("correct horse".into())));
        let v = view(&pending(), VaultKind::Passphrase, &st, 0);
        assert!(!v.content().contains("correct horse"));
        on_result(&mut st, VaultKind::Passphrase, Err(UnlockError::Wrong { failures: None, retry_after_ms: 0 }), 0);
        assert!(view(&pending(), VaultKind::Passphrase, &st, 0).contains_text("Wrong passphrase. 5 tries before a wait."));
        on_result(&mut st, VaultKind::Passphrase, Err(UnlockError::Throttled { retry_after_ms: None }), 0);
        assert_eq!(st.until, 5000);
    }
}
