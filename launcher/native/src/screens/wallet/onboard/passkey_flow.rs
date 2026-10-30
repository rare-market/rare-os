//! The passkey flow (launcher/src/wallet/ui/onboard/PasskeyFlow.tsx): choose phone or key, the
//! device PIN (create), Bluetooth when it is off, the live ceremony, and the "signed in" note
//! when this device has no key for the account. Shared by the Passkey page and first run.

use super::ceremony::{self, bluetooth_view, BtMsg, Ceremony, Via};
use super::flow::{plain_error, udev_fix, Failure, Gate};
use super::hw_parts::hw_error;
use super::pin_setup::{self, PinOptions};
use crate::screens::wallet::kit::{btn, Variant};
use crate::screens::wallet::markup::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Choose,
    Pin,
    Bluetooth,
    Run,
    Device,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub step: Step,
    pub error: Option<(String, Option<String>)>,
    /// signed in, but this device holds no key for the account
    pub landed: Option<String>,
    /// the run being waited for (a cancelled run's answer is ignored)
    pub run: u64,
    pub pending_pin: Option<PinOptions>,
    pub bt_note: Option<String>,
    pub bt_busy: bool,
    pub bt_error: Option<String>,
    pub pin: pin_setup::State,
    /// the security key's PIN being typed
    pub key_pin: String,
}

impl Default for State {
    fn default() -> Self {
        State { step: Step::Choose, error: None, landed: None, run: 0, pending_pin: None, bt_note: None, bt_busy: false, bt_error: None, pin: pin_setup::State::default(), key_pin: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Pick(Via),
    Continue,
    Pin(pin_setup::Msg),
    Bt(BtMsg),
    Ceremony(ceremony::Msg),
    /// "You are signed in": Continue
    Finish,
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// remember phone or key (usePasskeyPrefs)
    SetVia(Via),
    /// create: `vaultGate("Enter your PIN to add this account")`; answer with `gate`
    Gate,
    /// `bluetoothReady()`; answer with `bt_ready`
    CheckBluetooth,
    /// `bluetooth.powerOn()`; answer with `bt_result`
    PowerOn,
    /// create the account or sign in (run id); answer with `ran`
    Start { run: u64, create: bool, pin: Option<PinOptions> },
    /// `passkeyNative.cancel()`
    CancelCeremony,
    /// hand the security key its PIN (None cancels)
    KeyPin(Option<String>),
    AskPass { confirm: bool },
    /// an account landed: select it and finish
    Finish(String),
    /// B on the first step (the host's own exit, first run only)
    Exit,
}

fn choice(on: bool, ic: &'static str, title: &str, body: &str, msg: Msg, def: bool) -> El<Msg> {
    button(cn(&["flex w-full items-center gap-4 rounded border px-5 py-4 text-left transition-colors", if on { "border-border-light bg-layer-02" } else { "border-border-primary bg-layer-01" }]), Some(msg))
        .def(def)
        .role("radio")
        .attr("aria-checked", on.to_string())
        .kid(span("text-icon-primary").kid(icon(ic, "").attr("size", "1.5rem")))
        .kid(span("min-w-0 flex-1").kid(span("block text-base font-medium").text(title)).kid(span("mt-0.5 block text-sm text-secondary").text(body)))
        .kid(span(cn(&["flex size-5 shrink-0 items-center justify-center rounded-full border", if on { "border-primary" } else { "border-border-light" }])).kid_if(on, || span("size-2.5 rounded-full bg-primary")))
}

pub struct Props<'a> {
    pub create: bool,
    pub via: Via,
    pub ceremony: &'a Ceremony,
    pub qr_url: Option<&'a str>,
    /// the host gives B on the first step somewhere to go (first run)
    pub can_exit: bool,
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let mut root = match st.step {
        Step::Choose => {
            let mut d = div("flex flex-col gap-4").kid(
                div("flex flex-col gap-2")
                    .role("radiogroup")
                    .kid(choice(pr.via == Via::Phone, "smartphone", if pr.create { "Use your phone" } else { "Sign in with your phone" }, "Scan a code with your phone's camera, then use Face ID or your fingerprint", Msg::Pick(Via::Phone), true))
                    .kid(choice(pr.via == Via::Usb, "usb", "Use a security key", "A USB or NFC key such as a YubiKey. No camera needed.", Msg::Pick(Via::Usb), false)),
            );
            if pr.create {
                d = d.kid(p("text-sm text-secondary").text("Your passkey can always sign. This device signs everyday transactions with your PIN."));
            }
            if let Some((m, fix)) = &st.error {
                d = d.kid(hw_error(m, fix.as_deref(), false, false).map(&|_| Msg::Continue));
            }
            d.kid(btn("Continue", Variant::Primary, "w-full", Some(Msg::Continue)))
        }
        Step::Pin => div("pt-4").kid(pin_setup::view(&st.pin, "Set a device PIN", "This device signs everyday transactions with it. Your passkey can always sign too.", false).map(&Msg::Pin)),
        Step::Bluetooth => bluetooth_view(st.bt_note.as_deref(), st.bt_busy, st.bt_error.as_deref(), true).map(&Msg::Bt),
        Step::Run => div("pt-2").kid(ceremony::view(pr.via, pr.ceremony, pr.qr_url, &st.key_pin).map(&Msg::Ceremony)),
        Step::Device => div("flex flex-col items-center gap-4 py-10 text-center")
            .kid(h2("text-xl font-bold").text("You are signed in"))
            .kid(p("max-w-[28rem] text-secondary").text("This device will ask your phone to approve each transaction."))
            .kid(btn("Continue", Variant::Primary, "mt-2 w-64", Some(Msg::Finish)).def(true)),
    };
    let can_back = matches!(st.step, Step::Pin | Step::Bluetooth | Step::Run) || (st.step == Step::Choose && pr.can_exit);
    if can_back {
        root = root.on_pad("b", Msg::Back);
    }
    root
}

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Pick(v) => Some(Effect::SetVia(v)),
        Msg::Continue => {
            if pr.create {
                Some(Effect::Gate)
            } else {
                proceed(pr, st, None)
            }
        }
        Msg::Pin(m) => match pin_setup::update(&mut st.pin, m, false)? {
            pin_setup::Out::Done(pin) => proceed(pr, st, Some(pin)),
            pin_setup::Out::AskPass { confirm } => Some(Effect::AskPass { confirm }),
        },
        Msg::Bt(BtMsg::TurnOn) => {
            st.bt_busy = true;
            st.bt_error = None;
            Some(Effect::PowerOn)
        }
        Msg::Bt(BtMsg::Cancel) => {
            st.step = Step::Choose;
            None
        }
        Msg::Ceremony(ceremony::Msg::Cancel) => Some(cancel(st)),
        Msg::Ceremony(ceremony::Msg::PinKey(k)) => ceremony::pin_key(&mut st.key_pin, k).map(|p_| Effect::KeyPin(Some(p_))),
        Msg::Ceremony(ceremony::Msg::PinCancel) => {
            st.key_pin.clear();
            Some(Effect::KeyPin(None))
        }
        Msg::Finish => st.landed.clone().map(Effect::Finish),
        Msg::Back => match st.step {
            Step::Run => Some(cancel(st)),
            Step::Pin | Step::Bluetooth => {
                st.step = Step::Choose;
                None
            }
            _ if pr.can_exit => Some(Effect::Exit),
            _ => None,
        },
    }
}

