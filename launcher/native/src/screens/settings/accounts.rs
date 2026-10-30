//! The Accounts section (screens/settings/AccountsStorage.tsx): username, Rare account, wallet,
//! Epic Games and GOG (features/stores.rs), setup.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_render::Id;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct ProfileInfo {
    /// None: signed out, no username row.
    pub address: Option<String>,
    pub username: Option<String>,
    pub name: Option<String>,
    /// What the wallet calls the signed-in account ("Demo wallet", a SuiNS name).
    pub label: Option<String>,
    /// The Steam account signed in on this device (its profile name).
    pub steam: Option<String>,
}

/// The Username row's line (UsernameRow).
pub fn username_body(p: &ProfileInfo) -> String {
    match &p.username {
        Some(u) => format!("{u}{}. You can change it once every 30 days.", p.name.as_ref().map(|n| format!(", SuiNS {n}")).unwrap_or_default()),
        None => "Friends find you by it. Free, and nobody needs a SuiNS name.".into(),
    }
}

/// The Rare account row's line: who is signed in, or what signing in gives.
pub fn rare_body(p: &ProfileInfo) -> String {
    match &p.address {
        Some(a) => {
            let who = p.username.clone().or_else(|| p.name.clone()).or_else(|| p.label.clone()).unwrap_or_else(|| short(a));
            format!("Signed in as {who}. One sign-in for Rare Wallet and Rare Marketplace.")
        }
        None => "One sign-in for Rare Wallet and Rare Marketplace. Your games work without an account.".into(),
    }
}

/// The Steam row's line.
pub fn steam_body(p: &ProfileInfo) -> String {
    match &p.steam {
        Some(s) => format!("Signed in as {s}. Sign out or switch accounts in Steam's settings."),
        None => "Sign in to see your Steam games in Library and start them from Rare.".into(),
    }
}

/// One wallet account's row: what it is and its rare.market sign-in on this device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountLine {
    pub address: String,
    pub label: String,
    /// "Recovery phrase on this SuiPlay, signs with A", "Passkey", ...
    pub kind: String,
    /// Signed in to rare.market on this device: since (ms, 0 when not known).
    pub session: Option<i64>,
    pub main: bool,
    /// It can sign (not watch only).
    pub can_sign: bool,
    /// Its key is in the device vault (the send PIN applies to it).
    pub device: bool,
}

/// What kind of account it is and how it signs, in a few words.
pub fn kind_text(a: &crate::screens::wallet::types::RareAccount) -> String {
    match a.kind.as_str() {
        "native" if a.device && a.hd.is_some() => "Recovery phrase on this SuiPlay, signs with A",
        "native" if a.device => "Private key on this SuiPlay, signs with A",
        "native" if a.hd.is_some() => "Recovery phrase, signs with your PIN",
        "native" => "Private key, signs with your PIN",
        "passkey" if a.passkey.as_ref().is_some_and(|p| p.pin) => "PIN account, signs here with your PIN, your phone or key is the backup",
        "passkey" => "Passkey, signs with your phone or PIN",
        "ledger" => "Ledger",
        "keystone" => "Keystone",
        "watch" => "Watch only, cannot sign",
        _ => "Account",
    }
    .into()
}

/// The device's accounts with their rare.market sign-ins (the live launcher).
pub fn account_lines() -> Vec<AccountLine> {
    let (list, selected) = crate::screens::wallet::page::load_accounts();
    let main = selected.clone().or_else(|| list.first().map(|a| a.address.clone()));
    let keys = rare_friends::storage::FileStorage::new(rare_core::paths::data_dir().join("friends"));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    list.iter()
        .map(|a| AccountLine {
            address: a.address.clone(),
            label: a.label.clone(),
            kind: kind_text(a),
            session: rare_friends::sign::session_of(&keys, &a.address).filter(|s| s.live(now)).map(|s| s.linked_at),
            main: main.as_deref() == Some(a.address.as_str()),
            can_sign: a.kind != "watch",
            device: a.device,
        })
        .collect()
}

