//! Receive (launcher/src/wallet/ui/pages/Receive.tsx): a big QR of the address, the full address,
//! Copy, Verify on Ledger for Ledger accounts, and a note for how this account signs.

use crate::screens::wallet::home::lib::{hint, kind_label, Hint};
use crate::screens::wallet::kit::{btn, btn_icon, page, panel, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::RareAccount;

/// `verifyOnLedger`'s answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Verify {
    Busy,
    Ok,
    Different,
    Error { message: String, fix: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Copy(String),
    Verify,
}

pub fn view(account: Option<&RareAccount>, copied: bool, check: Option<&Verify>) -> Option<El<Msg>> {
    let a = account?;
    let mut actions = div("mt-4 flex flex-wrap gap-2").kid(btn_icon(if copied { "check" } else { "copy" }, if copied { "Copied" } else { "Copy address" }, Variant::Primary, Size::Lg, "", Some(Msg::Copy(a.address.clone()))).def(true));
    if a.kind == "ledger" {
        actions = actions.kid(btn(if check == Some(&Verify::Busy) { "Check your Ledger" } else { "Verify on Ledger" }, Variant::Secondary, "", Some(Msg::Verify)));
    }
    let mut right = div("min-w-0")
        .kid(div("text-sm text-secondary").text("Your address"))
        .kid(panel(None, None, "mt-2 px-5 py-4", vec![p("break-all font-mono text-lg leading-snug").text(&a.address)]))
        .kid(actions);
    match check {
        Some(Verify::Ok) => right = right.kid(div("mt-3 text-sm").kid(p("text-secondary").text("Your Ledger shows the same address."))),
        Some(Verify::Different) => right = right.kid(div("mt-3 text-sm").kid(p("text-negative").text("Your Ledger shows a different address. Don't use this one."))),
        Some(Verify::Error { message, fix }) => {
            right = right.kid(div("mt-3 text-sm").kid(p("text-negative").text(message)).kid_opt(fix.as_ref().map(|f| p("mt-2 break-all rounded border border-border-primary bg-layer-01 p-3 font-mono text-xs text-secondary").text(f))));
        }
        Some(Verify::Busy) | None => {}
    }
    let mut notes = ul("mt-6 flex flex-col gap-1.5 text-sm text-secondary").kid(li("").text("Only send Sui network assets to this address."));
    match a.kind.as_str() {
        "watch" => notes = notes.kid(li("").text("This wallet is watch only. You can receive here, but you can't send from it.")),
        "ledger" => notes = notes.kid(li("").text("Verify on Ledger shows this address on the device so you can match it before sharing.")),
        "keystone" => notes = notes.kid(li("").text("This address belongs to your Keystone. Sending needs the device.")),
        _ => {}
    }
    right = right.kid(notes);
    Some(page(
        Some("Receive"),
        Some(text(format!("{}, {}", a.label, kind_label(a).to_lowercase()))),
        None,
        false,
        "",
        vec![div("mt-2 grid grid-cols-[auto_minmax(0,1fr)] items-center gap-12").kid(el(Tag::Qr(a.address.clone()), "size-[20rem]")).kid(right)],
    ))
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Select"), hint("b", "Back")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::{account, ME};

    #[test]
    fn receive() {
        let v = view(Some(&account()), false, None).unwrap();
        assert_eq!(v.texts(), ["Receive", "Main, passkey", "Your address", ME, "Copy address", "Only send Sui network assets to this address."]);
        assert_eq!(v.landing().as_deref(), Some("Copy address"));
        let mut l = account();
        l.kind = "ledger".into();
        let v = view(Some(&l), true, Some(&Verify::Different)).unwrap();
        let t = v.texts();
        assert!(t.contains(&"Copied".to_string()));
        assert!(t.contains(&"Verify on Ledger".to_string()));
        assert!(t.contains(&"Your Ledger shows a different address. Don't use this one.".to_string()));
        assert!(t.contains(&"Verify on Ledger shows this address on the device so you can match it before sharing.".to_string()));
        assert!(view(Some(&l), false, Some(&Verify::Busy)).unwrap().find_nav("Check your Ledger").is_some());
    }
}
