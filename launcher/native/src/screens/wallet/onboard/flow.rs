//! Small helpers the onboarding pages share (launcher/src/wallet/ui/onboard/flow.ts): the hints,
//! plain error text, the udev fix line, and how a finished flow lands.

use crate::screens::wallet::home::lib::{hint, Hint};

/// `H`: the hints the onboarding pages show.
pub mod h {
    use super::*;
    pub fn select() -> Hint {
        hint("a", "Select")
    }
    pub fn back() -> Hint {
        hint("b", "Back")
    }
    pub fn type_() -> Hint {
        hint("a", "Type")
    }
    pub fn del() -> Hint {
        hint("x", "Delete")
    }
    pub fn ok() -> Hint {
        hint("start", "Confirm")
    }
}

/// An error from the keystore or the Rust side (`NativeError` code and message, or a plain Error).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Failure {
    pub code: Option<String>,
    pub message: Option<String>,
    /// `data.fix` on a "udev" error: the one-line USB permission fix
    pub fix: Option<String>,
}

impl Failure {
    pub fn code(code: &str) -> Failure {
        Failure { code: Some(code.into()), ..Default::default() }
    }
    pub fn msg(m: &str) -> Failure {
        Failure { message: Some(m.into()), ..Default::default() }
    }
}

/// `plainError`: plain words, never a secret.
pub fn plain_error(e: &Failure, fallback: &str) -> String {
    if let Some(c) = e.code.as_deref() {
        let known = match c {
            "cancelled" => Some("Cancelled."),
            "timeout" => Some("That took too long. Try again."),
            "ble-off" => Some("Bluetooth is off. Turn it on and try again."),
            "notfound" => Some("No device found. Check that it is connected and try again."),
            "busy" => Some("The device is busy. Finish what it is showing, then try again."),
            "network" => Some("Couldn't reach the network. Check your connection and try again."),
            "wrong-pin" => Some("That PIN is wrong. Try again."),
            "unsupported" => Some("This is not supported on this device."),
            _ => None,
        };
        if let Some(k) = known {
            return k.into();
        }
    }
    match e.message.as_deref() {
        Some(m) if !m.is_empty() && m.chars().count() < 140 => m.into(),
        _ => fallback.into(),
    }
}

pub const DEFAULT_ERROR: &str = "That didn't go through. Try again.";

/// `udevFix`.
pub fn udev_fix(e: &Failure) -> Option<String> {
    if e.code.as_deref() == Some("udev") {
        e.fix.clone()
    } else {
        None
    }
}

/// `vaultGate`'s answer: the vault is open, there is none yet (ask for a new PIN), or unlock was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Ready,
    Pin,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors() {
        assert_eq!(plain_error(&Failure::code("ble-off"), DEFAULT_ERROR), "Bluetooth is off. Turn it on and try again.");
        assert_eq!(plain_error(&Failure::msg("Name not found"), DEFAULT_ERROR), "Name not found");
        assert_eq!(plain_error(&Failure::msg(&"x".repeat(200)), "Could not add that address"), "Could not add that address");
        assert_eq!(plain_error(&Failure::default(), DEFAULT_ERROR), DEFAULT_ERROR);
        let u = Failure { code: Some("udev".into()), message: None, fix: Some("pkexec x".into()) };
        assert_eq!(udev_fix(&u).as_deref(), Some("pkexec x"));
    }
}
