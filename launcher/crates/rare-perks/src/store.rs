//! Rareling perks for the current account (launcher/src/perks/store.ts): one ownership check
//! (cached five minutes), the theme it picked, and a light check for other wallets (the "Rareling"
//! label in Top traders). Blocking: the host runs checks off the UI thread.

use crate::detect::{check_rareling, RarelingReader, RarelingResult};
use crate::themes::{theme_by_id, theme_key};
use std::collections::{HashMap, HashSet};

const TTL_MS: i64 = 5 * 60_000;

/// Device settings (themes per account); the host keeps them where it keeps the rest.
pub trait Kv: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
}

#[derive(Default)]
pub struct MemKv(std::sync::Mutex<HashMap<String, String>>);

impl Kv for MemKv {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: &str) {
        self.0.lock().unwrap().insert(key.into(), value.into());
    }
}

pub fn read_theme(kv: &dyn Kv, account: Option<&str>) -> String {
    match account {
        Some(a) => theme_by_id(kv.get(&theme_key(a)).as_deref()).id.to_string(),
        None => "rare".into(),
    }
}

#[derive(Debug, Default)]
pub struct Perks {
    pub address: Option<String>,
    pub result: Option<RarelingResult>,
    pub at_ms: i64,
    pub theme: String,
    others: HashMap<String, (i64, bool)>,
    /// Rareling holders among the wallets scanned (Top traders).
    pub held: HashSet<String>,
}

impl Perks {
    pub fn new() -> Self {
        Perks { theme: "rare".into(), ..Default::default() }
    }

    /// Checks `address` (lowercased) unless a fresh answer is cached. Returns the theme the shell
    /// should show (None: the default tokens): a theme is a perk, shown only while the account
    /// holds a Rareling.
    pub fn check(&mut self, address: Option<&str>, force: bool, rareling_type: Option<&str>, read: &dyn RarelingReader, kv: &dyn Kv, now_ms: i64) -> Option<String> {
        let Some(a) = address.map(|a| a.to_lowercase()) else {
            self.address = None;
            self.result = Some(if rareling_type.is_some() { RarelingResult::None } else { RarelingResult::Unavailable });
            self.theme = "rare".into();
            return None;
        };
        if !force && self.address.as_deref() == Some(&a) && self.result.is_some() && now_ms - self.at_ms < TTL_MS {
            return self.applied();
        }
        if self.address.as_deref() != Some(&a) {
            self.address = Some(a.clone());
            self.result = None;
            self.theme = read_theme(kv, Some(&a));
        }
        let result = check_rareling(&a, rareling_type, read).unwrap_or(RarelingResult::None);
        self.result = Some(result);
        self.at_ms = now_ms;
        self.applied()
    }

    fn applied(&self) -> Option<String> {
        self.holds().then(|| self.theme.clone())
    }

    pub fn holds(&self) -> bool {
        self.result.as_ref().is_some_and(|r| r.owned())
    }

    /// Picks a theme (holders only). Returns the theme to show, if it changed.
    pub fn set_theme(&mut self, id: &str, kv: &dyn Kv) -> Option<String> {
        let a = self.address.clone()?;
        if !self.holds() {
            return None;
        }
        let t = theme_by_id(Some(id)).id;
        kv.set(&theme_key(&a), t);
        self.theme = t.into();
        Some(self.theme.clone())
    }

    /// Whether a wallet holds a Rareling; false while no type is set. Cached ten minutes.
    pub fn wallet_holds(&mut self, address: &str, rareling_type: Option<&str>, read: &dyn RarelingReader, now_ms: i64) -> bool {
        let Some(t) = rareling_type else { return false };
        let a = address.to_lowercase();
        if let Some((at, owned)) = self.others.get(&a) {
            if now_ms - at < 2 * TTL_MS {
                return *owned;
            }
        }
        let owned = check_rareling(&a, Some(t), read).map(|r| r.owned()).unwrap_or(false);
        self.others.insert(a, (now_ms, owned));
        owned
    }

    /// Rareling holders among `addresses` (into [`Perks::held`]).
    pub fn scan(&mut self, addresses: &[&str], rareling_type: Option<&str>, read: &dyn RarelingReader, now_ms: i64) {
        if rareling_type.is_none() {
            return;
        }
        let mut todo: Vec<String> = Vec::new();
        for a in addresses.iter().map(|a| a.to_lowercase()) {
            if !todo.contains(&a) {
                todo.push(a);
            }
        }
        for a in todo {
            if self.wallet_holds(&a, rareling_type, read, now_ms) {
                self.held.insert(a);
            } else {
                self.held.remove(&a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::Owned;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Holder(AtomicUsize);
    impl RarelingReader for Holder {
        fn owned(&self, owner: &str, t: &str) -> Result<Vec<Owned>, String> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(if owner == "0xyes" && t.ends_with("::rareling::Rareling") { vec![Owned { object_id: "0xr".into(), json: None }] } else { vec![] })
        }
        fn kiosk_items(&self, _: &str) -> Result<Vec<(String, String)>, String> {
            Ok(vec![])
        }
    }

    const T: &str = "0x7::rareling::Rareling";

    #[test]
    #[cfg(not(feature = "rarelings"))]
    fn off_means_nothing_is_checked() {
        let r = Holder(AtomicUsize::new(0));
        let mut p = Perks::new();
        assert!(!p.wallet_holds("0xyes", crate::rareling_type(None).as_deref(), &r, 0));
        assert_eq!(r.0.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn theme_only_while_holding_and_cached() {
        let (r, kv) = (Holder(AtomicUsize::new(0)), MemKv::default());
        let mut p = Perks::new();
        assert_eq!(p.check(Some("0xYES"), false, Some(T), &r, &kv, 0).as_deref(), Some("rare"));
        assert_eq!(p.set_theme("royal", &kv).as_deref(), Some("royal"));
        let n = r.0.load(Ordering::Relaxed);
        assert_eq!(p.check(Some("0xyes"), false, Some(T), &r, &kv, 1000).as_deref(), Some("royal"));
        assert_eq!(r.0.load(Ordering::Relaxed), n);
        assert_eq!(p.check(Some("0xno"), false, Some(T), &r, &kv, 2000), None);
        assert!(p.set_theme("royal", &kv).is_none());
        // The theme comes back with the account.
        assert_eq!(p.check(Some("0xyes"), false, Some(T), &r, &kv, 3000).as_deref(), Some("royal"));
        assert_eq!(p.check(None, false, Some(T), &r, &kv, 4000), None);
        assert_eq!(p.result, Some(RarelingResult::None));
    }

    #[test]
    fn scans_trader_wallets() {
        let r = Holder(AtomicUsize::new(0));
        let mut p = Perks::new();
        p.scan(&["0xYES", "0xno", "0xyes"], Some(T), &r, 0);
        assert_eq!(p.held.iter().collect::<Vec<_>>(), [&"0xyes".to_string()]);
        p.scan(&[], None, &r, 0);
        assert_eq!(p.held.len(), 1);
    }
}
