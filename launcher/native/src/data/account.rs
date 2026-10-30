//! Home's account parts (screens/Home.tsx, sui/ui/HomeSui.tsx): the current Rare Wallet account,
//! its total value and today's change (rare-sui's portfolio over gRPC and the Trade API prices), and the
//! items it holds (rare.market). Read on a worker; nothing here when there is no account.
//!
//! The value and the in-game items come from the app-level wallet data (`data::wallet`), the same
//! the Wallet tab shows; the whole card is kept on disk (`keep`) so a new start draws the last one.

use crate::screens::wallet::live;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize)]
pub struct HomeItem {
    pub id: String,
    pub name: String,
    pub image: Option<String>,
    pub collection: Option<String>,
}

/// Items in your games (rare-market inventory summarize): the floor value, per game.
#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize)]
pub struct InGames {
    pub items: usize,
    pub offers: usize,
    /// "12.4" SUI, formatted.
    pub value: String,
    /// (name, logo, value formatted), most valuable first.
    pub games: Vec<(String, String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HomeAccount {
    pub address: String,
    pub label: String,
    /// The Rare username rare.market has for the address (None: none picked, or not answered).
    pub username: Option<String>,
    pub total_usd: Option<f64>,
    pub change_usd: Option<f64>,
    /// None until rare.market answered.
    pub items: Option<Vec<HomeItem>>,
    pub in_games: Option<InGames>,
}

impl std::hash::Hash for HomeAccount {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (&self.address, &self.label, &self.username, self.total_usd.map(f64::to_bits), self.change_usd.map(f64::to_bits), &self.items, &self.in_games).hash(h);
    }
}

/// A signed-in Home for shots (RR_ACCOUNT=1 with fixtures).
pub fn fixture() -> HomeAccount {
    let item = |id: &str, name: &str, col: &str| HomeItem { id: id.into(), name: name.into(), image: None, collection: Some(col.into()) };
    HomeAccount {
        address: "0x7a3f00000000000000000000000000000000000000000000000000000000c0de".into(),
        label: "rare.sui".into(),
        username: None,
        total_usd: Some(1284.52),
        change_usd: Some(-12.4),
        items: Some(vec![item("0x1", "Rareling #212", "Rarelings"), item("0x2", "Sword of Dawn", "ACT Items")]),
        in_games: Some(InGames { items: 7, offers: 1, value: "42.5".into(), games: vec![("ACT".into(), "games/act/logo.svg".into(), "40".into()), ("Word Game".into(), String::new(), "2.5".into())] }),
    }
}

impl HomeAccount {
    /// What the launcher calls the player: the Rare username, else the account name, else the
    /// short address.
    pub fn display_name(&self) -> String {
        let u = cached_username(&self.address).or_else(|| self.username.clone());
        display_name(u.as_deref(), &self.label, &self.address)
    }
}

/// The name the top bar and Home's card show: the Rare username (the last one picked on this
/// device or read), else the SuiNS name, else the wallet's account name, else the short address.
/// The account name comes from the wallet itself, so a rename shows at once.
pub fn shown_name(a: &HomeAccount, pl: Option<&crate::data::live::ProfileLive>) -> String {
    let pl = pl.filter(|p| p.address.eq_ignore_ascii_case(&a.address));
    let u = cached_username(&a.address).or_else(|| a.username.clone()).or_else(|| pl.and_then(|p| p.username.clone()));
    rare_sui::profile::display_name(u.as_deref(), pl.and_then(|p| p.suins.as_deref()), Some(&a.label), Some(&a.address))
}

/// The Rare username, else the account name (unless it is an address), else the short address.
pub fn display_name(username: Option<&str>, label: &str, address: &str) -> String {
    if let Some(u) = username.filter(|u| !u.is_empty()) {
        return u.to_string();
    }
    if !label.is_empty() && !label.starts_with("0x") {
        return label.to_string();
    }
    if address.len() > 10 {
        format!("{}...{}", &address[..6], &address[address.len() - 4..])
    } else {
        address.to_string()
    }
}

static USERNAMES: std::sync::Mutex<Vec<(String, Option<String>)>> = std::sync::Mutex::new(Vec::new());

/// Blocking: the Rare username rare.market has for `address` (`/v1/accounts/<address>`); the
/// answer is remembered for [`cached_username`].
pub fn username_of(address: &str) -> Option<String> {
    let v = rare_market::api::get::<serde_json::Value>(live::market_net(), &format!("/v1/accounts/{address}")).ok()?;
    let u = v.get("username").and_then(|u| u.as_str()).filter(|u| !u.is_empty()).map(str::to_string);
    remember_username(address, u.clone());
    u
}

/// A username learnt elsewhere (Friends after a pick, Settings' read).
pub fn remember_username(address: &str, username: Option<String>) {
    if let Ok(mut m) = USERNAMES.lock() {
        match m.iter_mut().find(|(a, _)| a.eq_ignore_ascii_case(address)) {
            Some(e) => e.1 = username,
            None => m.push((address.to_string(), username)),
        }
    }
}

/// The username last read for `address`, without asking (the Wallet header, the Rare menu).
pub fn cached_username(address: &str) -> Option<String> {
    USERNAMES.lock().ok()?.iter().find(|(a, _)| a.eq_ignore_ascii_case(address)).and_then(|(_, u)| u.clone())
}

/// The selected account (else the first), or None with no accounts.
pub fn current() -> Option<(String, String)> {
    let (accounts, selected) = crate::screens::wallet::page::load_accounts();
    current_of(&accounts, selected.as_deref())
}

/// `current` over a given list.
pub fn current_of(accounts: &[crate::screens::wallet::types::RareAccount], selected: Option<&str>) -> Option<(String, String)> {
    let a = selected.and_then(|s| accounts.iter().find(|a| a.address == s)).or(accounts.first())?;
    Some((a.address.clone(), a.label.clone()))
}

/// Home's account after the wallet changed: Some(new value) when the current account is not the
/// one Home shows (created, imported, signed in, picked or removed), so Home, the Rare menu and
/// Friends follow at once instead of on the next minute's read. The value and items come after.
pub fn follow(shown: Option<&HomeAccount>, cur: Option<(String, String)>) -> Option<Option<HomeAccount>> {
    let have = shown.map(|a| (a.address.as_str(), a.label.as_str()));
    if have == cur.as_ref().map(|(a, l)| (a.as_str(), l.as_str())) {
        return None;
    }
    Some(cur.map(|(address, label)| match shown {
        // a rename keeps what is loaded
        Some(a) if a.address == address => HomeAccount { label, ..a.clone() },
        _ => {
            let username = cached_username(&address);
            let mut a = HomeAccount { address, label, username, total_usd: None, change_usd: None, items: None, in_games: None };
            fill(&mut a);
            a
        }
    }))
}

/// Blocking: the account with its username and items (rare.market); its value and in-game items
/// from the app-level wallet data. A part that could not be read is None (see [`merge`]).
pub fn load() -> Option<HomeAccount> {
    let (address, label) = current()?;
    let mut acct = HomeAccount { address: address.clone(), label, username: username_of(&address), total_usd: None, change_usd: None, items: None, in_games: None };
    let path = rare_market::api::paths::account_items(&address).replace("limit=36", "limit=20");
    if let Ok(p) = rare_market::api::get::<rare_market::api::Page<rare_market::api::ItemCard>>(live::market_net(), &path) {
        acct.items = Some(p.data.into_iter().map(|i| HomeItem { id: i.id, name: i.name, image: i.image_url, collection: i.collection.map(|c| c.name) }).collect());
    }
    fill(&mut acct);
    Some(acct)
}

/// The value, today's change and the in-game items from the app-level wallet data, where it has them.
pub fn fill(acct: &mut HomeAccount) {
    let h = crate::data::wallet::get(&acct.address);
    if let Some(p) = &h.pf {
        acct.total_usd = p.total_usd.or(acct.total_usd);
        acct.change_usd = p.change_24h_usd.or(acct.change_usd);
    } else if acct.total_usd.is_none() {
        acct.total_usd = h.total;
    }
    if let Some(i) = &h.inv {
        let sui = |m: &str| rare_market::format::fmt_sui(Some(m), rare_market::format::SuiOpts { dp: 2, compact: false, trim: true });
        let s = rare_market::inventory::summarize(i);
        acct.in_games = Some(InGames { items: s.items, offers: s.offers, value: sui(&s.value_mist), games: s.games.iter().map(|g| (g.name.clone(), g.logo_url.clone(), sui(&g.value_mist))).collect() });
    }
}

/// A fresh read over what is shown: for the same account, a part the read could not get keeps
/// its last value instead of going blank.
pub fn merge(shown: Option<&HomeAccount>, new: Option<HomeAccount>) -> Option<HomeAccount> {
    let mut n = new?;
    if let Some(o) = shown.filter(|o| o.address == n.address) {
        n.username = n.username.or_else(|| o.username.clone());
        n.total_usd = n.total_usd.or(o.total_usd);
        n.change_usd = n.change_usd.or(o.change_usd);
        n.items = n.items.or_else(|| o.items.clone());
        n.in_games = n.in_games.or_else(|| o.in_games.clone());
    }
    Some(n)
}

const KEPT: &str = "home.account";

/// Home's account at start: the card from the last run when it is still the current account,
/// with whatever the wallet data holds on top.
pub fn boot() -> Option<HomeAccount> {
    // The account file directly: the wallet service starts later, off the UI thread.
    let (accounts, selected) = crate::screens::wallet::page::load_accounts_file();
    let (address, label) = current_of(&accounts, selected.as_deref())?;
    let mut a = crate::data::keep::load::<HomeAccount>(KEPT).filter(|a| a.address == address).unwrap_or_else(|| HomeAccount { username: cached_username(&address), address: address.clone(), label: label.clone(), total_usd: None, change_usd: None, items: None, in_games: None });
    a.label = label;
    if let Some(u) = &a.username {
        remember_username(&address, Some(u.clone()));
    }
    fill(&mut a);
    Some(a)
}

/// Keeps the card for the next start.
pub fn save(a: Option<&HomeAccount>) {
    if let Some(a) = a {
        crate::data::keep::save(KEPT, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::types::RareAccount;
    use rare_wallet::keystore::{Keystore, MemoryStore, PinOptions};
    use rare_wallet::vault::{Kdf, Vault};
    use std::sync::Arc;

    #[test]
    fn a_rename_shows_in_the_top_bar_at_once() {
        let addr = "0x00000000000000000000000000000000000000000000000000000000000a11ce";
        let a = HomeAccount { address: addr.into(), label: "New name".into(), username: None, total_usd: None, change_usd: None, items: None, in_games: None };
        // the profile read from before the rename still says the old name
        let pl = crate::data::live::ProfileLive { address: addr.into(), name: "Old name".into(), username: None, suins: None, avatar: None, banner: None, owned_games: None, total_usd: None, change_usd: None };
        assert_eq!(shown_name(&a, Some(&pl)), "New name");
        // a SuiNS name sits above the account name, a username above both
        let pl = crate::data::live::ProfileLive { suins: Some("me.sui".into()), ..pl };
        assert_eq!(shown_name(&a, Some(&pl)), "me.sui");
        remember_username(addr, Some("picked_now".into()));
        assert_eq!(shown_name(&a, Some(&pl)), "picked_now");
        assert_eq!(a.display_name(), "picked_now");
    }

    #[test]
    fn a_new_account_signs_home_in_at_once() {
        let dir = std::env::temp_dir().join(format!("rare-home-account-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let ks = Keystore::new(MemoryStore::default(), Arc::new(Vault::new(dir.join("w"), false, Kdf { m: 2048, t: 2, p: 1 })));
        // nothing on the device: Home is signed out and stays so
        let m = ks.meta().unwrap();
        let list: Vec<RareAccount> = m.accounts.iter().map(RareAccount::from_wallet).collect();
        assert_eq!(follow(None, current_of(&list, m.selected.as_deref())), None);
        // create: the keystore selects it, and Home follows on the next wallet change
        let acc = ks.add_phrase(None, None, Some(PinOptions { secret: "123456", kind: "pin" }), None).unwrap();
        let m = ks.meta().unwrap();
        let list: Vec<RareAccount> = m.accounts.iter().map(RareAccount::from_wallet).collect();
        let home = follow(None, current_of(&list, m.selected.as_deref())).expect("Home changes").expect("signed in");
        assert_eq!(home.address, acc.address);
        assert_eq!(home.label, acc.label);
        // nothing more until the account changes again
        assert_eq!(follow(Some(&home), current_of(&list, m.selected.as_deref())), None);
        // removing it signs Home out
        assert_eq!(follow(Some(&home), None), Some(None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
