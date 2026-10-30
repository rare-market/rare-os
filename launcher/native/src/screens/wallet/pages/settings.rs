//! Wallet settings (launcher/src/wallet/ui/pages/Settings.tsx): lock and auto-lock, PIN, recovery
//! phrase and key, connected games, hardware help, and removing this account or every wallet. The
//! sheets are home/sheets.rs.

use crate::screens::wallet::home::lib::{hint, Hint};
use crate::screens::wallet::home::sheets::{self, ChangePin, LedgerCheck, Remove, RemoveAll, Reveal, RevealWhat};
use crate::screens::wallet::kit::{list_button, page, panel, segmented};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::{RareAccount, SecretKind};

/// home/autoLock.ts: minutes idle before the vault locks (0 = off); 15 by default.
pub const AUTOLOCK_OPTIONS: [u32; 4] = [0, 1, 5, 15];
pub const AUTOLOCK_DEFAULT: u32 = 15;

/// `getAutoLock` over the stored value.
pub fn auto_lock(stored: Option<&str>) -> u32 {
    match stored.and_then(|v| v.parse::<u32>().ok()) {
        Some(n) if AUTOLOCK_OPTIONS.contains(&n) => n,
        _ => AUTOLOCK_DEFAULT,
    }
}

/// `VaultStatus`: whether there is a vault, how it opens and where keys live.
#[derive(Clone, Debug, PartialEq)]
pub struct Vault {
    pub exists: bool,
    pub kind: SecretKind,
    /// "keyring" | "file"
    pub backend: Option<String>,
}

/// home/origins.ts `hostOf`.
pub fn host_of(origin: &str) -> &str {
    origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")).unwrap_or(origin)
}

