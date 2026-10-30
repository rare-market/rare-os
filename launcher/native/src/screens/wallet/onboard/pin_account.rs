//! "Create account with PIN": set a PIN for this SuiPlay's device key, add a backup (your phone,
//! a security key, or both), and the account's address. The device signs with the PIN; a backup
//! signs anywhere (rare.market) and unlocks the device after 5 wrong PINs. Effects run in
//! `flows/pin_account.rs`.

use super::ceremony::{self, bluetooth_view, BtMsg, Ceremony, Via};
use super::flow::{plain_error, udev_fix, Failure};
use super::hw_parts::hw_error;
use super::pin_setup;
use crate::screens::wallet::kit::{btn, pin_pad, pin_press, PinPadSpec, Variant};
use crate::screens::wallet::markup::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Pin,
    Backups,
    Bluetooth,
    Run,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backups {
    Phone,
    Key,
    Both,
}

impl Backups {
    pub fn vias(self) -> Vec<Via> {
        match self {
            Backups::Phone => vec![Via::Phone],
            Backups::Key => vec![Via::Usb],
            Backups::Both => vec![Via::Phone, Via::Usb],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub step: Step,
    pub backups: Backups,
    pub pin: pin_setup::State,
    /// the PIN chosen (or entered, when this SuiPlay has one), until the account is made
    pub chosen: Option<String>,
    pub error: Option<(String, Option<String>)>,
    pub run: u64,
    pub bt_note: Option<String>,
    pub bt_busy: bool,
    pub bt_error: Option<String>,
    pub key_pin: String,
    /// the new account
    pub address: Option<String>,
    /// restoring a PIN account from its 12 words (the backup signs in once)
    pub restore: bool,
}

impl Default for State {
    fn default() -> Self {
        State { step: Step::Pin, backups: Backups::Phone, pin: pin_setup::State::default(), chosen: None, error: None, run: 0, bt_note: None, bt_busy: false, bt_error: None, key_pin: String::new(), address: None, restore: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Pin(pin_setup::Msg),
    Pick(Backups),
    Continue,
    Bt(BtMsg),
    Ceremony(ceremony::Msg),
    /// Done: open "Move everything to this account"
    Move,
    Finish,
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    CheckBluetooth,
    PowerOn,
    /// make the device key (or open it with the PIN) and the backups, in order (run id)
    Start { run: u64, vias: Vec<Via>, pin: String },
    CancelCeremony,
    KeyPin(Option<String>),
    Move(String),
    Finish(String),
    Exit,
}

pub struct Props<'a> {
    /// this SuiPlay already has a PIN (another PIN account): enter it instead of setting one
    pub has_pin: bool,
    /// which backup the ceremony on screen is for
    pub via: Via,
    pub ceremony: &'a Ceremony,
    pub qr_url: Option<&'a str>,
    /// "Checked by this SuiPlay's security chip", ...
    pub kept: &'a str,
    /// restore: this SuiPlay already lists the account, so no backup has to sign in
    pub known: bool,
}

pub const PIN_BODY: &str = "You sign on this SuiPlay with this PIN. After 5 wrong PINs it locks until your backup unlocks it.";
pub const RESTORE_PIN_BODY: &str = "Your account signs on this SuiPlay with this PIN. After 5 wrong PINs it locks until your backup unlocks it.";
pub const RESTORE_BACKUP_BODY: &str = "Your phone or security key signs in once, so this SuiPlay knows the account. The address stays the same.";
pub const BACKUP_BODY: &str = "Your backup signs on rare.market and anywhere you are away from this SuiPlay, and unlocks it if you forget the PIN.";

fn choice(on: bool, ic: &'static str, title: &str, body: &str, msg: Msg, def: bool) -> El<Msg> {
    button(cn(&["flex w-full items-center gap-4 rounded border px-5 py-4 text-left transition-colors", if on { "border-border-light bg-layer-02" } else { "border-border-primary bg-layer-01" }]), Some(msg))
        .def(def)
        .role("radio")
        .attr("aria-checked", on.to_string())
        .kid(span("text-icon-primary").kid(icon(ic, "").attr("size", "1.5rem")))
        .kid(span("min-w-0 flex-1").kid(span("block text-base font-medium").text(title)).kid(span("mt-0.5 block text-sm text-secondary").text(body)))
        .kid(span(cn(&["flex size-5 shrink-0 items-center justify-center rounded-full border", if on { "border-primary" } else { "border-border-light" }])).kid_if(on, || span("size-2.5 rounded-full bg-primary")))
}

/// `0x1234 5678 ...`: groups of four, easy to compare by eye.
pub fn grouped(a: &str) -> String {
    let hex: Vec<char> = a.trim_start_matches("0x").chars().collect();
    format!("0x {}", hex.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" "))
}

pub fn title(st: &State) -> Option<&'static str> {
    match st.step {
        Step::Pin | Step::Done => None,
        Step::Backups | Step::Bluetooth | Step::Run if st.restore => Some("Sign in with your backup"),
        Step::Backups => Some("Add a backup"),
        Step::Bluetooth | Step::Run => Some("Add a backup"),
    }
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let root = match st.step {
        Step::Pin if pr.has_pin => {
            let mut d = div("flex flex-col items-center text-center")
                .kid(h2("text-xl font-bold").text("Enter your PIN"))
                .kid(p("mt-1 max-w-[28rem] text-sm text-secondary").text("This SuiPlay already has a PIN. Your new account uses it too."))
                .kid(div("mt-6 w-full").kid(pin_pad(PinPadSpec { value: &st.pin.value, ..Default::default() }, |k| Msg::Pin(pin_setup::Msg::Key(k))).on_pad("x", Msg::Pin(pin_setup::Msg::Key("back"))).on_pad("start", Msg::Pin(pin_setup::Msg::Key("ok")))));
            if let Some((m, _)) = &st.error {
                d = d.kid(p("mt-4 max-w-[28rem] text-sm text-negative").text(m));
            }
            d
        }
        Step::Pin => {
            let (t, b) = if st.restore { ("Set a PIN for this SuiPlay", RESTORE_PIN_BODY) } else { ("Create account with PIN", PIN_BODY) };
            let mut d = div("pt-4").kid(pin_setup::view_pin_only(&st.pin, t, b, false).map(&Msg::Pin));
            if let Some((m, _)) = &st.error {
                d = d.kid(p("mt-4 text-center text-sm text-negative").text(m));
            }
            d
        }
        Step::Backups if st.restore => {
            let b = st.backups;
            let mut d = div("flex flex-col gap-4").kid(p("text-sm text-secondary").text(RESTORE_BACKUP_BODY)).kid(
                div("flex flex-col gap-2")
                    .role("radiogroup")
                    .kid(choice(b != Backups::Key, "smartphone", "Use your phone", "Scan a code with your phone's camera, then use Face ID or your fingerprint", Msg::Pick(Backups::Phone), true))
                    .kid(choice(b == Backups::Key, "usb", "Use your security key", "Plug it into the USB-C port on top and touch it", Msg::Pick(Backups::Key), false)),
            );
            if let Some((m, fix)) = &st.error {
                d = d.kid(hw_error(m, fix.as_deref(), false, false).map(&|_| Msg::Continue));
            }
            d.kid(btn("Continue", Variant::Primary, "w-full", Some(Msg::Continue)))
        }
        Step::Backups => {
            let b = st.backups;
            let mut d = div("flex flex-col gap-4")
                .kid(p("text-sm text-secondary").text(BACKUP_BODY))
                .kid(
                    div("flex flex-col gap-2")
                        .role("radiogroup")
                        .kid(choice(b == Backups::Phone, "smartphone", "Add your phone", "Scan a code with your phone's camera, then use Face ID or your fingerprint", Msg::Pick(Backups::Phone), true))
                        .kid(choice(b == Backups::Key, "usb", "Add a security key", "A USB key such as a YubiKey, in the USB-C port on top", Msg::Pick(Backups::Key), false))
                        .kid(choice(b == Backups::Both, "shield-check", "Add both", "Your phone and a security key. Either one can sign and unlock.", Msg::Pick(Backups::Both), false)),
                );
            if let Some((m, fix)) = &st.error {
                d = d.kid(hw_error(m, fix.as_deref(), false, false).map(&|_| Msg::Continue));
            }
            d.kid(btn("Continue", Variant::Primary, "w-full", Some(Msg::Continue)))
        }
        Step::Bluetooth => bluetooth_view(st.bt_note.as_deref(), st.bt_busy, st.bt_error.as_deref(), true).map(&Msg::Bt),
        Step::Run => div("pt-2").kid(ceremony::view(pr.via, pr.ceremony, pr.qr_url, &st.key_pin).map(&Msg::Ceremony)),
        Step::Done => {
            let a = st.address.as_deref().unwrap_or("");
            let backup = match st.backups {
                Backups::Phone => "your phone",
                Backups::Key => "your security key",
                Backups::Both => "your phone or your security key",
            };
            let (head, line) = if st.restore {
                ("Your account is back", "This SuiPlay signs with your PIN again. Same address, same backups.".to_string())
            } else {
                ("Your account is ready", format!("This SuiPlay signs with your PIN. On rare.market and anywhere else, {backup} signs."))
            };
            div("flex flex-col items-center gap-4 py-6 text-center")
                .kid(icon("circle-check", "text-icon-primary").attr("size", "2.5rem").attr("stroke-width", "1.5"))
                .kid(h2("text-xl font-bold").text(head))
                .kid(p("max-w-[34rem] text-secondary").text(line))
                .kid(div("w-full max-w-[34rem] rounded border border-border-primary bg-layer-01 px-5 py-4").kid(p("text-sm text-secondary").text("Address")).kid(p("mt-1 break-all font-mono text-base").text(grouped(a))))
                .kid(p("max-w-[34rem] text-sm text-secondary").text(pr.kept))
                .kid(div("mt-2 flex w-full max-w-[34rem] gap-3").kid(btn("Move everything here", Variant::Secondary, "flex-1", Some(Msg::Move))).kid(btn("Done", Variant::Primary, "flex-1", Some(Msg::Finish)).def(true)))
        }
    };
    if matches!(st.step, Step::Backups | Step::Bluetooth | Step::Run) || st.step == Step::Pin {
        root.on_pad("b", Msg::Back)
    } else {
        root
    }
}

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Pin(m) if pr.has_pin => {
            let pin_setup::Msg::Key(k) = m else { return None };
            let (next, done) = pin_press(&st.pin.value, k, None, 6, 12);
            st.pin.value = next.clone();
            if done {
                st.pin.value.clear();
                st.chosen = Some(next);
                st.error = None;
                st.step = Step::Backups;
                if st.restore && pr.known {
                    return start(st);
                }
            }
            None
        }
        Msg::Pin(m) => match pin_setup::update(&mut st.pin, m, false)? {
            pin_setup::Out::Done(p) => {
                st.chosen = Some(p.secret);
                st.error = None;
                st.step = Step::Backups;
                if st.restore && pr.known {
                    return start(st);
                }
                None
            }
            pin_setup::Out::AskPass { .. } => None,
        },
        Msg::Pick(b) => {
            st.backups = b;
            None
        }
        Msg::Continue => {
            st.error = None;
            if st.backups.vias().contains(&Via::Phone) {
                Some(Effect::CheckBluetooth)
            } else {
                start(st)
            }
        }
        Msg::Bt(BtMsg::TurnOn) => {
            st.bt_busy = true;
            st.bt_error = None;
            Some(Effect::PowerOn)
        }
        Msg::Bt(BtMsg::Cancel) => {
            st.step = Step::Backups;
            None
        }
        Msg::Ceremony(ceremony::Msg::Cancel) => Some(cancel(st)),
        Msg::Ceremony(ceremony::Msg::PinKey(k)) => ceremony::pin_key(&mut st.key_pin, k).map(|p_| Effect::KeyPin(Some(p_))),
        Msg::Ceremony(ceremony::Msg::PinCancel) => {
            st.key_pin.clear();
            Some(Effect::KeyPin(None))
        }
        Msg::Move => st.address.clone().map(Effect::Move),
        Msg::Finish => st.address.clone().map(Effect::Finish),
        Msg::Back => match st.step {
            Step::Run => Some(cancel(st)),
            Step::Bluetooth => {
                st.step = Step::Backups;
                None
            }
            Step::Backups => {
                st.step = Step::Pin;
                st.pin = pin_setup::State::default();
                st.chosen = None;
                None
            }
            Step::Pin if st.pin.first.is_some() => {
                st.pin = pin_setup::State::default();
                None
            }
            _ => Some(Effect::Exit),
        },
    }
}