fn cancel(st: &mut State) -> Effect {
    st.run += 1;
    st.step = Step::Choose;
    Effect::CancelCeremony
}

fn proceed(pr: &Props, st: &mut State, pin: Option<PinOptions>) -> Option<Effect> {
    st.pending_pin = pin;
    if pr.via == Via::Phone {
        return Some(Effect::CheckBluetooth);
    }
    Some(start(pr, st, st.pending_pin.clone()))
}

fn start(pr: &Props, st: &mut State, pin: Option<PinOptions>) -> Effect {
    st.run += 1;
    st.error = None;
    st.step = Step::Run;
    Effect::Start { run: st.run, create: pr.create, pin }
}

/// `vaultGate`'s answer: no vault yet asks for a PIN, an open vault goes on, a refused unlock stays.
pub fn gate(pr: &Props, st: &mut State, g: Gate) -> Option<Effect> {
    match g {
        Gate::Pin => {
            st.step = Step::Pin;
            st.pin = pin_setup::State::default();
            None
        }
        Gate::Ready => proceed(pr, st, None),
        Gate::Cancelled => None,
    }
}

/// `bluetoothReady()`'s answer.
pub fn bt_ready(pr: &Props, st: &mut State, ready: bool) -> Option<Effect> {
    if !ready {
        st.bt_note = None;
        st.step = Step::Bluetooth;
        return None;
    }
    Some(start(pr, st, st.pending_pin.clone()))
}