/// Screenshots: three accounts (a phrase one signed in, a passkey one, a watch one).
pub fn fixture_lines() -> Vec<AccountLine> {
    vec![
        AccountLine { address: "0x7d20dcdb2bca4f50".into(), label: "Account 1".into(), kind: "Recovery phrase on this SuiPlay, signs with A".into(), session: Some(1_793_000_000_000), main: true, can_sign: true, device: true },
        AccountLine { address: "0x7bba19bc168f6774".into(), label: "trade".into(), kind: "Passkey, signs with your phone or PIN".into(), session: None, main: false, can_sign: true, device: false },
        AccountLine { address: "0x1111222233334444".into(), label: "A very long account name that keeps going on".into(), kind: "Watch only, cannot sign".into(), session: None, main: false, can_sign: false, device: false },
    ]
}

/// "Signed in to rare.market since Oct 27, 2026" or why not.
pub fn session_text(l: &AccountLine) -> String {
    let mut s = l.kind.clone();
    s.push_str(". ");
    match l.session {
        Some(0) => s.push_str("Signed in to rare.market."),
        Some(at) => s.push_str(&format!("Signed in to rare.market since {}.", date_of(at))),
        None if l.can_sign => s.push_str("Not signed in to rare.market."),
        None => s.pop().map(|_| ()).unwrap_or(()),
    }
    s.trim_end().to_string()
}

/// "Oct 27, 2026" from ms since the epoch (UTC).
pub fn date_of(ms: i64) -> String {
    let iso = crate::screens::wallet::service::iso_of(ms);
    let (y, m, d) = (&iso[0..4], iso[5..7].parse::<usize>().unwrap_or(1), iso[8..10].trim_start_matches('0'));
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {d}, {y}", M[(m - 1).min(11)])
}

/// The send PIN row's line.
pub fn send_pin_body(threshold: Option<f64>) -> String {
    match threshold {
        Some(t) => format!("On. Sends over ${t:.0} ask for it. Everything else signs with A."),
        None => format!("Off. Accounts on this SuiPlay sign with A alone. Turn it on to ask for a PIN before sends over ${:.0}.", rare_wallet::send_pin::DEFAULT_THRESHOLD_USD),
    }
}

fn short(a: &str) -> String {
    if a.len() > 12 {
        format!("{}...{}", &a[..6], &a[a.len() - 4..])
    } else {
        a.to_string()
    }
}

pub struct AccountsSection {
    /// The account rows' addresses, by row (tags 100 + 3 * row + {0 sign out, 1 make main, 2 sign in}).
    addrs: Vec<String>,
}

const T_ACCOUNT: u32 = 100;
const T_PIN_SET: u32 = 90;
const T_PIN_OFF: u32 = 91;
const T_DEV_PIN: u32 = 92;
const T_DEV_WORDS: u32 = 93;

/// The PIN accounts' row.
pub const DEVICE_PIN_BODY: &str = "PIN accounts sign on this SuiPlay with it. 5 wrong PINs lock it until your phone or security key unlocks it.";

