//! Sheets used by Settings and Accounts (launcher/src/wallet/ui/home/Sheets.tsx): PIN entry, reveal
//! the recovery phrase or key, change PIN, remove an account, remove everything, hardware wallet
//! help. The phrase or key shows only after the vault opened for it, as on the web, and leaves
//! with the sheet (`State` is dropped).

use super::lib::kind_label;
use crate::screens::wallet::kit::{btn, btn_full, field, pin_pad, pin_press, sheet, FieldSpec, PinPadSpec, SheetSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::{RareAccount, SecretKind};

// ------------------------------------------------------------------ SecretEntry

/// `SecretEntry`: the PIN pad (or passphrase field) that hands the typed secret on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub value: String,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EntryMsg {
    Key(&'static str),
    /// A on the passphrase field: open the keyboard ("Passphrase", secret)
    Type,
    Typed(String),
    /// the passphrase OK button
    Submit,
}

/// What the entry did: hand a secret on, or ask for the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryOut {
    Submit(String),
    AskPassphrase,
}

pub fn entry_view(kind: SecretKind, prompt: &str, ok_label: &str, e: &Entry) -> El<EntryMsg> {
    let mut d = div("flex flex-col items-center gap-5").kid(p("text-center text-base text-secondary").text(prompt));
    d = match kind {
        SecretKind::Passphrase => d.kid(
            div("flex w-full max-w-[26rem] flex-col gap-3")
                .kid(field(FieldSpec { value: &e.value, secret: true, def: true, placeholder: Some("Passphrase"), ..Default::default() }, EntryMsg::Type))
                .kid(btn(ok_label, Variant::Primary, "", Some(EntryMsg::Submit))),
        ),
        SecretKind::Pin => d.kid(pin_pad(PinPadSpec { value: &e.value, disabled: e.busy, ok_label, ..Default::default() }, EntryMsg::Key).on_pad("x", EntryMsg::Key("back")).on_pad("start", EntryMsg::Key("ok"))),
    };
    if let Some(err) = &e.error {
        d = d.kid(p("text-center text-sm text-negative").text(err));
    }
    d
}

pub fn entry_update(kind: SecretKind, e: &mut Entry, msg: EntryMsg) -> Option<EntryOut> {
    match msg {
        EntryMsg::Type => Some(EntryOut::AskPassphrase),
        EntryMsg::Typed(v) => {
            e.value = v;
            None
        }
        EntryMsg::Submit if kind == SecretKind::Passphrase => submit(e),
        EntryMsg::Submit => None,
        EntryMsg::Key(k) => {
            if e.busy {
                return None;
            }
            let (next, go) = pin_press(&e.value, k, None, 6, 12);
            e.value = next;
            if go {
                submit(e)
            } else {
                None
            }
        }
    }
}

fn submit(e: &mut Entry) -> Option<EntryOut> {
    if e.busy {
        return None;
    }
    e.busy = true;
    e.error = None;
    Some(EntryOut::Submit(e.value.clone()))
}

/// The answer to a submitted secret: None went through, Some(error) shows and clears the entry.
pub fn entry_done(e: &mut Entry, err: Option<String>) {
    e.busy = false;
    if let Some(m) = err {
        e.error = Some(m);
        e.value.clear();
    }
}

