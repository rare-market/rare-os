//! This SuiPlay's PIN, from Settings > Accounts: Change PIN (the current PIN, or a backup's
//! signature when it is forgotten or locked) and Show recovery words (a warning first, then the
//! PIN, then the 12 words). Shown by the review host when no request is on screen.

use super::locked;
use super::unlock;
use crate::screens::wallet::kit::{btn, sheet, SheetSpec, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::onboard::pin_setup;
use crate::screens::wallet::onboard::word_grid::{self, Grid};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Change,
    Words,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// Words: what the words are and who must never see them
    Warn,
    /// the current PIN
    Current(unlock::State),
    /// Change: the new PIN, twice (`old` is the current one, or None after a backup's unlock)
    New { pin: pin_setup::State, saving: bool, error: Option<String> },
    /// Change: the backup signs instead of the forgotten PIN (the locked sheet in "forgot" words)
    Backup(locked::State),
    /// Words: the 12 words
    Shown(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub task: Task,
    pub phase: Phase,
    /// the current PIN, kept until the new one is saved
    pub old: Option<String>,
}

impl State {
    pub fn new(task: Task) -> State {
        State { task, phase: if task == Task::Words { Phase::Warn } else { Phase::Current(unlock::State::default()) }, old: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Show,
    Unlock(unlock::Msg),
    New(pin_setup::Msg),
    Backup(locked::Msg),
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// check the PIN: Change keeps it for later, Words reads the words with it
    Check(String),
    /// save the new PIN: with the current PIN, or with the backup's token
    Save { old: Option<String>, new: String },
    /// run the backup's unlock ceremony
    Backup(crate::screens::wallet::onboard::ceremony::Via),
    Close,
}

pub const WARN: &str = "These 12 words are this SuiPlay's key to your account. With them and your phone or security key, anyone can take everything in it. Rare never asks for them. Write them on paper and keep it safe. Never type them into a website, a message or a photo.";

pub fn view(st: &State, phone: bool, key: bool, passkey: &super::panels::Passkey, now: u64) -> El<Msg> {
    match &st.phase {
        Phase::Warn => sheet(
            SheetSpec {
                title: "Recovery words".into(),
                subtitle: None,
                icon: None,
                footer: Some(vec![btn("Show the words", Variant::Primary, "flex-1", Some(Msg::Show)).def(true), btn("Cancel", Variant::Secondary, "flex-1", Some(Msg::Done))]),
                close: Some(Msg::Done),
                width: "40rem",
            },
            vec![div("flex flex-col items-center gap-4 text-center")
                .kid(icon("triangle-alert", "text-icon-secondary").attr("size", "2.25rem").attr("stroke-width", "1.5"))
                .kid(p("max-w-[32rem] text-base").role("status").text(WARN))
                .kid(p("max-w-[32rem] text-sm text-secondary").text("Check that nobody can see the screen. You need them only to sign with a PIN on another SuiPlay."))],
        ),
        Phase::Current(u) => {
            let (reason, alt) = match st.task {
                Task::Change => ("Enter your current PIN", Some("Forgot it? Use your backup")),
                Task::Words => ("Enter your PIN to see your recovery words", None),
            };
            let pend = unlock::Pending { reason: reason.into(), phone: alt.is_some(), title: Some(if st.task == Task::Change { "Change PIN" } else { "Recovery words" }.into()), phone_label: alt.map(str::to_string) };
            unlock::view(&pend, unlock::VaultKind::Pin, u, now).map(&Msg::Unlock)
        }
        Phase::New { pin, saving, error } => {
            let mut d = div("pt-2").kid(pin_setup::view_pin_only(pin, "Pick a new PIN", "6 to 12 digits. It signs on this SuiPlay; 5 wrong ones lock it until your backup unlocks it.", *saving).map(&Msg::New));
            if let Some(e) = error {
                d = d.kid(p("mt-3 text-center text-sm text-negative").text(e));
            }
            sheet(SheetSpec { title: "Change PIN".into(), subtitle: None, icon: None, footer: Some(vec![btn("Cancel", Variant::Secondary, "flex-1", Some(Msg::Done))]), close: Some(Msg::Done), width: "40rem" }, vec![d])
        }
        Phase::Backup(l) => locked::view(&locked::Props { phone, key, passkey, forgot: true }, l).map(&Msg::Backup),
        Phase::Shown(words) => sheet(
            SheetSpec { title: "Recovery words".into(), subtitle: Some("Write them down in this order".into()), icon: None, footer: Some(vec![btn("Done", Variant::Primary, "flex-1", Some(Msg::Done)).def(true)]), close: Some(Msg::Done), width: "52rem" },
            vec![div("flex flex-col gap-4")
                .kid(word_grid::view(&Grid { words, cols: Some(3), pick: false, active: None, bad: &BTreeSet::new(), def_index: None }, |_| Msg::Done))
                .kid(p("text-sm text-secondary").text("Never share them. Anyone with these words and your backup can take your account."))],
        ),
    }
}

pub fn update(st: &mut State, msg: Msg, now: u64) -> Option<Effect> {
    match msg {
        Msg::Done => Some(Effect::Close),
        Msg::Show => {
            st.phase = Phase::Current(unlock::State::default());
            None
        }
        Msg::Unlock(m) => {
            let Phase::Current(u) = &mut st.phase else { return None };
            match unlock::update(u, m, now)? {
                unlock::Effect::Unlock(pin) => Some(Effect::Check(pin)),
                unlock::Effect::Finish(false) => Some(Effect::Close),
                unlock::Effect::Phone => {
                    st.phase = Phase::Backup(locked::State::default());
                    None
                }
                _ => None,
            }
        }
        Msg::New(m) => {
            let Phase::New { pin, saving, error } = &mut st.phase else { return None };
            match pin_setup::update(pin, m, *saving)? {
                pin_setup::Out::Done(p) => {
                    *saving = true;
                    *error = None;
                    Some(Effect::Save { old: st.old.clone(), new: p.secret })
                }
                pin_setup::Out::AskPass { .. } => None,
            }
        }
        Msg::Backup(m) => {
            let Phase::Backup(l) = &mut st.phase else { return None };
            match locked::update(l, m)? {
                locked::Effect::Unlock(v) => Some(Effect::Backup(v)),
                locked::Effect::Reset(new) => Some(Effect::Save { old: None, new }),
                locked::Effect::Cancel | locked::Effect::Phone => Some(Effect::Close),
            }
        }
    }
}

/// The PIN check's answer. Change: Ok goes to the new PIN; Words: Ok carries the words.
/// `locked`: the PIN guard locked (5 wrong): Change goes to the backup, Words says so.
pub fn checked(st: &mut State, pin: String, res: Result<Option<Vec<String>>, (bool, String)>) {
    match res {
        Ok(Some(words)) => st.phase = Phase::Shown(words),
        Ok(None) => {
            st.old = Some(pin);
            st.phase = Phase::New { pin: pin_setup::State::default(), saving: false, error: None };
        }
        Err((true, _)) if st.task == Task::Change => st.phase = Phase::Backup(locked::State::default()),
        Err((_, m)) => {
            if let Phase::Current(u) = &mut st.phase {
                u.busy = false;
                u.value.clear();
                u.error = Some(m);
            }
        }
    }
}

/// The backup's unlock answer (a token for the new PIN).
pub fn backed(st: &mut State, res: Result<String, String>) {
    if let Phase::Backup(l) = &mut st.phase {
        locked::unlocked(l, res);
    }
}

/// The save's answer: true closes with "PIN changed".
pub fn saved(st: &mut State, res: Result<(), String>) -> bool {
    match &mut st.phase {
        Phase::New { pin, saving, error } => match res {
            Ok(()) => true,
            Err(e) => {
                *saving = false;
                *pin = pin_setup::State::default();
                *error = Some(e);
                false
            }
        },
        Phase::Backup(l) => locked::saved(l, res),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::sign::panels::Passkey;

    fn press(st: &mut State, keys: &str, f: impl Fn(&'static str) -> Msg) -> Option<Effect> {
        let mut out = None;
        for c in keys.chars() {
            let k: &'static str = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"][c.to_digit(10).unwrap() as usize];
            out = update(st, f(k), 0);
        }
        out
    }

    #[test]
    fn change_with_the_pin_then_with_the_backup() {
        let mut st = State::new(Task::Change);
        assert!(view(&st, true, false, &Passkey::Idle, 0).contains_text("Forgot it? Use your backup"));
        press(&mut st, "123456", |k| Msg::Unlock(unlock::Msg::Key(k)));
        assert_eq!(update(&mut st, Msg::Unlock(unlock::Msg::Key("ok")), 0), Some(Effect::Check("123456".into())));
        checked(&mut st, "123456".into(), Err((false, "Wrong PIN".into())));
        assert!(view(&st, true, false, &Passkey::Idle, 0).contains_text("Wrong PIN"));
        checked(&mut st, "123456".into(), Ok(None));
        press(&mut st, "654321", |k| Msg::New(pin_setup::Msg::Key(k)));
        assert_eq!(press(&mut st, "654321", |k| Msg::New(pin_setup::Msg::Key(k))), Some(Effect::Save { old: Some("123456".into()), new: "654321".into() }));
        assert!(saved(&mut st, Ok(())));
        // forgot it: the backup, then a new PIN without the old one
        let mut st = State::new(Task::Change);
        update(&mut st, Msg::Unlock(unlock::Msg::Phone), 0);
        let v = view(&st, true, true, &Passkey::Idle, 0);
        assert!(v.contains_text("Use your phone") && v.contains_text("Use your security key") && !v.contains_text("Sign this with your phone instead"));
        assert_eq!(update(&mut st, Msg::Backup(locked::Msg::Unlock(crate::screens::wallet::onboard::ceremony::Via::Usb)), 0), Some(Effect::Backup(crate::screens::wallet::onboard::ceremony::Via::Usb)));
        backed(&mut st, Ok("t".into()));
        press(&mut st, "111222", |k| Msg::Backup(locked::Msg::Pin(pin_setup::Msg::Key(k))));
        assert_eq!(press(&mut st, "111222", |k| Msg::Backup(locked::Msg::Pin(pin_setup::Msg::Key(k)))), Some(Effect::Save { old: None, new: "111222".into() }));
        // locked while entering the current PIN: straight to the backup
        let mut st = State::new(Task::Change);
        checked(&mut st, "0".into(), Err((true, locked::LOCKED.into())));
        assert!(matches!(st.phase, Phase::Backup(_)));
    }

    #[test]
    fn words_warn_first_then_pin_then_words() {
        let mut st = State::new(Task::Words);
        let v = view(&st, true, false, &Passkey::Idle, 0);
        assert!(v.contains_text(WARN));
        update(&mut st, Msg::Show, 0);
        assert!(view(&st, true, false, &Passkey::Idle, 0).contains_text("Enter your PIN to see your recovery words"));
        let w: Vec<String> = (1..=12).map(|i| format!("word{i}")).collect();
        checked(&mut st, "123456".into(), Ok(Some(w)));
        let v = view(&st, true, false, &Passkey::Idle, 0);
        assert!(v.contains_text("word12"));
        assert_eq!(update(&mut st, Msg::Done, 0), Some(Effect::Close));
    }
}