impl AccountsSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> AccountsSection {
        let mut prof = super::live::get().map(|s| s.profile).unwrap_or_default();
        if super::live::enabled() {
            // Read the account and Steam now: signing in or out elsewhere shows here at once.
            let fresh = super::live::read_profile();
            if fresh.address != prof.address {
                prof.username = None;
                prof.name = None;
            }
            prof.address = fresh.address;
            prof.label = fresh.label;
            prof.steam = fresh.steam;
        }
        AccountsSection::build_with(cx, nav, parent, cfg, &prof, now)
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, prof: &ProfileInfo, now: f64) -> AccountsSection {
        let (lines, pin) = if super::live::enabled() {
            let pin = crate::screens::wallet::service::wallet().and_then(|w| w.send_pin.threshold_usd());
            (account_lines(), pin)
        } else {
            (fixture_lines(), None)
        };
        Self::build_lines(cx, nav, parent, cfg, prof, &lines, pin, now)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_lines(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, prof: &ProfileInfo, lines: &[AccountLine], pin: Option<f64>, _now: f64) -> AccountsSection {
        let p = panel(cx, parent);
        let signed_in = prof.address.is_some();
        let rows: Vec<(&str, &str, String, &str, Variant, u32)> = vec![
            ("rare-account", "Rare account", rare_body(prof), if signed_in { "Sign out" } else { "Sign in" }, if signed_in { Variant::Secondary } else { Variant::Primary }, if signed_in { 5 } else { 2 }),
            ("steam", "Steam", steam_body(prof), if prof.steam.is_some() { "Open Steam" } else { "Sign in" }, if prof.steam.is_some() { Variant::Secondary } else { Variant::Primary }, 6),
            ("wallet", "Rare Wallet", "Balances, collectibles and moving assets from your old SuiPlay wallet.".into(), "Open", Variant::Secondary, 3),
            ("setup", "Setup", "Go through the first-run steps again: your account, moving assets, Steam and favourites.".into(), "Run setup", Variant::Secondary, 4),
        ];
        let mut first = true;
        for (i, (sid, title, body, label, v, tag)) in rows.into_iter().enumerate() {
            let r = setting_row(cx, p, title, Some(&body), first);
            first = false;
            let b = button(cx, r.right, label, v, Sz::Md, false).sid(sid).tag(tag);
            nav.add(b.group(10));
            // Epic Games and GOG after Steam (features/stores.rs).
            if sid == "steam" {
                for (j, (store, line, signed)) in crate::features::stores::account_rows().into_iter().enumerate() {
                    let r = setting_row(cx, p, store.name(), Some(&line), false);
                    let b = button(cx, r.right, if signed { "Sign out" } else { "Sign in" }, if signed { Variant::Secondary } else { Variant::Primary }, Sz::Md, false).sid(store.key()).tag(7 + j as u32);
                    nav.add(b.group(10));
                }
            }
            // The username sits under the Rare account it belongs to.
            if i == 0 && signed_in {
                let r = setting_row(cx, p, "Username", Some(&username_body(prof)), false);
                let b = button(cx, r.right, if prof.username.is_some() { "Change" } else { "Pick a username" }, if prof.username.is_some() { Variant::Secondary } else { Variant::Primary }, Sz::Md, false).sid("username").tag(1);
                nav.add(b.group(10));
            }
        }
        // Every account on this device: what it is, its rare.market sign-in, sign out, main.
        let mut addrs = Vec::new();
        if !lines.is_empty() {
            let p = panel(cx, parent);
            let mut first = true;
            for (i, l) in lines.iter().enumerate() {
                let title = if l.main { format!("{} (main)", l.label) } else { l.label.clone() };
                let r = setting_row(cx, p, &title, Some(&session_text(l)), first);
                first = false;
                let base = T_ACCOUNT + 3 * i as u32;
                if l.session.is_some() {
                    let b = button(cx, r.right, "Sign out", Variant::Secondary, Sz::Md, false).sid(&format!("acct-out-{i}")).tag(base);
                    nav.add(b.group(11));
                } else if l.main && l.can_sign {
                    let b = button(cx, r.right, "Sign in", Variant::Primary, Sz::Md, false).sid(&format!("acct-in-{i}")).tag(base + 2);
                    nav.add(b.group(11));
                }
                if !l.main {
                    let b = button(cx, r.right, "Make this the main account", Variant::Secondary, Sz::Md, false).sid(&format!("acct-main-{i}")).tag(base + 1);
                    nav.add(b.group(11));
                }
                addrs.push(l.address.clone());
            }
            let device_pin = if super::live::enabled() { crate::screens::wallet::service::has_device_pin() } else { true };
            if device_pin {
                let r = setting_row(cx, p, "PIN on this SuiPlay", Some(DEVICE_PIN_BODY), false);
                let b = button(cx, r.right, "Change PIN", Variant::Secondary, Sz::Md, false).sid("device-pin").tag(T_DEV_PIN);
                nav.add(b.group(11));
                let b = button(cx, r.right, "Recovery words", Variant::Secondary, Sz::Md, false).sid("device-words").tag(T_DEV_WORDS);
                nav.add(b.group(11));
            }
            if lines.iter().any(|l| l.device) || !super::live::enabled() {
                let r = setting_row(cx, p, "PIN for big sends", Some(&send_pin_body(pin)), false);
                let b = if pin.is_some() {
                    button(cx, r.right, "Turn off", Variant::Secondary, Sz::Md, false).sid("send-pin-off").tag(T_PIN_OFF)
                } else {
                    button(cx, r.right, "Set a PIN", Variant::Secondary, Sz::Md, false).sid("send-pin-set").tag(T_PIN_SET)
                };
                nav.add(b.group(11));
            }
        }
        AccountsSection { addrs }
    }
}