// ------------------------------------------------------------------ Reveal

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealWhat {
    Phrase,
    Key,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reveal {
    pub entry: Entry,
    /// the phrase or key, once the vault opened for it
    pub secret: Option<String>,
    pub copied: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RevealMsg {
    Entry(EntryMsg),
    Close,
    Copy,
}

pub fn reveal_view(account: &RareAccount, what: RevealWhat, kind: SecretKind, st: &Reveal) -> El<RevealMsg> {
    let phrase = what == RevealWhat::Phrase;
    let footer = st.secret.as_ref().map(|_| {
        let mut f = vec![btn("Done", Variant::Primary, "flex-1", Some(RevealMsg::Close)).def(true)];
        if !phrase {
            f.push(btn(if st.copied { "Copied" } else { "Copy key" }, Variant::Secondary, "", Some(RevealMsg::Copy)));
        }
        f
    });
    let body = match &st.secret {
        None => entry_view(kind, if kind == SecretKind::Passphrase { "Enter your passphrase to continue" } else { "Enter your PIN to continue" }, "OK", &st.entry).map(&RevealMsg::Entry),
        Some(secret) => {
            let it = if phrase { "them" } else { "it" };
            let warn = p("rounded border border-border-primary bg-layer-01 px-4 py-3 text-sm text-secondary").text(format!(
                "Anyone with {} can take everything in this account. Write {it} down and keep {it} offline. Never type {it} into a website or share {it} in chat.",
                if phrase { "these words" } else { "this key" }
            ));
            let shown = if phrase {
                ol("grid grid-cols-3 gap-2").kids(secret.split_whitespace().enumerate().map(|(i, w)| {
                    li("flex items-baseline gap-3 rounded border border-border-primary bg-layer-01 px-4 py-3")
                        .key(i.to_string())
                        .kid(span("w-5 text-right text-xs tabular-nums text-secondary").text((i + 1).to_string()))
                        .kid(span("font-mono text-base font-medium").text(w))
                }))
            } else {
                p("break-all rounded border border-border-primary bg-layer-01 px-4 py-4 font-mono text-base").text(secret)
            };
            div("flex flex-col gap-4").kid(warn).kid(shown)
        }
    };
    sheet(
        SheetSpec {
            title: if phrase { "Recovery phrase" } else { "Private key" }.into(),
            subtitle: Some(account.label.clone()),
            icon: None,
            footer,
            close: Some(RevealMsg::Close),
            width: if st.secret.is_some() { "56rem" } else { "36rem" },
        },
        vec![body],
    )
}

// ------------------------------------------------------------------ Change PIN

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeStep {
    Current,
    New,
    Confirm,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChangePin {
    pub step: ChangeStep,
    pub cur: String,
    pub next: String,
    pub error: Option<String>,
    pub entry: Entry,
}

impl Default for ChangePin {
    fn default() -> Self {
        ChangePin { step: ChangeStep::Current, cur: String::new(), next: String::new(), error: None, entry: Entry::default() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChangeMsg {
    Entry(EntryMsg),
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChangeOut {
    AskPassphrase,
    /// `keystore.changeSecret(cur, next, kind)`; answer with `change_result`
    Change { cur: String, next: String },
    Close,
}

fn noun(kind: SecretKind) -> &'static str {
    if kind == SecretKind::Passphrase {
        "passphrase"
    } else {
        "PIN"
    }
}

/// `validNew`.
pub fn valid_new(kind: SecretKind, v: &str) -> Option<&'static str> {
    match kind {
        SecretKind::Passphrase => (v.chars().count() < 8).then_some("Use at least 8 characters"),
        SecretKind::Pin => (!(v.len() >= 6 && v.bytes().all(|b| b.is_ascii_digit()))).then_some("Use at least 6 digits"),
    }
}

pub fn change_view(kind: SecretKind, st: &ChangePin) -> El<ChangeMsg> {
    let n = noun(kind);
    let mut body = Vec::new();
    if let Some(e) = st.error.as_ref().filter(|_| st.step != ChangeStep::Done) {
        body.push(p("mb-4 text-center text-sm text-negative").text(e));
    }
    let entry = |prompt: &str, ok: &str, key: &str| entry_view(kind, prompt, ok, &st.entry).map(&ChangeMsg::Entry).key(key);
    match st.step {
        ChangeStep::Current => body.push(entry(&format!("Enter your current {n}"), "Next", "c")),
        ChangeStep::New => body.push(entry(if kind == SecretKind::Passphrase { "Choose a new passphrase" } else { "Choose a new PIN (6 digits or more)" }, "Next", "n")),
        ChangeStep::Confirm => body.push(entry(&format!("Enter the new {n} again"), "Change", "f")),
        ChangeStep::Done => body.push(p("py-6 text-center text-base").text(format!("Your {n} is changed."))),
    }
    sheet(
        SheetSpec {
            title: format!("Change {n}"),
            subtitle: None,
            icon: None,
            footer: (st.step == ChangeStep::Done).then(|| vec![btn("Done", Variant::Primary, "flex-1", Some(ChangeMsg::Close)).def(true)]),
            close: Some(ChangeMsg::Close),
            width: "36rem",
        },
        body,
    )
}

pub fn change_update(kind: SecretKind, st: &mut ChangePin, msg: ChangeMsg) -> Option<ChangeOut> {
    let m = match msg {
        ChangeMsg::Close => return Some(ChangeOut::Close),
        ChangeMsg::Entry(m) => m,
    };
    let secret = match entry_update(kind, &mut st.entry, m)? {
        EntryOut::AskPassphrase => return Some(ChangeOut::AskPassphrase),
        EntryOut::Submit(s) => s,
    };
    // each step's entry is a fresh one (React keys c, n, f)
    match st.step {
        ChangeStep::Current => {
            st.cur = secret;
            st.error = None;
            st.step = ChangeStep::New;
            st.entry = Entry::default();
            None
        }
        ChangeStep::New => {
            if let Some(bad) = valid_new(kind, &secret) {
                entry_done(&mut st.entry, Some(bad.into()));
                return None;
            }
            st.next = secret;
            st.error = None;
            st.step = ChangeStep::Confirm;
            st.entry = Entry::default();
            None
        }
        ChangeStep::Confirm => {
            st.entry.busy = false;
            if secret != st.next {
                st.step = ChangeStep::New;
                st.entry = Entry::default();
                st.error = Some(format!("Those did not match. Choose the new {} again.", noun(kind)));
                return None;
            }
            Some(ChangeOut::Change { cur: st.cur.clone(), next: st.next.clone() })
        }
        ChangeStep::Done => None,
    }
}

/// The vault's answer to `ChangeOut::Change`: Err carries `secretError`'s message.
pub fn change_result(st: &mut ChangePin, res: Result<(), String>) {
    match res {
        Ok(()) => {
            st.cur.clear();
            st.next.clear();
            st.step = ChangeStep::Done;
            st.error = None;
        }
        Err(m) => {
            st.step = ChangeStep::Current;
            st.entry = Entry::default();
            st.error = Some(m);
        }
    }
}

// ------------------------------------------------------------------ Remove one / all

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Remove {
    pub busy: bool,
    pub error: Option<String>,
    /// Opened from Settings > Accounts' Sign out: worded as signing out.
    pub sign_out: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RemoveMsg {
    Cancel,
    Remove,
}

pub fn remove_view(account: &RareAccount, st: &Remove) -> El<RemoveMsg> {
    let watch = account.kind == "watch";
    let device = account.kind == "ledger" || account.kind == "keystone";
    let copy = if watch {
        "Rare Wallet stops watching this address. Nothing moves and nothing is lost."
    } else if account.kind == "passkey" {
        "The account leaves this device. Your passkey signs you back in any time, here or on another device."
    } else if device {
        "The account is removed from this device. Your hardware wallet still holds the key, so you can add it again later."
    } else {
        "The account and its key are removed from this device. Make sure you saved the recovery phrase or key first. Without it the funds in this account cannot be recovered."
    };
    let mut body = vec![p("text-base leading-relaxed text-secondary").text(copy)];
    if let Some(e) = &st.error {
        body.push(p("mt-3 text-sm text-negative").text(e));
    }
    sheet(
        SheetSpec {
            title: if st.sign_out { "Sign out of Rare?" } else { "Remove this account?" }.into(),
            subtitle: Some(format!("{}, {}", account.label, kind_label(account).to_lowercase())),
            icon: None,
            footer: Some(vec![
                btn("Cancel", Variant::Secondary, "flex-1", Some(RemoveMsg::Cancel)).def(true),
                btn(
                    match (st.sign_out, st.busy) {
                        (true, true) => "Signing out",
                        (true, false) => "Sign out",
                        (false, true) => "Removing",
                        (false, false) => "Remove",
                    },
                    Variant::Secondary,
                    "flex-1 !text-negative",
                    Some(RemoveMsg::Remove),
                ),
            ]),
            close: Some(RemoveMsg::Cancel),
            width: "38rem",
        },
        body,
    )
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoveAll {
    pub typed: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl RemoveAll {
    pub fn ok(&self) -> bool {
        self.typed.trim().to_uppercase() == "REMOVE"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RemoveAllMsg {
    Cancel,
    /// A on the field: the keyboard ("Type REMOVE to confirm")
    Type,
    Typed(String),
    /// Remove all: wipes only when REMOVE is typed
    Wipe,
}

pub fn remove_all_view(st: &RemoveAll) -> El<RemoveAllMsg> {
    let ok = st.ok();
    sheet(
        SheetSpec {
            title: "Remove all wallets?".into(),
            subtitle: None,
            icon: None,
            footer: Some(vec![
                btn("Cancel", Variant::Secondary, "flex-1", Some(RemoveAllMsg::Cancel)).def(true),
                btn(if st.busy { "Removing" } else { "Remove all" }, Variant::Secondary, if ok { "flex-1 !text-negative" } else { "flex-1 opacity-40" }, Some(RemoveAllMsg::Wipe)),
            ]),
            close: Some(RemoveAllMsg::Cancel),
            width: "40rem",
        },
        vec![
            p("text-base leading-relaxed text-secondary").text("Every account, recovery phrase and key is erased from this device, and the PIN is cleared. Funds stay on the network, but without your recovery phrases or hardware wallets you cannot reach them again."),
            field(FieldSpec { label: Some("Type REMOVE to confirm"), value: &st.typed, placeholder: Some("REMOVE"), error: st.error.as_deref(), class: "mt-5", ..Default::default() }, RemoveAllMsg::Type),
        ],
    )
}

// ------------------------------------------------------------------ Hardware help

/// `checkLedger`'s answer.
#[derive(Clone, Debug, PartialEq)]
pub enum LedgerCheck {
    Busy,
    Ok { found: String },
    Error { message: String, fix: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum HelpMsg {
    Check,
    Close,
    CopyFix(String),
}

pub fn hardware_help_view(res: Option<&LedgerCheck>, copied: bool) -> El<HelpMsg> {
    let mut d = div("flex flex-col gap-4 text-base leading-relaxed text-secondary")
        .kid(p("").text("Ledger and Keystone connect over USB. Plug the device in, unlock it, and for Ledger open the Sui app. Ledger also works over Bluetooth."))
        .kid(p("").text("If Rare OS says it needs permission to use the device, run one command in a terminal, then unplug and plug the device back in. The check below shows the exact command for this system."));
    match res {
        Some(LedgerCheck::Ok { found }) => d = d.kid(div("rounded border border-border-primary bg-layer-01 p-4 text-sm").kid(p("text-primary").text(format!("Found {found}. Nothing is in the way.")))),
        Some(LedgerCheck::Error { message, fix }) => {
            let mut b = div("rounded border border-border-primary bg-layer-01 p-4 text-sm").kid(p("text-primary").text(message));
            if let Some(f) = fix {
                b = b.kid(p("mt-3 break-all font-mono text-xs text-secondary").text(f)).kid(btn_full(if copied { "Copied" } else { "Copy command" }, Variant::Secondary, Size::Sm, false, "mt-3", Some(HelpMsg::CopyFix(f.clone()))));
            }
            d = d.kid(b);
        }
        Some(LedgerCheck::Busy) | None => {}
    }
    sheet(
        SheetSpec {
            title: "Hardware wallets".into(),
            subtitle: None,
            icon: None,
            footer: Some(vec![
                btn(if res == Some(&LedgerCheck::Busy) { "Looking" } else { "Check my Ledger" }, Variant::Primary, "flex-1", Some(HelpMsg::Check)).def(true),
                btn("Close", Variant::Secondary, "", Some(HelpMsg::Close)),
            ]),
            close: Some(HelpMsg::Close),
            width: "46rem",
        },
        vec![d],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::kit::sheet_b;

    const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident";

    fn acct() -> RareAccount {
        let mut a = RareAccount::new("0x1", "Main", "native");
        a.hd = Some(Default::default());
        a
    }

    #[test]
    fn reveal_needs_the_pin_first() {
        let mut st = Reveal::default();
        let v = reveal_view(&acct(), RevealWhat::Phrase, SecretKind::Pin, &st);
        assert_eq!(v.texts()[..3], ["Recovery phrase", "Main", "Enter your PIN to continue"]);
        assert!(!v.content().contains("abandon"));
        assert_eq!(v.get_attr("data-on-b"), None);
        for k in ["1", "2", "3", "4", "5", "6"] {
            entry_update(SecretKind::Pin, &mut st.entry, EntryMsg::Key(k));
        }
        assert_eq!(entry_update(SecretKind::Pin, &mut st.entry, EntryMsg::Key("ok")), Some(EntryOut::Submit("123456".into())));
        entry_done(&mut st.entry, Some("Wrong PIN. Try again.".into()));
        let v = reveal_view(&acct(), RevealWhat::Phrase, SecretKind::Pin, &st);
        assert!(v.contains_text("Wrong PIN. Try again."));
        st.secret = Some(PHRASE.into());
        let v = reveal_view(&acct(), RevealWhat::Phrase, SecretKind::Pin, &st);
        let t = v.texts();
        assert!(t[2].starts_with("Anyone with these words can take everything in this account. Write them down"));
        assert_eq!(&t[3..7], ["1", "abandon", "2", "ability"]);
        assert_eq!(v.landing().as_deref(), Some("Done"));
        assert!(v.find_nav("Copy key").is_none());
        let k = reveal_view(&acct(), RevealWhat::Key, SecretKind::Pin, &Reveal { secret: Some("suiprivkey1qq".into()), ..Default::default() });
        assert!(k.contains_text("Anyone with this key can take everything in this account. Write it down and keep it offline. Never type it into a website or share it in chat."));
        assert!(k.find_nav("Copy key").is_some());
        assert_eq!(sheet_b(&k), Some(RevealMsg::Close));
    }

    #[test]
    fn change_pin_steps() {
        let mut st = ChangePin::default();
        let k = SecretKind::Pin;
        let typ = |st: &mut ChangePin, pin: &str| {
            let mut out = None;
            for c in pin.chars() {
                let key: &'static str = Box::leak(c.to_string().into_boxed_str());
                out = change_update(k, st, ChangeMsg::Entry(EntryMsg::Key(key)));
            }
            out.or_else(|| change_update(k, st, ChangeMsg::Entry(EntryMsg::Key("ok"))))
        };
        assert!(change_view(k, &st).contains_text("Enter your current PIN"));
        typ(&mut st, "111111");
        assert_eq!(st.step, ChangeStep::New);
        assert!(change_view(k, &st).contains_text("Choose a new PIN (6 digits or more)"));
        typ(&mut st, "222222");
        assert_eq!(st.step, ChangeStep::Confirm);
        typ(&mut st, "333333");
        assert_eq!(st.step, ChangeStep::New);
        assert!(change_view(k, &st).contains_text("Those did not match. Choose the new PIN again."));
        typ(&mut st, "222222");
        assert_eq!(typ(&mut st, "222222"), Some(ChangeOut::Change { cur: "111111".into(), next: "222222".into() }));
        change_result(&mut st, Ok(()));
        let v = change_view(k, &st);
        assert!(v.contains_text("Your PIN is changed."));
        assert_eq!(v.landing().as_deref(), Some("Done"));
        assert_eq!(valid_new(SecretKind::Passphrase, "short"), Some("Use at least 8 characters"));
    }

    #[test]
    fn removes() {
        let v = remove_view(&RareAccount::new("0x1", "Demo wallet", "watch"), &Remove::default());
        assert_eq!(v.texts()[..3], ["Remove this account?", "Demo wallet, watch only", "Rare Wallet stops watching this address. Nothing moves and nothing is lost."]);
        assert_eq!(v.landing().as_deref(), Some("Cancel"));
        let v = remove_view(&acct(), &Remove { busy: true, error: None, sign_out: false });
        assert!(v.contains_text("Make sure you saved the recovery phrase or key first."));
        assert!(v.find_nav("Removing").is_some());
        let mut st = RemoveAll::default();
        let v = remove_all_view(&st);
        assert!(v.find_nav("Remove all").is_some());
        assert!(!st.ok());
        st.typed = " remove ".into();
        assert!(st.ok());
        assert!(remove_all_view(&st).focusables().iter().any(|(t, _)| t == "Remove all"));
    }

    #[test]
    fn hardware_help() {
        let v = hardware_help_view(None, false);
        assert_eq!(v.landing().as_deref(), Some("Check my Ledger"));
        let v = hardware_help_view(Some(&LedgerCheck::Busy), false);
        assert!(v.find_nav("Looking").is_some());
        let v = hardware_help_view(Some(&LedgerCheck::Error { message: "Rare OS needs permission to use the Ledger.".into(), fix: Some("pkexec rare-udev".into()) }), false);
        assert!(v.contains_text("pkexec rare-udev"));
        assert!(v.find_nav("Copy command").is_some());
        let v = hardware_help_view(Some(&LedgerCheck::Ok { found: "Nano X".into() }), false);
        assert!(v.contains_text("Found Nano X. Nothing is in the way."));
    }
}
