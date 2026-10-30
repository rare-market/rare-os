//! What the review sheet's footer shows while a device or phone is signing: the Ledger's live
//! state, the Keystone (with the hash to compare), or the passkey phone ceremony
//! (launcher/src/wallet/ui/sign/HardwarePanels.tsx).

use crate::screens::wallet::kit::{btn, Variant};
use crate::screens::wallet::markup::*;

/// A device error with an optional one-time fix (the udev line for Linux).
#[derive(Clone, Debug, PartialEq)]
pub struct HwError {
    pub message: String,
    pub fix: Option<String>,
}

/// `useLedgerPrompt()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LedgerPrompt {
    pub message: String,
    pub device: Option<String>,
    pub error: Option<HwError>,
}

/// `useKeystonePrompt()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeystonePrompt {
    pub message: String,
    pub digest_hex: Option<String>,
    pub error: Option<HwError>,
}

/// The passkey ceremony (`CeremonyState`).
#[derive(Clone, Debug, PartialEq)]
pub enum Passkey {
    Idle,
    Qr { url: String },
    Status { message: String },
    Pin { attempts_left: Option<u32> },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum PanelMsg {
    Retry,
    Cancel,
    /// Ask for the security key PIN on the keyboard ("Security key PIN", secret) and pass it on.
    EnterPin,
}

/// 64 hex chars as eight groups of 8, the way the Keystone shows its hash.
pub fn chunk_hash(hex: &str, size: usize) -> Vec<String> {
    hex.as_bytes().chunks(size).map(|c| String::from_utf8_lossy(c).into_owned()).collect()
}

/// What to do with the QR code, in order.
pub const PHONE_STEPS: [&str; 3] = [
    "Open the camera on your phone and point it at this code.",
    "Tap the passkey prompt and pick your Rare passkey.",
    "Confirm with your face or fingerprint. Keep Bluetooth on, near this SuiPlay.",
];

fn lead(main: &str, sub: Option<&str>) -> El<PanelMsg> {
    div("min-w-0")
        .kid(p("flex items-center gap-2.5 text-base font-semibold").kid(span("size-2 shrink-0 animate-pulse rounded-full bg-primary").attr("aria-hidden", "true")).kid(text(main)))
        .kid_opt(sub.map(|s| p("mt-1 text-sm text-secondary").text(s)))
}

fn error_block(err: &HwError, device: &str) -> El<PanelMsg> {
    let mut d = div("min-w-0").kid(p("text-base font-semibold text-negative").text(&err.message));
    if let Some(fix) = &err.fix {
        d = d
            .kid(p("mt-2 text-sm text-secondary").text(format!("Run this once, then plug the {device} in again:")))
            .kid(pre("mt-1.5 overflow-x-auto rounded border border-border-primary bg-layer-01 px-3 py-2 font-mono text-xs").text(fix));
    }
    d
}

fn buttons(err: bool) -> El<PanelMsg> {
    let mut row = div("flex gap-2");
    if err {
        row = row.kid(btn("Retry", Variant::Primary, "flex-1", Some(PanelMsg::Retry)).def(true));
    }
    row.kid(btn("Cancel", Variant::Secondary, if err { "" } else { "flex-1" }, Some(PanelMsg::Cancel)).def(!err))
}

pub fn ledger_panel(p_: &LedgerPrompt) -> El<PanelMsg> {
    let top = match &p_.error {
        Some(e) => error_block(e, "Ledger"),
        None => lead(if p_.message.is_empty() { "Connecting to your Ledger" } else { &p_.message }, p_.device.as_deref().filter(|d| !d.is_empty())),
    };
    div("flex w-full flex-col gap-4").kid(top).kid(buttons(p_.error.is_some()))
}

pub fn keystone_panel(k: &KeystonePrompt) -> El<PanelMsg> {
    let err = k.error.is_some();
    let top = match &k.error {
        Some(e) => error_block(e, "Keystone"),
        None => lead(if k.message.is_empty() { "Connecting to your Keystone" } else { &k.message }, None),
    };
    let groups = k.digest_hex.as_deref().filter(|h| !h.is_empty()).map(|h| chunk_hash(h, 8)).unwrap_or_default();
    let mut d = div("flex w-full flex-col gap-4").kid(top);
    if !err && !groups.is_empty() {
        d = d.kid(
            div("")
                .kid(
                    div("grid grid-cols-4 gap-x-4 gap-y-1.5 rounded border border-border-primary bg-layer-01 px-4 py-3 font-mono text-sm tabular-nums")
                        .label("Transaction hash")
                        .kids(groups.iter().enumerate().map(|(i, g)| span("").key(i.to_string()).text(g))),
                )
                .kid(p("mt-2 text-sm text-secondary").text("Check it matches the hash on your Keystone.")),
        );
    }
    d.kid(buttons(err))
}

pub fn passkey_panel(c: &Passkey) -> El<PanelMsg> {
    let top = match c {
        Passkey::Qr { url } => div("flex items-center gap-5").kid(el(Tag::Qr(url.clone()), "size-[10rem] shrink-0 rounded bg-white p-2")).kid(
            div("min-w-0 flex flex-col gap-1.5")
                .kid(p("text-base font-semibold").text("Sign with your phone"))
                .kids(PHONE_STEPS.iter().enumerate().map(|(i, s)| p("text-sm text-secondary").key(i.to_string()).text(format!("{}. {s}", i + 1)))),
        ),
        Passkey::Status { message } => lead(message, None),
        Passkey::Pin { attempts_left } => lead("Your security key needs its PIN", attempts_left.map(|n| format!("{n} tries left")).as_deref()),
        Passkey::Error { message } => p("text-base font-semibold text-negative").text(message),
        Passkey::Idle => lead("Getting a QR code for your phone", Some("It shows here in a moment. Keep Bluetooth on.")),
    };
    let pin = matches!(c, Passkey::Pin { .. });
    let error = matches!(c, Passkey::Error { .. });
    let mut row = div("flex gap-2");
    if pin {
        row = row.kid(btn("Enter PIN", Variant::Primary, "flex-1", Some(PanelMsg::EnterPin)).def(true));
    }
    if error {
        row = row.kid(btn("Retry", Variant::Primary, "flex-1", Some(PanelMsg::Retry)).def(true));
    }
    row = row.kid(btn("Cancel", Variant::Secondary, if pin || error { "" } else { "flex-1" }, Some(PanelMsg::Cancel)).def(!pin && !error));
    div("flex w-full flex-col gap-4").kid(top).kid(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_groups() {
        let h = "0123456789abcdef".repeat(4);
        let g = chunk_hash(&h, 8);
        assert_eq!(g.len(), 8);
        assert_eq!(g[0], "01234567");
        assert_eq!(g[7], "89abcdef");
    }

    #[test]
    fn ledger_states() {
        let w = ledger_panel(&LedgerPrompt::default());
        assert!(w.contains_text("Connecting to your Ledger"));
        assert_eq!(w.landing().as_deref(), Some("Cancel"));
        let e = ledger_panel(&LedgerPrompt { error: Some(HwError { message: "Ledger not found".into(), fix: Some("sudo udevadm".into()) }), ..Default::default() });
        assert_eq!(e.texts(), ["Ledger not found", "Run this once, then plug the Ledger in again:", "sudo udevadm", "Retry", "Cancel"]);
        assert_eq!(e.landing().as_deref(), Some("Retry"));
    }

    #[test]
    fn keystone_shows_the_hash_to_compare() {
        let k = keystone_panel(&KeystonePrompt { message: "Scan the QR on your Keystone".into(), digest_hex: Some("ab".repeat(32)), error: None });
        assert!(k.contains_text("Check it matches the hash on your Keystone."));
        assert_eq!(k.texts().iter().filter(|t| t.as_str() == "abababab").count(), 8);
    }

    #[test]
    fn passkey_states() {
        assert!(passkey_panel(&Passkey::Idle).contains_text("Getting a QR code for your phone"));
        let p = passkey_panel(&Passkey::Pin { attempts_left: Some(3) });
        assert!(p.contains_text("3 tries left"));
        assert_eq!(p.landing().as_deref(), Some("Enter PIN"));
        let q = passkey_panel(&Passkey::Qr { url: "FIDO:/123".into() });
        assert!(q.contains_text("Sign with your phone"));
        assert!(q.contains_text("1. Open the camera on your phone and point it at this code."));
        assert_eq!(q.landing().as_deref(), Some("Cancel"));
    }
}
