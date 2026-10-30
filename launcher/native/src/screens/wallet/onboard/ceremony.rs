//! What the person sees while a passkey ceremony runs (launcher/src/wallet/ui/onboard/Ceremony.tsx):
//! a QR for the phone (hybrid over Bluetooth), or "insert your security key", with a PIN pad sheet
//! when the key asks for its PIN. Also "Turn on Bluetooth" (BluetoothStep.tsx).

use crate::screens::wallet::kit::{btn, btn_full, pin_pad, pin_press, sheet, PinPadSpec, SheetSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::sign::panels::Passkey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Phone,
    Usb,
}

/// `CeremonyState` (passkey/index.ts).
#[derive(Clone, Debug, PartialEq)]
pub enum Ceremony {
    Idle,
    Qr { url: String },
    /// stage: "waiting" | "bluetooth" | "connecting" | "touch" | "processing"
    Status { stage: String, message: String },
    Pin { attempts_left: Option<u32> },
    Error { code: String, message: String },
}

impl Ceremony {
    /// The review sheet's view of it.
    pub fn panel(&self) -> Passkey {
        match self {
            Ceremony::Idle => Passkey::Idle,
            Ceremony::Qr { url } => Passkey::Qr { url: url.clone() },
            Ceremony::Status { message, .. } => Passkey::Status { message: message.clone() },
            Ceremony::Pin { attempts_left } => Passkey::Pin { attempts_left: *attempts_left },
            Ceremony::Error { message, .. } => Passkey::Error { message: message.clone() },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Cancel,
    PinKey(&'static str),
    /// B on the PIN sheet: hand the key no PIN (cancels it)
    PinCancel,
}

/// A PIN key press on the security key's sheet: Some(pin) to hand to the key.
pub fn pin_key(pin: &mut String, k: &'static str) -> Option<String> {
    let (next, done) = pin_press(pin, k, None, 4, 12);
    if done {
        pin.clear();
        Some(next)
    } else {
        *pin = next;
        None
    }
}

/// `qr_url` is the ceremony store's last QR (it stays while the phone connects).
pub fn view(via: Via, state: &Ceremony, qr_url: Option<&str>, pin: &str) -> El<Msg> {
    if let Ceremony::Pin { attempts_left } = state {
        return sheet(
            SheetSpec {
                title: "Security key PIN".into(),
                subtitle: Some(attempts_left.map(|n| format!("{n} tries left")).unwrap_or_else(|| "Enter the PIN you set on your key".into())),
                icon: None,
                footer: None,
                close: Some(Msg::PinCancel),
                width: "30rem",
            },
            vec![pin_pad(PinPadSpec { value: pin, min_length: 4, ..Default::default() }, Msg::PinKey).on_pad("x", Msg::PinKey("back")).on_pad("start", Msg::PinKey("ok"))],
        );
    }
    let scanned = matches!(state, Ceremony::Status { stage, .. } if stage == "connecting" || stage == "touch" || stage == "processing");
    let show_qr = via == Via::Phone && qr_url.is_some() && !scanned;
    let status = match state {
        Ceremony::Status { message, .. } => Some(message.as_str()),
        _ => None,
    };
    let mut d = div("flex flex-col items-center gap-5 text-center");
    // a second touch says why (sign in: two accounts fit the first signature)
    if let Some(n) = crate::screens::wallet::service::passkey_note() {
        d = d.kid(p("max-w-[30rem] text-base font-semibold").role("status").text(n));
    }
    if *state == Ceremony::Idle {
        d = d.kid(div("pointer-events-none absolute left-0 right-0 top-0 z-30 h-[2px] load-bar"));
    }
    if via == Via::Phone {
        if show_qr {
            d = d
                .kid(el(Tag::Qr(qr_url.unwrap_or("").to_string()), "size-[19rem]"))
                .kid(p("max-w-[30rem] text-base").text("Scan this code with your phone's camera, then approve with Face ID or your fingerprint."))
                .kid(p("max-w-[30rem] text-sm text-secondary").text("Your phone connects to this SuiPlay over Bluetooth, so keep Bluetooth on for both."))
                .kid_opt(status.map(|s| p("text-sm text-secondary").text(s)));
        } else {
            d = d.kid(div("py-14").kid(p("text-xl font-semibold").text(if scanned { "Approve on your phone" } else { "Getting ready" })).kid(p("mt-2 text-sm text-secondary").text(status.unwrap_or("Starting the connection"))));
        }
    } else {
        d = d.kid(div("py-12").kid(p("text-xl font-semibold").text("Insert your security key and touch it")).kid(p("mt-2 text-sm text-secondary").text(status.unwrap_or("Plug it into the USB port. Touch it when it blinks."))));
    }
    d.kid(btn_full("Cancel", Variant::Ghost, Size::Md, false, "", Some(Msg::Cancel)))
}

#[derive(Clone, Debug, PartialEq)]
pub enum BtMsg {
    TurnOn,
    Cancel,
}

/// `BluetoothStep`. `error` is the last try's message.
pub fn bluetooth_view(note: Option<&str>, busy: bool, error: Option<&str>, can_cancel: bool) -> El<BtMsg> {
    let mut d = div("flex flex-col items-center gap-5 py-10 text-center")
        .kid(icon("bluetooth", "text-icon-secondary").attr("size", "2.5rem").attr("stroke-width", "1.5"))
        .kid(div("").kid(p("text-xl font-semibold").text("Turn on Bluetooth")).kid(p("mt-2 max-w-[30rem] text-sm text-secondary").text(note.unwrap_or("Your phone and this SuiPlay talk over Bluetooth to set up your passkey."))));
    if let Some(e) = error {
        d = d.kid(p("max-w-[30rem] text-sm text-negative").text(e));
    }
    d = d.kid(btn(if busy { "Turning on" } else { "Turn on Bluetooth" }, Variant::Primary, "w-64", Some(BtMsg::TurnOn)).def(true));
    if can_cancel {
        d = d.kid(btn_full("Cancel", Variant::Ghost, Size::Md, false, "", Some(BtMsg::Cancel)));
    }
    d
}

/// What turning Bluetooth on answered: powered, blocked by a switch, or an error message.
pub fn bluetooth_error(powered: bool, blocked: bool, native_message: Option<&str>) -> Option<String> {
    if let Some(m) = native_message {
        return Some(m.to_string());
    }
    if powered {
        return None;
    }
    Some(if blocked { "Bluetooth is blocked by a switch on this device." } else { "Could not turn Bluetooth on. Try again." }.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_ceremony() {
        let v = view(Via::Phone, &Ceremony::Qr { url: "FIDO:/123".into() }, Some("FIDO:/123"), "");
        assert_eq!(v.texts(), ["Scan this code with your phone's camera, then approve with Face ID or your fingerprint.", "Your phone connects to this SuiPlay over Bluetooth, so keep Bluetooth on for both.", "Cancel"]);
        let v = view(Via::Phone, &Ceremony::Status { stage: "touch".into(), message: "Confirm on your phone".into() }, Some("FIDO:/123"), "");
        assert_eq!(v.texts(), ["Approve on your phone", "Confirm on your phone", "Cancel"]);
        let v = view(Via::Phone, &Ceremony::Idle, None, "");
        assert_eq!(v.texts(), ["Getting ready", "Starting the connection", "Cancel"]);
        let v = view(Via::Usb, &Ceremony::Idle, None, "");
        assert_eq!(v.texts(), ["Insert your security key and touch it", "Plug it into the USB port. Touch it when it blinks.", "Cancel"]);
    }

    #[test]
    fn key_pin() {
        let v = view(Via::Usb, &Ceremony::Pin { attempts_left: Some(3) }, None, "12");
        assert_eq!(v.texts()[..2], ["Security key PIN", "3 tries left"]);
        let mut pin = String::new();
        for k in ["1", "2", "3"] {
            assert_eq!(pin_key(&mut pin, k), None);
        }
        assert_eq!(pin_key(&mut pin, "ok"), None);
        pin_key(&mut pin, "4");
        assert_eq!(pin_key(&mut pin, "ok"), Some("1234".into()));
        assert_eq!(pin, "");
    }

    #[test]
    fn bluetooth() {
        let v = bluetooth_view(None, false, None, true);
        assert_eq!(v.texts(), ["Turn on Bluetooth", "Your phone and this SuiPlay talk over Bluetooth to set up your passkey.", "Turn on Bluetooth", "Cancel"]);
        assert_eq!(bluetooth_error(false, true, None).as_deref(), Some("Bluetooth is blocked by a switch on this device."));
        assert_eq!(bluetooth_error(true, false, None), None);
    }
}