pub struct Props<'a> {
    pub account: Option<&'a RareAccount>,
    pub vault: Option<&'a Vault>,
    pub has_phrase: bool,
    /// games approved for this wallet (origins)
    pub origins: &'a [String],
    /// an account on this device keeps its key here (the device vault holds keys without a PIN vault)
    pub keys_here: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Open {
    Pin(ChangePin),
    Reveal(RevealWhat, Reveal),
    Hw(Option<LedgerCheck>, bool),
    Remove(Remove),
    RemoveAll(RemoveAll),
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub open: Option<Open>,
    pub auto: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    LockNow,
    Auto(u32),
    Open(&'static str),
    RemoveOrigin(String),
    Close,
    Pin(sheets::ChangeMsg),
    Reveal(sheets::RevealMsg),
    Hw(sheets::HelpMsg),
    Remove(sheets::RemoveMsg),
    RemoveAll(sheets::RemoveAllMsg),
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let has_vault = pr.vault.is_some_and(|v| v.exists);
    let kind = pr.vault.map(|v| v.kind).unwrap_or(SecretKind::Pin);
    let noun = if kind == SecretKind::Passphrase { "passphrase" } else { "PIN" };
    let has_key = pr.account.is_some_and(|a| a.kind == "native");
    let ic = |n: &'static str| icon(n, "").attr("size", "1.3rem");
    let mut sec = div("mt-2 divide-y divide-border-primary");
    if has_vault {
        sec = sec.kid(list_button(Some(ic("lock")), "Lock now", Some(text("Ask for your PIN next time")), None, Msg::LockNow, true, ""));
        sec = sec.kid(
            div("flex items-center gap-4 px-4 py-3")
                .kid(span("flex w-[1.3rem] shrink-0 justify-center text-icon-primary").kid(ic("plug")))
                .kid(span("min-w-0 flex-1").kid(span("block text-base font-medium").text("Auto-lock")).kid(span("block text-sm text-secondary").text("Lock after being idle")))
                .kid(segmented(
                    &[("0", "Off"), ("1", "1 min"), ("5", "5 min"), ("15", "15 min")],
                    &st.auto.to_string(),
                    |v| Msg::Auto(v.parse().unwrap_or(AUTOLOCK_DEFAULT)),
                    "",
                )),
        );
        sec = sec.kid(list_button(Some(ic("key-round")), &format!("Change {noun}"), None, None, Msg::Open("pin"), false, ""));
    }
    // A key in the device vault opens without a PIN: say "Needs your PIN" only when it does.
    let needs = has_vault.then(|| text(format!("Needs your {noun}")));
    if pr.has_phrase {
        sec = sec.kid(list_button(Some(ic("file-text")), "Show recovery phrase", needs.clone(), None, Msg::Open("phrase"), false, ""));
    }
    if has_key {
        sec = sec.kid(list_button(Some(ic("eye")), "Show private key", needs, None, Msg::Open("key"), false, ""));
    }
    let where_ = match pr.vault.and_then(|v| v.backend.as_deref()) {
        Some("keyring") => "Keys are stored in your system keyring.",
        Some("file") => "Keys are stored encrypted on this device.",
        _ if pr.keys_here => "Keys are stored encrypted on this device.",
        _ => "No keys are stored on this device yet.",
    };
    let security = panel(Some("Security"), None, "pb-2", vec![sec, p("px-5 pb-3 pt-3 text-sm text-secondary").text(where_)]);
    let mut games = div("mt-2 divide-y divide-border-primary");
    if pr.origins.is_empty() {
        games = games.kid(p("flex items-center gap-4 px-4 py-3.5 text-sm text-secondary").kid(ic("gamepad-2")).kid(text(" No games are connected to this wallet.")));
    }
    for o in pr.origins {
        games = games.kid(list_button(Some(ic("gamepad-2")), host_of(o), None, Some(text("Remove")), Msg::RemoveOrigin(o.clone()), false, "").key(o));
    }
    let connected = panel(Some("Connected games"), None, "pb-2", vec![games]);
    let hw = panel(Some("Hardware wallets"), None, "pb-2", vec![div("mt-2").kid(list_button(Some(ic("plug")), "Ledger and Keystone help", Some(text("Fix USB permission and check your device")), None, Msg::Open("hw"), false, ""))]);
    let mut rm = div("mt-2 divide-y divide-border-primary");
    if let Some(a) = pr.account {
        rm = rm.kid(list_button(Some(ic("trash-2")), "Remove this account", Some(text(&a.label)), None, Msg::Open("remove"), false, ""));
    }
    rm = rm.kid(list_button(Some(ic("shield-alert")), "Remove all wallets from this device", Some(text("Erases every account and key here")), None, Msg::Open("removeAll"), false, ""));
    let remove = panel(Some("Remove"), None, "pb-2", vec![rm]);

    let mut body = vec![div("grid grid-cols-2 items-start gap-6").kid(div("flex min-w-0 flex-col gap-6").kid(security).kid(connected)).kid(div("flex min-w-0 flex-col gap-6").kid(hw).kid(remove))];
    match &st.open {
        Some(Open::Pin(s)) => body.push(sheets::change_view(kind, s).map(&Msg::Pin)),
        Some(Open::Reveal(w, s)) => {
            if let Some(a) = pr.account {
                body.push(sheets::reveal_view(a, *w, kind, s).map(&Msg::Reveal));
            }
        }
        Some(Open::Hw(res, copied)) => body.push(sheets::hardware_help_view(res.as_ref(), *copied).map(&Msg::Hw)),
        Some(Open::Remove(s)) => {
            if let Some(a) = pr.account {
                body.push(sheets::remove_view(a, s).map(&Msg::Remove));
            }
        }
        Some(Open::RemoveAll(s)) => body.push(sheets::remove_all_view(s).map(&Msg::RemoveAll)),
        None => {}
    }
    page(Some("Settings"), pr.account.map(|a| text(&a.label)), None, false, "", body)
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Select"), hint("b", "Back")]
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// `keystore.lock()`, refresh, toast "Wallet locked"
    Lock,
    /// store the auto-lock minutes and re-arm the timer
    SetAutoLock(u32),
    RemoveOrigin(String),
    /// a sheet's own effect (unlock, reveal, change, remove, wipe, check Ledger, copy, keyboard)
    Sheet(Msg),
}

/// Opens, closes and auto-lock; everything a sheet does goes back to the host as `Effect::Sheet`.
pub fn update(st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::LockNow => Some(Effect::Lock),
        Msg::Auto(m) => {
            st.auto = m;
            Some(Effect::SetAutoLock(m))
        }
        Msg::Open(which) => {
            st.open = Some(match which {
                "pin" => Open::Pin(ChangePin::default()),
                "phrase" => Open::Reveal(RevealWhat::Phrase, Reveal::default()),
                "key" => Open::Reveal(RevealWhat::Key, Reveal::default()),
                "hw" => Open::Hw(None, false),
                "remove" => Open::Remove(Remove::default()),
                _ => Open::RemoveAll(RemoveAll::default()),
            });
            None
        }
        Msg::RemoveOrigin(o) => Some(Effect::RemoveOrigin(o)),
        Msg::Close
        | Msg::Pin(sheets::ChangeMsg::Close)
        | Msg::Reveal(sheets::RevealMsg::Close)
        | Msg::Hw(sheets::HelpMsg::Close)
        | Msg::Remove(sheets::RemoveMsg::Cancel)
        | Msg::RemoveAll(sheets::RemoveAllMsg::Cancel) => {
            // the revealed secret leaves memory with the sheet
            st.open = None;
            None
        }
        other => Some(Effect::Sheet(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings() {
        let mut a = RareAccount::new("0x1", "Main", "native");
        a.hd = Some(Default::default());
        let vault = Vault { exists: true, kind: SecretKind::Pin, backend: Some("keyring".into()) };
        let origins = vec!["https://wordgame.example.com".to_string()];
        let pr = Props { account: Some(&a), vault: Some(&vault), has_phrase: true, origins: &origins, keys_here: true };
        let mut st = State { open: None, auto: auto_lock(None) };
        let v = view(&pr, &st);
        assert_eq!(
            v.texts(),
            [
                "Settings", "Main", "Security", "Lock now", "Ask for your PIN next time", "Auto-lock", "Lock after being idle", "Off", "1 min", "5 min", "15 min", "Change PIN", "Show recovery phrase", "Needs your PIN", "Show private key", "Needs your PIN",
                "Keys are stored in your system keyring.", "Connected games", "wordgame.example.com", "Remove", "Hardware wallets", "Ledger and Keystone help", "Fix USB permission and check your device", "Remove", "Remove this account", "Main", "Remove all wallets from this device",
                "Erases every account and key here",
            ]
        );
        assert_eq!(v.landing().as_deref(), Some("Lock nowAsk for your PIN next time"));
        assert_eq!(update(&mut st, Msg::Auto(5)), Some(Effect::SetAutoLock(5)));
        update(&mut st, Msg::Open("phrase"));
        let v = view(&pr, &st);
        assert!(v.contains_text("Recovery phrase"));
        assert!(v.contains_text("Enter your PIN to continue"));
        update(&mut st, Msg::Reveal(sheets::RevealMsg::Close));
        assert!(st.open.is_none());
        let none = Props { account: None, vault: None, has_phrase: false, origins: &[], keys_here: false };
        let v = view(&none, &st);
        assert!(v.contains_text("No keys are stored on this device yet."));
        assert!(v.contains_text(" No games are connected to this wallet."));
        assert_eq!(auto_lock(Some("7")), 15);
        assert_eq!(auto_lock(Some("1")), 1);
    }
}