/// `bluetooth.powerOn()`'s answer: powered starts the ceremony.
pub fn bt_result(pr: &Props, st: &mut State, powered: bool, blocked: bool, native_message: Option<&str>) -> Option<Effect> {
    st.bt_busy = false;
    match ceremony::bluetooth_error(powered, blocked, native_message) {
        None => Some(start(pr, st, st.pending_pin.clone())),
        Some(e) => {
            st.bt_error = Some(e);
            None
        }
    }
}

/// The run's answer: the account (and whether this device holds a key for it when signing in), or the error.
pub fn ran(pr: &Props, st: &mut State, run: u64, res: Result<(String, bool), Failure>) -> Option<Effect> {
    if run != st.run {
        return None;
    }
    match res {
        Ok((address, has_device_key)) => {
            if !pr.create && !has_device_key {
                st.landed = Some(address);
                st.step = Step::Device;
                return None;
            }
            Some(Effect::Finish(address))
        }
        Err(e) => {
            let code = e.code.as_deref();
            if code == Some("cancelled") {
                st.step = Step::Choose;
                return None;
            }
            if pr.via == Via::Phone && code == Some("ble-off") {
                st.bt_note = Some("Turn Bluetooth back on to finish with your phone.".into());
                st.step = Step::Bluetooth;
                return None;
            }
            let message = if code == Some("notfound") {
                if pr.via == Via::Usb { "No security key found. Insert it and try again." } else { "No Rare passkey found on that phone." }.to_string()
            } else {
                plain_error(&e, "Could not use the passkey. Try again.")
            };
            st.error = Some((message, udev_fix(&e)));
            st.step = Step::Choose;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(create: bool, via: Via) -> Props<'static> {
        Props { create, via, ceremony: &Ceremony::Idle, qr_url: None, can_exit: false }
    }

    #[test]
    fn sign_in_with_phone() {
        let pr = props(false, Via::Phone);
        let mut st = State::default();
        let v = view(&pr, &st);
        assert_eq!(v.texts(), ["Sign in with your phone", "Scan a code with your phone's camera, then use Face ID or your fingerprint", "Use a security key", "A USB or NFC key such as a YubiKey. No camera needed.", "Continue"]);
        assert_eq!(v.pad("b"), None);
        assert_eq!(update(&pr, &mut st, Msg::Continue), Some(Effect::CheckBluetooth));
        assert_eq!(bt_ready(&pr, &mut st, false), None);
        assert_eq!(st.step, Step::Bluetooth);
        assert_eq!(update(&pr, &mut st, Msg::Bt(BtMsg::TurnOn)), Some(Effect::PowerOn));
        assert_eq!(bt_result(&pr, &mut st, true, false, None), Some(Effect::Start { run: 1, create: false, pin: None }));
        assert_eq!(st.step, Step::Run);
        assert_eq!(view(&pr, &st).pad("b"), Some(&Msg::Back));
        // landed with no device key: the note
        assert_eq!(ran(&pr, &mut st, 1, Ok(("0xabc".into(), false))), None);
        let v = view(&pr, &st);
        assert!(v.contains_text("You are signed in"));
        assert_eq!(update(&pr, &mut st, Msg::Finish), Some(Effect::Finish("0xabc".into())));
    }

    #[test]
    fn create_with_key_and_errors() {
        let pr = props(true, Via::Usb);
        let mut st = State::default();
        let v = view(&pr, &st);
        assert_eq!(v.texts()[0], "Use your phone");
        assert!(v.contains_text("Your passkey can always sign. This device signs everyday transactions with your PIN."));
        assert_eq!(update(&pr, &mut st, Msg::Continue), Some(Effect::Gate));
        assert_eq!(gate(&pr, &mut st, Gate::Pin), None);
        assert!(view(&pr, &st).contains_text("Set a device PIN"));
        let e = ran(&pr, &mut st, 0, Err(Failure::code("notfound")));
        assert_eq!(e, None);
        assert_eq!(st.step, Step::Choose);
        assert!(view(&pr, &st).contains_text("No security key found. Insert it and try again."));
        // a cancelled run's answer is ignored
        st.run = 5;
        assert_eq!(ran(&pr, &mut st, 4, Ok(("0x1".into(), true))), None);
        assert_eq!(ran(&pr, &mut st, 5, Ok(("0x1".into(), true))), Some(Effect::Finish("0x1".into())));
    }
}
