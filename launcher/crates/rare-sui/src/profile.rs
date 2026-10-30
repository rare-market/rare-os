//! The player's Sui profile: SuiNS name, the NFT they picked as their picture, SUI balance and
//! portfolio value. Ports `src/sui/profile.ts`.
//!
//! Name and balance come from Sui gRPC; collectibles, their value and the SUI price from
//! rare.market. The picture choice is local to this device, per address, through [`Persist`].

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::grpc::ChainRead;
use crate::ids::SUI_TYPE_SHORT;
use crate::market::{self, ItemCard, MarketApi};

/// Small key-value storage the host provides (rare-state in the launcher; localStorage in the TS).
pub trait Persist: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn put(&self, key: &str, value: String);
    fn remove(&self, key: &str);
}

/// In-memory [`Persist`] for tests and a session without storage.
#[derive(Default)]
pub struct MemPersist(Mutex<std::collections::HashMap<String, String>>);

impl Persist for MemPersist {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }
    fn put(&self, key: &str, value: String) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), value);
    }
    fn remove(&self, key: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPick {
    pub id: String,
    pub image_url: String,
    pub name: String,
    /// When it was picked (epoch ms; 0 for picks saved before this was kept). The newest pick wins
    /// between this device and rare.market.
    #[serde(default)]
    pub at: i64,
}

/// Now, epoch ms.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The signed-in (or watched) account: what the wallet's keystore answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Current {
    pub address: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileState {
    pub address: Option<String>,
    pub label: Option<String>,
    /// The default SuiNS name.
    pub name: Option<String>,
    /// The Rare username the player picked.
    pub username: Option<String>,
    pub avatar: Option<AvatarPick>,
    /// rare.market's own avatar for the account, used when nothing is picked.
    pub market_avatar: Option<String>,
    /// rare.market's banner for the account (1500 x 500), its pick or its default.
    pub banner: Option<String>,
    pub sui_mist: Option<u64>,
    pub nft_mist: Option<u128>,
    pub sui_usd: Option<f64>,
    pub loading: bool,
    pub at_ms: f64,
}

fn avatar_key(a: &str) -> String {
    format!("rare.sui.avatar.{a}")
}

fn read_pick(p: &dyn Persist, address: &str) -> Option<AvatarPick> {
    serde_json::from_str(&p.get(&avatar_key(address))?).ok()
}

/// Portfolio total in MIST (balance plus collectibles at rare.market's value), None before the
/// first read.
pub fn total_mist(p: &ProfileState) -> Option<u128> {
    if p.sui_mist.is_none() && p.nft_mist.is_none() {
        return None;
    }
    Some(u128::from(p.sui_mist.unwrap_or(0)) + p.nft_mist.unwrap_or(0))
}

/// What the top bar and profile call the player: Rare username, else SuiNS name, else the wallet
/// label, else the short address.
pub fn display_name(username: Option<&str>, name: Option<&str>, label: Option<&str>, address: Option<&str>) -> String {
    if let Some(u) = username.filter(|s| !s.is_empty()) {
        return u.to_string();
    }
    if let Some(n) = name.filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    if let Some(l) = label.filter(|l| !l.is_empty() && !l.starts_with("0x")) {
        return l.to_string();
    }
    match address {
        Some(a) if a.len() > 10 => format!("{}...{}", &a[..6], &a[a.len() - 4..]),
        Some(a) => a.to_string(),
        None => String::new(),
    }
}

impl ProfileState {
    pub fn display_name(&self) -> String {
        display_name(self.username.as_deref(), self.name.as_deref(), self.label.as_deref(), self.address.as_deref())
    }
}

/// The profile store (`useProfile`).
#[derive(Default)]
pub struct ProfileStore {
    state: Mutex<ProfileState>,
}

impl ProfileStore {
    pub fn snapshot(&self) -> ProfileState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn with<R>(&self, f: impl FnOnce(&mut ProfileState) -> R) -> R {
        f(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Reads the profile for `current` (None: signed out) unless it is under a minute old.
    pub async fn refresh(&self, chain: &dyn ChainRead, api: &dyn MarketApi, persist: &dyn Persist, current: Option<Current>, force: bool, now_ms: f64) {
        let Some(cur) = current else {
            self.with(|s| {
                let usd = s.sui_usd;
                *s = ProfileState { sui_usd: usd, at_ms: now_ms, ..Default::default() };
            });
            return;
        };
        let go = self.with(|s| {
            if s.loading || (!force && s.address.as_deref() == Some(&cur.address) && now_ms - s.at_ms < 60_000.0) {
                return false;
            }
            let changed = s.address.as_deref() != Some(&cur.address);
            s.loading = true;
            s.address = Some(cur.address.clone());
            s.label = cur.label.clone();
            s.avatar = read_pick(persist, &cur.address);
            if changed {
                s.name = None;
                s.username = None;
                s.market_avatar = None;
                s.banner = None;
                s.sui_mist = None;
                s.nft_mist = None;
            }
            true
        });
        if !go {
            return;
        }
        let a = cur.address.as_str();
        let (name, bal, acct, port, status) = futures::join!(
            chain.default_name(a),
            chain.balance(a, SUI_TYPE_SHORT),
            market::account(api, a),
            market::portfolio(api, a),
            market::status(api),
        );
        self.with(|s| {
            if s.address.as_deref() != Some(a) {
                s.loading = false;
                return;
            }
            s.name = name.ok().flatten();
            s.username = acct.as_ref().ok().and_then(|x| x.username.clone());
            s.sui_mist = bal.ok().map(|b| b.balance);
            // A newer pick on rare.market (made on the site) replaces this device's.
            if let Some(av) = acct.as_ref().ok().and_then(|x| x.avatar.clone()) {
                let at = av.at.unwrap_or(0.0) as i64;
                if at > s.avatar.as_ref().map_or(0, |p| p.at) && !av.url.is_empty() {
                    let pick = AvatarPick { id: av.id, image_url: av.url, name: String::new(), at };
                    persist.put(&avatar_key(a), serde_json::to_string(&pick).unwrap_or_default());
                    s.avatar = Some(pick);
                }
            }
            if let Some(b) = acct.as_ref().ok().and_then(|x| x.banner.as_ref()).filter(|b| !b.url.is_empty()) {
                s.banner = Some(b.url.clone());
            }
            s.market_avatar = acct.ok().and_then(|x| x.avatar_url);
            s.nft_mist = port.ok().and_then(|p| p.value_mist.parse().ok());
            s.sui_usd = status.ok().map(|x| x.sui_usd).or(s.sui_usd);
            s.loading = false;
            s.at_ms = now_ms;
        });
    }

    /// Reads the picture choice back from storage (the launcher saved it there itself).
    pub fn reload_pick(&self, persist: &dyn Persist) {
        self.with(|s| {
            if let Some(a) = s.address.clone() {
                s.avatar = read_pick(persist, &a);
            }
        });
    }

    /// The picture choice (None clears it), kept per address on this device.
    pub fn pick_avatar(&self, persist: &dyn Persist, item: Option<&ItemCard>) {
        self.with(|s| {
            let Some(a) = s.address.clone() else { return };
            let pick = item.and_then(|i| i.image_url.as_ref().map(|u| AvatarPick { id: i.id.clone(), image_url: u.clone(), name: i.name.clone(), at: now_ms() }));
            match &pick {
                Some(p) => persist.put(&avatar_key(&a), serde_json::to_string(p).unwrap_or_default()),
                None => persist.remove(&avatar_key(&a)),
            }
            s.avatar = pick;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::CoinBalance;
    use crate::BoxFut;
    use serde_json::{json, Value};

    #[test]
    fn display_name_order() {
        let a = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        assert_eq!(display_name(Some("mira"), Some("m.sui"), None, Some(a)), "mira");
        assert_eq!(display_name(None, Some("m.sui"), Some("Main"), Some(a)), "m.sui");
        assert_eq!(display_name(None, None, Some("Main"), Some(a)), "Main");
        assert_eq!(display_name(None, None, Some("0xabc"), Some(a)), "0x1234...cdef");
        assert_eq!(display_name(None, None, None, None), "");
    }

    #[test]
    fn total_mist_needs_one_read() {
        let mut p = ProfileState::default();
        assert_eq!(total_mist(&p), None);
        p.sui_mist = Some(5);
        p.nft_mist = Some(7);
        assert_eq!(total_mist(&p), Some(12));
    }

    struct Chain;
    impl ChainRead for Chain {
        fn default_name(&self, _a: &str) -> BoxFut<'_, Option<String>> {
            Box::pin(async { Ok(Some("mira.sui".into())) })
        }
        fn balance(&self, _o: &str, t: &str) -> BoxFut<'_, CoinBalance> {
            let t = t.to_string();
            Box::pin(async move { Ok(CoinBalance { coin_type: t, balance: 2_000_000_000, ..Default::default() }) })
        }
    }
    struct Api;
    impl MarketApi for Api {
        fn get(&self, p: &str) -> BoxFut<'_, Value> {
            let v = if p == "/v1/status" {
                json!({ "suiUsd": 2.5, "tps": 100, "checkpoint": 1, "degraded": false })
            } else if p.ends_with("/portfolio?limit=1") {
                json!({ "valueMist": "3000000000", "items": 2, "collections": [] })
            } else {
                json!({ "address": "0x1", "username": "mira", "avatarUrl": "https://a.png" })
            };
            Box::pin(async move { Ok(v) })
        }
    }

    #[tokio::test]
    async fn refresh_reads_chain_and_market() {
        let s = ProfileStore::default();
        let p = MemPersist::default();
        let cur = Current { address: "0x1".into(), label: Some("Main".into()) };
        s.refresh(&Chain, &Api, &p, Some(cur.clone()), false, 1_000.0).await;
        let st = s.snapshot();
        assert_eq!(st.name.as_deref(), Some("mira.sui"));
        assert_eq!(st.username.as_deref(), Some("mira"));
        assert_eq!(st.sui_mist, Some(2_000_000_000));
        assert_eq!(st.nft_mist, Some(3_000_000_000));
        assert_eq!(st.sui_usd, Some(2.5));
        assert_eq!(st.display_name(), "mira");
        assert_eq!(total_mist(&st), Some(5_000_000_000));

        let item = ItemCard { id: "0xn".into(), name: "Popkin".into(), image_url: Some("https://p.png".into()), ..Default::default() };
        s.pick_avatar(&p, Some(&item));
        assert_eq!(s.snapshot().avatar.unwrap().image_url, "https://p.png");
        // the pick survives a reload from storage
        let s2 = ProfileStore::default();
        s2.refresh(&Chain, &Api, &p, Some(cur), false, 2_000.0).await;
        assert_eq!(s2.snapshot().avatar.unwrap().id, "0xn");
        s2.refresh(&Chain, &Api, &p, None, false, 3_000.0).await;
        assert_eq!(s2.snapshot().address, None);
    }

    struct Picked(f64);
    impl MarketApi for Picked {
        fn get(&self, p: &str) -> BoxFut<'_, Value> {
            let v = if p.starts_with("/v1/accounts/") && !p.contains("portfolio") {
                json!({ "address": "0x1", "avatarUrl": "https://rare.market/avatars/512/fox.png", "avatar": { "kind": "default", "id": "fox", "url": "https://rare.market/avatars/512/fox.png", "at": self.0 } })
            } else {
                return Api.get(p);
            };
            Box::pin(async move { Ok(v) })
        }
    }

    #[tokio::test]
    async fn newest_pick_wins() {
        let s = ProfileStore::default();
        let p = MemPersist::default();
        let cur = Current { address: "0x1".into(), label: None };
        s.refresh(&Chain, &Api, &p, Some(cur.clone()), false, 1_000.0).await;
        let item = ItemCard { id: "0xn".into(), name: "Popkin".into(), image_url: Some("https://p.png".into()), ..Default::default() };
        s.pick_avatar(&p, Some(&item));
        let mine = s.snapshot().avatar.unwrap().at;
        // an older pick on the site does not replace this device's
        s.refresh(&Chain, &Picked((mine - 60_000) as f64), &p, Some(cur.clone()), true, 2_000.0).await;
        assert_eq!(s.snapshot().avatar.unwrap().id, "0xn");
        // a newer one does, and is kept
        s.refresh(&Chain, &Picked((mine + 60_000) as f64), &p, Some(cur.clone()), true, 3_000.0).await;
        assert_eq!(s.snapshot().avatar.unwrap().id, "fox");
        assert_eq!(read_pick(&p, "0x1").unwrap().at, mine + 60_000);
    }
}
