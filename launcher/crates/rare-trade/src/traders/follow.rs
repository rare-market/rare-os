//! Followed traders, kept on this device for each Rare Wallet account (no account: one device
//! list). Nothing leaves the device: the follow list is local, the feed polls public trades.

use crate::kv::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Followed {
    pub address: String,
    pub name: String,
    /// when the follow started (ms); trades before it never notify
    pub since: f64,
}

/// The most traders one account follows, so the feed stays a handful of light reads.
pub const MAX_FOLLOWS: usize = 30;

pub fn follow_key(account: Option<&str>) -> String {
    format!("rare.traders.follow.{}", account.map(str::to_lowercase).unwrap_or_else(|| "device".into()))
}

fn long_lower_address(a: &str) -> bool {
    a.len() == 66 && a.starts_with("0x") && a[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The list kept for `account`; junk in storage is skipped.
pub fn read_follows(account: Option<&str>, kv: &dyn Kv) -> Vec<Followed> {
    let Some(raw) = kv.get(&follow_key(account)) else { return Vec::new() };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&raw) else { return Vec::new() };
    items
        .iter()
        .filter_map(|f| {
            let address = f.get("address")?.as_str().filter(|a| long_lower_address(a))?;
            Some(Followed { address: address.to_string(), name: f.get("name").and_then(Value::as_str).unwrap_or_default().to_string(), since: f.get("since").and_then(Value::as_f64).unwrap_or(0.0) })
        })
        .take(MAX_FOLLOWS)
        .collect()
}

pub fn write_follows(account: Option<&str>, list: &[Followed], kv: &dyn Kv) {
    if let Ok(s) = serde_json::to_string(list) {
        kv.set(&follow_key(account), &s);
    }
}

/// Follows or unfollows; the newest follow first; never more than MAX_FOLLOWS.
pub fn toggle_follow(list: &[Followed], address: &str, name: &str, now_ms: f64) -> Vec<Followed> {
    let a = address.to_lowercase();
    if list.iter().any(|f| f.address == a) {
        return list.iter().filter(|f| f.address != a).cloned().collect();
    }
    let mut out = vec![Followed { address: a, name: name.to_string(), since: now_ms }];
    out.extend(list.iter().cloned());
    out.truncate(MAX_FOLLOWS);
    out
}

pub fn is_following(list: &[Followed], address: &str) -> bool {
    let a = address.to_lowercase();
    list.iter().any(|f| f.address == a)
}

/// The follow list for the current account (the web launcher's `useFollows`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Follows {
    pub account: Option<String>,
    pub list: Vec<Followed>,
    loaded: bool,
}

impl Follows {
    /// Reads the list for this account (the feed and the screens call it as the account changes).
    pub fn use_account(&mut self, account: Option<&str>, kv: &dyn Kv) {
        let a = account.map(str::to_lowercase);
        if self.loaded && a == self.account {
            return;
        }
        self.list = read_follows(a.as_deref(), kv);
        self.account = a;
        self.loaded = true;
    }

    pub fn toggle(&mut self, address: &str, name: &str, now_ms: f64, kv: &dyn Kv) {
        self.list = toggle_follow(&self.list, address, name, now_ms);
        write_follows(self.account.as_deref(), &self.list, kv);
    }
}

#[cfg(test)]
mod tests {
    //! follow.test.ts
    use super::*;
    use crate::kv::MemKv;

    fn a() -> String {
        format!("0x{}", "a".repeat(64))
    }
    fn b() -> String {
        format!("0x{}", "B".repeat(64))
    }

    #[test]
    fn toggles_newest_first_addresses_lower_case() {
        let mut l = toggle_follow(&[], &a(), "a", 1.0);
        l = toggle_follow(&l, &b(), "b", 2.0);
        assert_eq!(l.iter().map(|f| f.address.clone()).collect::<Vec<_>>(), [b().to_lowercase(), a()]);
        assert!(is_following(&l, &b()));
        l = toggle_follow(&l, &b().to_lowercase(), "b", 3.0);
        assert_eq!(l.iter().map(|f| f.address.clone()).collect::<Vec<_>>(), [a()]);
    }

    #[test]
    fn caps_the_list() {
        let mut l = Vec::new();
        for i in 0..MAX_FOLLOWS + 5 {
            l = toggle_follow(&l, &format!("0x{i:064x}"), &i.to_string(), 0.0);
        }
        assert_eq!(l.len(), MAX_FOLLOWS);
        assert_eq!(l[0].name, (MAX_FOLLOWS + 4).to_string());
    }

    #[test]
    fn keeps_one_list_per_account_on_the_device() {
        let kv = MemKv::new();
        write_follows(Some(&a()), &[Followed { address: b().to_lowercase(), name: "b".into(), since: 1.0 }], &kv);
        write_follows(None, &[Followed { address: a(), name: "a".into(), since: 1.0 }], &kv);
        let upper = a().to_uppercase().replace("0X", "0x");
        assert_eq!(read_follows(Some(&upper), &kv).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["b"]);
        assert_eq!(read_follows(None, &kv).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a"]);
        assert!(read_follows(Some(&b()), &kv).is_empty());
        assert_eq!(follow_key(None), "rare.traders.follow.device");
    }

    #[test]
    fn ignores_junk_in_storage() {
        let kv = MemKv::new();
        kv.set(&follow_key(Some(&a())), &serde_json::json!([{ "address": "nope" }, null, { "address": a(), "name": "ok", "since": 0 }]).to_string());
        assert_eq!(read_follows(Some(&a()), &kv).iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["ok"]);
        kv.set(&follow_key(Some(&a())), "{broken");
        assert!(read_follows(Some(&a()), &kv).is_empty());
    }

    #[test]
    fn the_store_follows_the_account() {
        let kv = MemKv::new();
        let mut f = Follows::default();
        f.use_account(Some(&a()), &kv);
        f.toggle(&b(), "b", 5.0, &kv);
        f.use_account(None, &kv);
        assert!(f.list.is_empty());
        f.use_account(Some(&a().to_uppercase().replace("0X", "0x")), &kv);
        assert_eq!(f.list.len(), 1);
    }
}
