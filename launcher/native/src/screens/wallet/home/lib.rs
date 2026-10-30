//! Shared helpers for the wallet's home, coin, collectible and activity pages
//! (launcher/src/wallet/ui/home/lib.tsx, bits.tsx): account kind labels, digests, day headings,
//! the loading bar, the copy button, and plain PIN errors.

use crate::screens::wallet::fmt::{date_md, local};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::RareAccount;

/// `isSuiType`: `0x2::sui::SUI` in any padding.
pub fn is_sui_type(t: &str) -> bool {
    let Some(rest) = t.strip_prefix("0x") else { return false };
    let Some(addr) = rest.strip_suffix("::sui::SUI") else { return false };
    addr.trim_start_matches('0') == "2"
}

/// `kindLabel`: how the account signs, as a short plain label.
pub fn kind_label(a: &RareAccount) -> &'static str {
    match a.kind.as_str() {
        "watch" => "Watch only",
        "ledger" => "Ledger",
        "keystone" => "Keystone",
        "passkey" => "Passkey",
        "social" => "Social",
        _ => {
            if a.hd.is_some() {
                "Recovery phrase"
            } else {
                "Private key"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindGroup {
    Rare,
    Hardware,
    Watch,
}

/// `kindGroup`: which group an account falls under on the Accounts page.
pub fn kind_group(a: &RareAccount) -> KindGroup {
    match a.kind.as_str() {
        "watch" => KindGroup::Watch,
        "ledger" | "keystone" => KindGroup::Hardware,
        _ => KindGroup::Rare,
    }
}

/// `shortDigest`.
pub fn short_digest(d: &str) -> String {
    let c: Vec<char> = d.chars().collect();
    if c.len() > 12 {
        format!("{}...{}", c[..6].iter().collect::<String>(), c[c.len() - 4..].iter().collect::<String>())
    } else {
        d.to_string()
    }
}

/// `dayLabel`: "Today", "Yesterday", "Mar 4", "Mar 4, 2025" (local time, `tz_min` east of UTC).
pub fn day_label(ts: i64, now: i64, tz_min: i32) -> String {
    let d = local(ts, tz_min);
    let n = local(now, tz_min);
    if d.days == n.days {
        return "Today".into();
    }
    if d.days == local(now - 86_400_000, tz_min).days {
        return "Yesterday".into();
    }
    date_md(ts, tz_min, d.year != n.year)
}

/// The thin loading bar from the Marketplace (no skeletons).
pub fn load_bar<M>(on: bool) -> El<M> {
    div(cn(&["pointer-events-none h-[2px] w-full", when(on, "load-bar")]))
}

/// `CopyValue`: a value in mono with a copy button (digests, addresses). `done` is the 1.5 s "copied" state.
pub fn copy_value<M>(shown: &str, label: &str, done: bool, on: M) -> El<M> {
    button("inline-flex h-8 items-center gap-1.5 rounded px-2 font-mono text-sm transition-colors focus-visible:bg-layer-03", Some(on))
        .label(label)
        .kid(span("").text(shown))
        .kid(if done { icon("check", "").attr("size", "0.9rem") } else { icon("copy", "text-secondary").attr("size", "0.9rem") })
}

/// What the vault said when a PIN or passphrase did not open it (`NativeError`).
#[derive(Clone, Debug, PartialEq)]
pub enum SecretFail {
    Wrong { retry_after_ms: u64 },
    Throttled { retry_after_ms: u64 },
    Locked,
    Other(Option<String>),
}

/// `secretError`: a plain message for a wrong or throttled PIN.
pub fn secret_error(e: &SecretFail, noun: &str) -> String {
    match e {
        SecretFail::Wrong { retry_after_ms } if *retry_after_ms > 1000 => format!("Wrong {noun}. Wait {} seconds and try again.", retry_after_ms.div_ceil(1000)),
        SecretFail::Wrong { .. } => format!("Wrong {noun}. Try again."),
        SecretFail::Throttled { retry_after_ms } => format!("Too many tries. Wait {} seconds.", retry_after_ms.div_ceil(1000)),
        SecretFail::Locked => "The wallet is locked.".into(),
        SecretFail::Other(Some(m)) if m.chars().count() < 140 => m.clone(),
        SecretFail::Other(_) => "That didn't work. Try again.".into(),
    }
}

/// A bottom-bar hint while a page is on top (`Hint` in Chrome.tsx): the glyph and its label.
#[derive(Clone, Debug, PartialEq)]
pub struct Hint {
    pub b: &'static str,
    pub label: String,
}

pub fn hint(b: &'static str, label: &str) -> Hint {
    Hint { b, label: label.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert!(is_sui_type("0x2::sui::SUI"));
        assert!(is_sui_type("0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI"));
        assert!(!is_sui_type("0x3::sui::SUI"));
        let mut a = RareAccount::new("0x1", "Main", "native");
        assert_eq!(kind_label(&a), "Private key");
        a.hd = Some(Default::default());
        assert_eq!(kind_label(&a), "Recovery phrase");
        assert_eq!(kind_label(&RareAccount::new("0x1", "W", "watch")), "Watch only");
        assert_eq!(kind_group(&RareAccount::new("0x1", "K", "keystone")), KindGroup::Hardware);
        assert_eq!(short_digest("8kQ1abcdefghWXYZ"), "8kQ1ab...WXYZ");
        assert_eq!(short_digest("short"), "short");
    }

    #[test]
    fn days() {
        let now = 1_793_113_620_000; // 2026-10-27 15:07 UTC
        assert_eq!(day_label(now - 3_600_000, now, 0), "Today");
        assert_eq!(day_label(now - 86_400_000, now, 0), "Yesterday");
        assert_eq!(day_label(now - 5 * 86_400_000, now, 0), "Oct 22");
        assert_eq!(day_label(now - 400 * 86_400_000, now, 0), "Sep 22, 2025");
    }

    #[test]
    fn secret_errors() {
        assert_eq!(secret_error(&SecretFail::Wrong { retry_after_ms: 0 }, "PIN"), "Wrong PIN. Try again.");
        assert_eq!(secret_error(&SecretFail::Wrong { retry_after_ms: 30_000 }, "PIN"), "Wrong PIN. Wait 30 seconds and try again.");
        assert_eq!(secret_error(&SecretFail::Throttled { retry_after_ms: 4500 }, "PIN"), "Too many tries. Wait 5 seconds.");
        assert_eq!(secret_error(&SecretFail::Other(None), "PIN"), "That didn't work. Try again.");
    }
}