impl SectionView for AccountsSection {
    fn press(&mut self, _cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
        match nav.current().map(|f| f.tag) {
            Some(1) => SectionOut::Act("accounts:pick-username".into()),
            Some(2) => SectionOut::Act("open:signin".into()),
            Some(3) => SectionOut::Act("tab:wallet".into()),
            Some(4) => SectionOut::Act("open:firstrun".into()),
            Some(5) => SectionOut::Act("accounts:sign-out".into()),
            Some(6) => SectionOut::Act("accounts:steam".into()),
            Some(t @ (7 | 8)) => {
                let store = if t == 7 { rare_stores::Store::Epic } else { rare_stores::Store::Gog };
                SectionOut::Act(format!("stores:{}:{}", if store.signed_in() { "signout" } else { "signin" }, store.key()))
            }
            Some(T_PIN_SET) => SectionOut::Act("accounts:send-pin:set".into()),
            Some(T_PIN_OFF) => SectionOut::Act("accounts:send-pin:off".into()),
            Some(T_DEV_PIN) => SectionOut::Act("accounts:pin:change".into()),
            Some(T_DEV_WORDS) => SectionOut::Act("accounts:pin:words".into()),
            Some(t) if t >= T_ACCOUNT => {
                let (row, what) = (((t - T_ACCOUNT) / 3) as usize, (t - T_ACCOUNT) % 3);
                let Some(a) = self.addrs.get(row) else { return SectionOut::None };
                SectionOut::Act(match what {
                    0 => format!("accounts:signout:{a}"),
                    1 => format!("accounts:main:{a}"),
                    _ => "accounts:signin".into(),
                })
            }
            _ => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_username_line() {
        assert_eq!(username_body(&ProfileInfo::default()), "Friends find you by it. Free, and nobody needs a SuiNS name.");
        let p = ProfileInfo { address: Some("0x1".into()), username: Some("tyler".into()), ..Default::default() };
        assert_eq!(username_body(&p), "tyler. You can change it once every 30 days.");
        let p = ProfileInfo { name: Some("tyler.sui".into()), ..p };
        assert_eq!(username_body(&p), "tyler, SuiNS tyler.sui. You can change it once every 30 days.");
        assert_eq!(rare_body(&p), "Signed in as tyler. One sign-in for Rare Wallet and Rare Marketplace.");
        assert_eq!(rare_body(&ProfileInfo::default()), "One sign-in for Rare Wallet and Rare Marketplace. Your games work without an account.");
        let s = ProfileInfo { steam: Some("playerone".into()), ..Default::default() };
        assert_eq!(steam_body(&s), "Signed in as playerone. Sign out or switch accounts in Steam's settings.");
    }
}