fn cancel(st: &mut State) -> Effect {
    st.run += 1;
    st.step = Step::Backups;
    Effect::CancelCeremony
}

fn start(st: &mut State) -> Option<Effect> {
    let pin = st.chosen.clone()?;
    st.run += 1;
    st.error = None;
    st.step = Step::Run;
    let vias = if st.restore { vec![if st.backups == Backups::Key { Via::Usb } else { Via::Phone }] } else { st.backups.vias() };
    Some(Effect::Start { run: st.run, vias, pin })
}

pub fn bt_ready(st: &mut State, ready: bool) -> Option<Effect> {
    if !ready {
        st.bt_note = Some("Your phone and this SuiPlay talk over Bluetooth to add your phone.".into());
        st.step = Step::Bluetooth;
        return None;
    }
    start(st)
}

pub fn bt_result(st: &mut State, powered: bool, blocked: bool, native_message: Option<&str>) -> Option<Effect> {
    st.bt_busy = false;
    match ceremony::bluetooth_error(powered, blocked, native_message) {
        None => start(st),
        Some(e) => {
            st.bt_error = Some(e);
            None
        }
    }
}

/// The run's answer: the account's address, or why not.
pub fn ran(st: &mut State, run: u64, res: Result<String, Failure>) -> Option<Effect> {
    if run != st.run {
        return None;
    }
    match res {
        Ok(address) => {
            st.chosen = None;
            st.address = Some(address);
            st.step = Step::Done;
            None
        }
        Err(e) => {
            let code = e.code.as_deref();
            if code == Some("cancelled") {
                st.step = Step::Backups;
                return None;
            }
            if code == Some("ble-off") {
                st.bt_note = Some("Turn Bluetooth back on to finish with your phone.".into());
                st.step = Step::Bluetooth;
                return None;
            }
            // a wrong PIN for this SuiPlay's existing key: enter it again
            if matches!(code, Some("wrong") | Some("pin-locked")) {
                st.error = Some((plain_error(&e, "Wrong PIN"), None));
                st.chosen = None;
                st.step = Step::Pin;
                return None;
            }
            let message = if code == Some("notfound") { "No security key found. Plug it into the USB-C port on top and try again.".to_string() } else { plain_error(&e, "Could not add the backup. Try again.") };
            st.error = Some((message, udev_fix(&e)));
            st.step = Step::Backups;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(has_pin: bool) -> Props<'static> {
        Props { has_pin, via: Via::Phone, ceremony: &Ceremony::Idle, qr_url: None, kept: "Checked by this SuiPlay's security chip", known: false }
    }

    fn keys(pr: &Props, st: &mut State, pin: &str) {
        for c in pin.chars() {
            let k: &'static str = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"][c.to_digit(10).unwrap() as usize];
            update(pr, st, Msg::Pin(pin_setup::Msg::Key(k)));
        }
    }

    #[test]
    fn set_pin_pick_both_and_land() {
        let pr = props(false);
        let mut st = State::default();
        let v = view(&pr, &st);
        assert!(v.contains_text("Create account with PIN"));
        assert!(v.contains_text(PIN_BODY));
        assert!(!v.contains_text("Use a passphrase instead"));
        keys(&pr, &mut st, "482916");
        assert!(view(&pr, &st).contains_text("Enter it again"));
        keys(&pr, &mut st, "482916");
        assert_eq!(st.step, Step::Backups);
        let v = view(&pr, &st);
        assert!(v.contains_text("Add your phone") && v.contains_text("Add a security key") && v.contains_text("Add both"));
        update(&pr, &mut st, Msg::Pick(Backups::Both));
        assert_eq!(update(&pr, &mut st, Msg::Continue), Some(Effect::CheckBluetooth));
        assert_eq!(bt_ready(&mut st, true), Some(Effect::Start { run: 1, vias: vec![Via::Phone, Via::Usb], pin: "482916".into() }));
        assert_eq!(ran(&mut st, 1, Ok("0xabc".into())), None);
        assert_eq!(st.chosen, None);
        let v = view(&pr, &st);
        assert!(v.contains_text("Your account is ready"));
        assert!(v.contains_text("This SuiPlay signs with your PIN. On rare.market and anywhere else, your phone or your security key signs."));
        assert_eq!(update(&pr, &mut st, Msg::Move), Some(Effect::Move("0xabc".into())));
        // the typed PIN is never on screen
        assert!(!v.texts().iter().any(|t| t.contains("482916")));
    }

    #[test]
    fn restore_sets_a_pin_then_one_backup() {
        let pr = props(false);
        let mut st = State { restore: true, ..State::default() };
        assert!(view(&pr, &st).contains_text("Set a PIN for this SuiPlay"));
        keys(&pr, &mut st, "135792");
        keys(&pr, &mut st, "135792");
        let v = view(&pr, &st);
        assert!(v.contains_text(RESTORE_BACKUP_BODY) && !v.contains_text("Add both"));
        update(&pr, &mut st, Msg::Pick(Backups::Key));
        assert_eq!(update(&pr, &mut st, Msg::Continue), Some(Effect::Start { run: 1, vias: vec![Via::Usb], pin: "135792".into() }));
        ran(&mut st, 1, Ok("0xabc".into()));
        assert!(view(&pr, &st).contains_text("Your account is back"));
        // the account is listed here already: no backup step
        let pr = Props { known: true, ..props(false) };
        let mut st = State { restore: true, ..State::default() };
        keys(&pr, &mut st, "135792");
        keys(&pr, &mut st, "135792");
        assert_eq!(st.step, Step::Run);
    }

    #[test]
    fn key_only_skips_bluetooth_and_errors_go_back() {
        let pr = props(true);
        let mut st = State::default();
        assert!(view(&pr, &st).contains_text("Enter your PIN"));
        keys(&pr, &mut st, "111111");
        update(&pr, &mut st, Msg::Pin(pin_setup::Msg::Key("ok")));
        assert_eq!(st.step, Step::Backups);
        update(&pr, &mut st, Msg::Pick(Backups::Key));
        assert!(matches!(update(&pr, &mut st, Msg::Continue), Some(Effect::Start { .. })));
        let run = st.run;
        ran(&mut st, run, Err(Failure { code: Some("wrong".into()), message: Some("Wrong PIN".into()), fix: None }));
        assert_eq!(st.step, Step::Pin);
        assert!(view(&pr, &st).contains_text("Wrong PIN"));
        assert_eq!(st.chosen, None);
    }
}
