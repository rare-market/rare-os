//! Rare usernames for addresses we meet outside friends (top traders, a trader's page): one lookup
//! per address on rare.market's public /v1/players route, a few at a time, failures quiet.
//!
//! The route is public and the same for everyone, so it goes through the net's cache
//! (`rare:/v1/players/<address>`); this map only remembers the answer for the session, including
//! "none" and failures, so a screen asks once.

use crate::display::{shown, Shown};
use crate::types::Person;
use rare_market::net::{get_market, Net};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

/// Lookups in flight at once.
pub const SLOTS: usize = 3;

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub suins: Option<String>,
}

#[derive(Default)]
struct Inner {
    cache: HashMap<String, Option<Entry>>,
    loading: HashSet<String>,
    active: usize,
}

/// The session's username lookups. Shared (`Arc`) between the screens and the threads that load.
#[derive(Default)]
pub struct RareNames {
    inner: Mutex<Inner>,
    cv: Condvar,
    rev: AtomicU64,
}

impl RareNames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Changes whenever a lookup finishes (screens redraw).
    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::SeqCst)
    }

    /// Loads one address into the map (no-op when known). Blocks while [`SLOTS`] lookups run, and
    /// while another thread loads the same address.
    pub fn load(&self, net: &dyn Net, address: &str) {
        let a = address.to_lowercase();
        let mut g = self.inner.lock().unwrap();
        loop {
            if g.cache.contains_key(&a) {
                return;
            }
            if !g.loading.contains(&a) && g.active < SLOTS {
                break;
            }
            g = self.cv.wait(g).unwrap();
        }
        g.loading.insert(a.clone());
        g.active += 1;
        drop(g);
        let r = get_market::<Entry>(net, &format!("/v1/players/{a}")).ok();
        let mut g = self.inner.lock().unwrap();
        g.cache.insert(a.clone(), r);
        g.loading.remove(&a);
        g.active -= 1;
        drop(g);
        self.rev.fetch_add(1, Ordering::SeqCst);
        self.cv.notify_all();
    }

    /// Whether this address has been looked up (or failed) already.
    pub fn known(&self, address: &str) -> bool {
        self.inner.lock().unwrap().cache.contains_key(&address.to_lowercase())
    }

    /// The Rare username of an address, when it has one; None until known or when there is none.
    pub fn get(&self, address: &str) -> Option<Shown> {
        let g = self.inner.lock().unwrap();
        let e = g.cache.get(&address.to_lowercase())?.as_ref()?;
        let username = e.username.as_ref().filter(|u| !u.is_empty())?;
        Some(shown(Some(&Person { address: address.to_string(), name: None, username: Some(username.clone()), suins: e.suins.clone(), avatar_url: None }), "Someone"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::net::{MockNet, NetError};

    #[test]
    fn looks_up_once_and_stays_quiet_on_failure() {
        let net = MockNet::new();
        net.on_read("/v1/players/0xab", r#"{"username":"mira","suins":"mira.sui"}"#);
        net.on_read("/v1/players/0xcd", r#"{"username":null,"suins":"x.sui"}"#);
        net.fail_read("/v1/players/0xef", NetError::Network("offline".into()));
        let n = RareNames::new();
        assert_eq!(n.get("0xAB"), None);
        n.load(&net, "0xAB");
        n.load(&net, "0xab");
        n.load(&net, "0xcd");
        n.load(&net, "0xef");
        assert_eq!(n.get("0xab").map(|s| (s.primary, s.secondary)), Some(("mira".into(), Some("mira.sui".into()))));
        assert_eq!(n.get("0xcd"), None);
        assert_eq!(n.get("0xef"), None);
        assert!(n.known("0xef"));
        assert_eq!(net.reads().len(), 3);
        assert_eq!(n.rev(), 3);
    }
}
