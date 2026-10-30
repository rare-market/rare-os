//! Setting the PIN (or a passphrase) that protects keys on this device: enter it, then again
//! (launcher/src/wallet/ui/onboard/PinSetup.tsx). Every flow that creates the vault uses it.

use crate::screens::wallet::kit::{btn, btn_full, field, pin_pad, pin_press, FieldSpec, PinPadSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::SecretKind;

pub const PIN_LENGTH: usize = 6;

/// `PinOptions`: the secret that will protect the vault.
#[derive(Clone, Debug, PartialEq)]
pub struct PinOptions {
    pub kind: SecretKind,
    pub secret: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub mode: SecretKind,
    /// the first PIN, while asking for it again
    pub first: Option<String>,
    pub value: String,
    pub pass: String,
    pub pass2: String,
    pub error: String,
}

impl Default for State {
    fn default() -> Self {
        State { mode: SecretKind::Pin, first: None, value: String::new(), pass: String::new(), pass2: String::new(), error: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Key(&'static str),
    /// A on a passphrase field: keyboard ("Passphrase" / "Confirm passphrase", secret)
    TypePass,
    TypePass2,
    Pass(String),
    Pass2(String),
    Continue,
    Switch,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Done(PinOptions),
    AskPass { confirm: bool },
}

pub fn view(st: &State, title: &str, body: &str, busy: bool) -> El<Msg> {
    view_opts(st, title, body, busy, true)
}

/// The PIN only, no "Use a passphrase instead" (PIN accounts: the guard checks digits).
pub fn view_pin_only(st: &State, title: &str, body: &str, busy: bool) -> El<Msg> {
    view_opts(st, title, body, busy, false)
}

fn view_opts(st: &State, title: &str, body: &str, busy: bool, allow_pass: bool) -> El<Msg> {
    let pin = st.mode == SecretKind::Pin;
    let heading = if pin {
        if st.first.is_none() {
            title.to_string()
        } else {
            "Enter it again".into()
        }
    } else {
        "Choose a passphrase".into()
    };
    let sub = if pin {
        if st.first.is_none() {
            body.to_string()
        } else {
            "Type the same 6 digits to confirm.".into()
        }
    } else {
        "At least 8 characters. It unlocks your keys on this device.".into()
    };
    let entry = if pin {
        pin_pad(PinPadSpec { length: Some(PIN_LENGTH), value: &st.value, disabled: busy, ..Default::default() }, Msg::Key)
            .key(if st.first.is_none() { "a" } else { "b" })
            .on_pad("x", Msg::Key("back"))
            .on_pad("start", Msg::Key("ok"))
    } else {
        let off = st.pass.chars().count() < 8 || st.pass != st.pass2 || busy;
        div("mx-auto flex max-w-[28rem] flex-col gap-4 text-left")
            .kid(field(FieldSpec { label: Some("Passphrase"), value: &st.pass, secret: true, def: true, placeholder: Some("At least 8 characters"), ..Default::default() }, Msg::TypePass))
            .kid(field(
                FieldSpec { label: Some("Confirm passphrase"), value: &st.pass2, secret: true, placeholder: Some("Type it again"), error: (!st.pass2.is_empty() && st.pass != st.pass2).then_some("These do not match"), ..Default::default() },
                Msg::TypePass2,
            ))
            .kid(btn("Continue", Variant::Primary, &cn(&["w-full", when(off, "pointer-events-none opacity-40")]), Some(Msg::Continue)))
    };
    let mut d = div("flex flex-col items-center text-center")
        .kid(h2("text-xl font-bold").text(heading))
        .kid(p("mt-1 max-w-[28rem] text-sm text-secondary").text(sub))
        .kid(div("mt-6 w-full").kid(entry));
    if !st.error.is_empty() {
        d = d.kid(p("mt-4 text-sm text-negative").text(&st.error));
    }
    if !allow_pass {
        return d;
    }
    d.kid(div("mt-6").kid(btn_full(if pin { "Use a passphrase instead" } else { "Use a 6 digit PIN instead" }, Variant::Ghost, Size::Md, false, "", Some(Msg::Switch))))
}

pub fn update(st: &mut State, msg: Msg, busy: bool) -> Option<Out> {
    match msg {
        Msg::Key(k) => {
            if busy {
                return None;
            }
            let (next, done) = pin_press(&st.value, k, Some(PIN_LENGTH), 6, 12);
            st.value = next.clone();
            if !done {
                return None;
            }
            match st.first.take() {
                None => {
                    st.first = Some(next);
                    st.value.clear();
                    st.error.clear();
                    None
                }
                Some(f) if f == next => Some(Out::Done(PinOptions { kind: SecretKind::Pin, secret: next })),
                Some(_) => {
                    st.value.clear();
                    st.error = "Those PINs did not match. Start again.".into();
                    None
                }
            }
        }
        Msg::TypePass => Some(Out::AskPass { confirm: false }),
        Msg::TypePass2 => Some(Out::AskPass { confirm: true }),
        Msg::Pass(v) => {
            st.pass = v;
            None
        }
        Msg::Pass2(v) => {
            st.pass2 = v;
            None
        }
        Msg::Continue => {
            if st.pass.chars().count() < 8 || st.pass != st.pass2 || busy {
                return None;
            }
            Some(Out::Done(PinOptions { kind: SecretKind::Passphrase, secret: st.pass.clone() }))
        }
        Msg::Switch => {
            *st = State { mode: if st.mode == SecretKind::Pin { SecretKind::Passphrase } else { SecretKind::Pin }, ..State::default() };
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typ(st: &mut State, pin: &str) -> Option<Out> {
        let mut out = None;
        for c in pin.chars() {
            let k: &'static str = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"][c.to_digit(10).unwrap() as usize];
            out = update(st, Msg::Key(k), false);
        }
        out
    }

    #[test]
    fn pin_twice() {
        let mut st = State::default();
        let v = view(&st, "Set a PIN", "It unlocks your keys on this device. It never leaves it.", false);
        assert_eq!(v.texts()[..2], ["Set a PIN", "It unlocks your keys on this device. It never leaves it."]);
        assert_eq!(typ(&mut st, "123456"), None);
        assert!(view(&st, "Set a PIN", "", false).contains_text("Type the same 6 digits to confirm."));
        assert_eq!(typ(&mut st, "654321"), None);
        assert!(view(&st, "Set a PIN", "", false).contains_text("Those PINs did not match. Start again."));
        typ(&mut st, "111111");
        assert_eq!(typ(&mut st, "111111"), Some(Out::Done(PinOptions { kind: SecretKind::Pin, secret: "111111".into() })));
    }

    #[test]
    fn passphrase() {
        let mut st = State::default();
        update(&mut st, Msg::Switch, false);
        let v = view(&st, "Set a PIN", "", false);
        assert!(v.contains_text("Choose a passphrase"));
        assert!(v.contains_text("Use a 6 digit PIN instead"));
        update(&mut st, Msg::Pass("correct horse".into()), false);
        update(&mut st, Msg::Pass2("correct hors".into()), false);
        assert!(view(&st, "", "", false).contains_text("These do not match"));
        assert_eq!(update(&mut st, Msg::Continue, false), None);
        update(&mut st, Msg::Pass2("correct horse".into()), false);
        assert!(!view(&st, "", "", false).content().contains("correct horse"));
        assert_eq!(update(&mut st, Msg::Continue, false), Some(Out::Done(PinOptions { kind: SecretKind::Passphrase, secret: "correct horse".into() })));
    }
}
