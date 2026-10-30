//! Recipient pieces shared by Send and Send collectibles (launcher/src/wallet/ui/sign/Recipient.tsx,
//! recents.ts): the resolved-name line under the field and the quick picks (own accounts, recent
//! recipients). Resolving the typed text (tx/recipient.ts `resolveRecipient`) is the wallet
//! crate's; this takes its state.

use super::review::Account;
use crate::screens::wallet::kit::{addr, list_button, wallet_avatar};
use crate::screens::wallet::markup::*;

/// tx/recipient.ts `RecipientState`.
#[derive(Clone, Debug, PartialEq)]
pub enum RecipientState {
    Empty,
    /// still typing something that is not valid yet; no error until they pause
    Typing,
    Invalid(String),
    Loading { name: String },
    /// `suins` is the name typed, or the address's default name
    Ready { address: String, suins: Option<String> },
    Missing(String),
    Error(String),
}

/// A recent recipient (this device only), newest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub address: String,
    pub suins: Option<String>,
    /// ms since the epoch
    pub at: i64,
}

/// tx/recipient.ts `shortAddress`.
pub fn short_address(a: &str) -> String {
    if a.chars().count() > 14 {
        let c: Vec<char> = a.chars().collect();
        format!("{}...{}", c[..6].iter().collect::<String>(), c[c.len() - 4..].iter().collect::<String>())
    } else {
        a.to_string()
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `since`: "Today", "3d ago", "Mar 4" (UTC date; the device runs in UTC unless set).
pub fn since(ts: i64, now: i64) -> String {
    let days = (now - ts).div_euclid(86_400_000);
    if days < 1 {
        return "Today".into();
    }
    if days < 7 {
        return format!("{days}d ago");
    }
    let (_, m, d) = crate::screens::wallet::fmt::civil(ts.div_euclid(86_400_000));
    format!("{} {d}", MONTHS[(m - 1) as usize])
}

/// `RecipientPicks`: your other accounts, then up to four recent recipients matching the filter.
pub fn recipient_picks<M>(accounts: &[Account], current: Option<&str>, recents: &[Recent], filter: &str, now: i64, on: impl Fn(&str) -> M) -> Option<El<M>> {
    let picks: Vec<&Account> = accounts.iter().filter(|a| Some(a.address.as_str()) != current).collect();
    let q = filter.trim().to_lowercase();
    let list: Vec<&Recent> = recents.iter().filter(|r| q.is_empty() || r.address.contains(&q) || r.suins.as_deref().is_some_and(|s| s.contains(&q))).collect();
    if picks.is_empty() && list.is_empty() {
        return None;
    }
    let mut d = div("min-h-0");
    if !picks.is_empty() {
        d = d.kid(h3("mb-1 text-sm text-secondary").text("Your accounts"));
        for a in picks {
            d = d.kid(list_button(Some(crate::screens::wallet::kit::account_avatar(&a.address, "2rem")), &a.label, Some(addr(&a.address, false, "")), None, on(&a.address), false, "").key(&a.address));
        }
    }
    if !list.is_empty() {
        d = d.kid(h3("mb-1 mt-3 text-sm text-secondary").text("Recent"));
        for r in list.into_iter().take(4) {
            let title = r.suins.clone().unwrap_or_else(|| short_address(&r.address));
            let body = r.suins.as_ref().map(|_| addr(&r.address, false, ""));
            d = d.kid(list_button(Some(wallet_avatar(&r.address, "2rem")), &title, body, Some(text(since(r.at, now))), on(&r.address), false, "").key(&r.address));
        }
    }
    Some(d)
}

/// `RecipientLine`: the line under the field.
pub fn recipient_line<M>(rs: &RecipientState, self_: bool) -> Option<El<M>> {
    match rs {
        RecipientState::Ready { address, suins } => {
            let mut l = p(cn(&["mt-1.5 flex items-center gap-2 text-sm", if self_ { "text-negative" } else { "text-secondary" }]));
            if self_ {
                l = l.text("Pick an address other than this account");
            } else {
                l = l.kid(wallet_avatar(address, "1.25rem")).kid_opt(suins.as_ref().map(|s| span("text-primary").text(s))).kid(addr(address, false, ""));
            }
            Some(l)
        }
        RecipientState::Loading { name } => Some(p("mt-1.5 text-sm text-secondary").text(format!("Looking up {name}"))),
        RecipientState::Invalid(m) | RecipientState::Missing(m) | RecipientState::Error(m) => Some(p("mt-1.5 text-sm text-negative").role("alert").text(m)),
        RecipientState::Empty | RecipientState::Typing => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::sign::fixtures::{accounts, ME};

    const DAY: i64 = 86_400_000;

    #[test]
    fn since_reads_plainly() {
        let now = 1_780_000_000_000;
        assert_eq!(since(now - 1000, now), "Today");
        assert_eq!(since(now - 3 * DAY, now), "3d ago");
        // 2026-03-04
        assert_eq!(since(1_772_582_400_000, now), "Mar 4");
    }

    #[test]
    fn picks_and_lines() {
        let now = 1_780_000_000_000;
        let recents = vec![
            Recent { address: "0xaaaa000000000000000000000000000000000000000000000000000000001111".into(), suins: Some("alice.sui".into()), at: now - 2 * DAY },
            Recent { address: "0xbbbb000000000000000000000000000000000000000000000000000000002222".into(), suins: None, at: now - 1000 },
        ];
        let el: El<String> = recipient_picks(&accounts(), Some(ME), &recents, "", now, |a| a.to_string()).unwrap();
        assert_eq!(el.texts(), ["Your accounts", "Ledger", "0x5ab1...d2e1", "Recent", "alice.sui", "0xaaaa...1111", "2d ago", "0xbbbb...2222", "Today"]);
        let el: El<String> = recipient_picks(&accounts(), Some(ME), &recents, "ALICE", now, |a| a.to_string()).unwrap();
        assert_eq!(el.texts().iter().filter(|t| *t == "Today").count(), 0);
        assert!(recipient_picks::<String>(&[], None, &[], "", now, |a| a.to_string()).is_none());

        let l: El<()> = recipient_line(&RecipientState::Ready { address: recents[0].address.clone(), suins: Some("alice.sui".into()) }, false).unwrap();
        assert_eq!(l.texts(), ["alice.sui", "0xaaaa...1111"]);
        let l: El<()> = recipient_line(&RecipientState::Ready { address: ME.into(), suins: None }, true).unwrap();
        assert_eq!(l.texts(), ["Pick an address other than this account"]);
        let l: El<()> = recipient_line(&RecipientState::Loading { name: "bob.sui".into() }, false).unwrap();
        assert_eq!(l.texts(), ["Looking up bob.sui"]);
        assert!(recipient_line::<()>(&RecipientState::Typing, false).is_none());
    }
}
