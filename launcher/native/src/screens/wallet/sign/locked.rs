//! A PIN account after 5 wrong PINs: "Locked. Use your phone to unlock". The phone (or the
//! security key) signs a fresh challenge from the PIN guard, then a new PIN; or the backup signs
//! this request directly. Shown by the review host over the request.

use super::panels::Passkey;
use crate::screens::wallet::kit::{btn, sheet, SheetSpec, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::onboard::ceremony::Via;
use crate::screens::wallet::onboard::pin_setup;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// the two ways out
    Locked,
    /// the backup is signing the unlock challenge
    Unlocking(Via),
    /// unlocked: the new PIN (twice)
    NewPin,
    /// saving the new PIN
    Saving,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub phase: Phase,
    pub pin: pin_setup::State,
    pub token: Option<String>,
    pub error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State { phase: Phase::Locked, pin: pin_setup::State::default(), token: None, error: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Unlock(Via),
    /// sign this request with the backup instead
    Phone,
    Pin(pin_setup::Msg),
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// run the unlock ceremony with this backup
    Unlock(Via),
    /// close this and sign with the backup
    Phone,
    /// set the new PIN with the token
    Reset(String),
    /// stop the ceremony (if any) and close everything
    Cancel,
}

pub struct Props<'a> {
    /// which backups the account has
    pub phone: bool,
    pub key: bool,
    /// the ceremony's last word
    pub passkey: &'a Passkey,
    /// not locked: the PIN is forgotten (Change PIN), so no "sign this instead"
    pub forgot: bool,
}

pub const LOCKED: &str = "Locked. Use your phone to unlock";
const BODY: &str = "5 wrong PINs locked the key on this SuiPlay. Your phone (or security key) unlocks it, then you pick a new PIN.";

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let (title, body): (&str, Vec<El<Msg>>) = match &st.phase {
        Phase::Locked => {
            let (head, body) = if pr.forgot { ("Use your backup to change the PIN", "Your phone or security key signs once, then you pick a new PIN.") } else { (LOCKED, BODY) };
            let mut d = div("flex flex-col items-center gap-4 text-center")
                .kid(icon("lock", "text-icon-secondary").attr("size", "2.25rem").attr("stroke-width", "1.5"))
                .kid(p("text-lg font-semibold").role("status").text(head))
                .kid(p("max-w-[30rem] text-sm text-secondary").text(body));
            if let Some(e) = &st.error {
                d = d.kid(p("max-w-[30rem] text-sm text-negative").text(e));
            }
            (if pr.forgot { "Change PIN" } else { "Locked" }, vec![d])
        }
        Phase::Unlocking(via) => {
            let mut d = div("flex flex-col items-center gap-4 text-center");
            match (via, pr.passkey) {
                (Via::Phone, Passkey::Qr { url }) => {
                    d = d
                        .kid(el(Tag::Qr(url.clone()), "size-[16rem] rounded bg-white p-2"))
                        .kid(p("max-w-[30rem] text-base").text("Scan this code with your phone's camera, then approve with Face ID or your fingerprint."))
                        .kid(p("max-w-[30rem] text-sm text-secondary").text("Keep Bluetooth on for both."));
                }
                (Via::Phone, Passkey::Status { message }) => d = d.kid(div("py-10").kid(p("text-xl font-semibold").text("Approve on your phone")).kid(p("mt-2 text-sm text-secondary").text(message))),
                (Via::Usb, Passkey::Status { message }) => d = d.kid(div("py-10").kid(p("text-xl font-semibold").text("Touch your security key")).kid(p("mt-2 text-sm text-secondary").text(message))),
                (Via::Usb, _) => d = d.kid(div("py-10").kid(p("text-xl font-semibold").text("Touch your security key")).kid(p("mt-2 text-sm text-secondary").text("Plug it into the USB-C port on top. Touch it when it blinks."))),
                (_, Passkey::Error { message }) => d = d.kid(p("text-sm text-negative").text(message)),
                _ => d = d.kid(div("py-10").kid(p("text-xl font-semibold").text("Getting ready")).kid(div("pointer-events-none absolute left-0 right-0 top-0 z-30 h-[2px] load-bar"))),
            }
            ("Unlock this SuiPlay", vec![d])
        }
        Phase::NewPin | Phase::Saving => {
            let mut d = div("pt-2").kid(pin_setup::view_pin_only(&st.pin, "Set a new PIN", "Your backup unlocked this SuiPlay. Pick a new PIN for it.", st.phase == Phase::Saving).map(&Msg::Pin));
            if let Some(e) = &st.error {
                d = d.kid(p("mt-3 text-center text-sm text-negative").text(e));
            }
            (if pr.forgot { "Change PIN" } else { "Unlocked" }, vec![d])
        }
    };
    let mut body = body;
    if st.phase == Phase::Locked {
        let mut ways = div("mx-auto mt-6 flex w-full max-w-[26rem] flex-col gap-3");
        let (lp, lk) = if pr.forgot { ("Use your phone", "Use your security key") } else { ("Unlock with your phone", "Unlock with your security key") };
        if pr.phone {
            ways = ways.kid(btn(lp, Variant::Primary, "w-full", Some(Msg::Unlock(Via::Phone))).def(true));
        }
        if pr.key {
            ways = ways.kid(btn(lk, if pr.phone { Variant::Secondary } else { Variant::Primary }, "w-full", Some(Msg::Unlock(Via::Usb))).def(!pr.phone));
        }
        if !pr.forgot {
            ways = ways.kid(btn(if pr.phone { "Sign this with your phone instead" } else { "Sign this with your key instead" }, Variant::Secondary, "w-full", Some(Msg::Phone)));
        }
        body.push(ways);
    }
    let footer = vec![btn("Cancel", Variant::Secondary, "flex-1", Some(Msg::Cancel))];
    sheet(SheetSpec { title: title.into(), subtitle: None, icon: None, footer: Some(footer), close: Some(Msg::Cancel), width: "40rem" }, body)
}

pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Unlock(v) => {
            st.error = None;
            st.phase = Phase::Unlocking(v);
            Some(Effect::Unlock(v))
        }
        Msg::Phone => Some(Effect::Phone),
        Msg::Cancel => Some(Effect::Cancel),
        Msg::Pin(m) => {
            if st.phase != Phase::NewPin {
                return None;
            }
            match pin_setup::update(&mut st.pin, m, false)? {
                pin_setup::Out::Done(p) => {
                    st.phase = Phase::Saving;
                    st.error = None;
                    Some(Effect::Reset(p.secret))
                }
                pin_setup::Out::AskPass { .. } => None,
            }
        }
    }
}

/// The unlock ceremony's answer: a token for the new PIN, or why not (back to Locked).
pub fn unlocked(st: &mut State, res: Result<String, String>) {
    match res {
        Ok(token) => {
            st.token = Some(token);
            st.pin = pin_setup::State::default();
            st.phase = Phase::NewPin;
        }
        Err(e) => {
            st.phase = Phase::Locked;
            st.error = Some(e);
        }
    }
}

/// The new PIN's answer: Ok signs the waiting request; an error asks again.
pub fn saved(st: &mut State, res: Result<(), String>) -> bool {
    match res {
        Ok(()) => {
            st.token = None;
            true
        }
        Err(e) => {
            st.pin = pin_setup::State::default();
            st.phase = if st.token.is_some() { Phase::NewPin } else { Phase::Locked };
            st.error = Some(e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_unlock_new_pin() {
        let pr = Props { phone: true, key: true, passkey: &Passkey::Idle, forgot: false };
        let mut st = State::default();
        let v = view(&pr, &st);
        assert!(v.contains_text(LOCKED));
        assert!(v.contains_text("Unlock with your phone") && v.contains_text("Unlock with your security key") && v.contains_text("Sign this with your phone instead"));
        assert_eq!(update(&mut st, Msg::Unlock(Via::Usb)), Some(Effect::Unlock(Via::Usb)));
        assert!(view(&pr, &st).contains_text("Touch your security key"));
        unlocked(&mut st, Err("That passkey can't unlock this device".into()));
        assert_eq!(st.phase, Phase::Locked);
        update(&mut st, Msg::Unlock(Via::Phone));
        let qr = Passkey::Qr { url: "FIDO:/1".into() };
        assert!(view(&Props { passkey: &qr, ..pr }, &st).contains_text("Scan this code with your phone's camera, then approve with Face ID or your fingerprint."));
        unlocked(&mut st, Ok("t".into()));
        assert!(view(&pr, &st).contains_text("Set a new PIN"));
        for _ in 0..2 {
            for k in ["1", "3", "5", "7", "9", "2"] {
                let _ = update(&mut st, Msg::Pin(pin_setup::Msg::Key(k)));
            }
        }
        assert_eq!(st.phase, Phase::Saving);
        assert!(saved(&mut st, Ok(())));
        // a key-only account
        let pr = Props { phone: false, key: true, passkey: &Passkey::Idle, forgot: false };
        let v = view(&pr, &State::default());
        assert!(!v.contains_text("Unlock with your phone") && v.contains_text("Sign this with your key instead"));
    }
}
