//! The app side of `data::keep`: tells the feeds whether the launcher is in front, turns real
//! events into refreshes (waking from sleep, the network coming back, a new notice), keeps the
//! current account's wallet data fresh in the background and Home's account card in step with it.

use super::App;
use crate::data::{account, keep};

/// What the last update saw.
#[derive(Default)]
pub(super) struct KeepSeen {
    wall_ms: u64,
    online: Option<bool>,
    notices: Option<usize>,
    wallet_ver: u64,
    saved_account: u64,
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl App {
    pub(super) fn keep_update(&mut self, now: f64) {
        if self.fixture {
            return;
        }
        keep::set_front(self.st.running.is_empty() || self.ingame.is_some());
        // Waking from sleep: the frame loop stood still for a while.
        let w = wall_ms();
        if self.keep_seen.wall_ms > 0 && w.saturating_sub(self.keep_seen.wall_ms) > 60_000 {
            keep::nudge();
        }
        self.keep_seen.wall_ms = w;
        // The network came back.
        if let Some(online) = self.st.device.as_ref().map(|d| d.wifi.is_some()) {
            if self.keep_seen.online == Some(false) && online {
                keep::nudge();
            }
            self.keep_seen.online = Some(online);
        }
        // A new notice (a sale, an offer, a transfer) can change balances and items.
        if let Some(social) = &self.social {
            let n = social.with_store(|s| s.notices.len());
            if self.keep_seen.notices.is_some_and(|m| n > m) {
                keep::nudge();
            }
            self.keep_seen.notices = Some(n);
        }
        // The current account's wallet data, and Home's card from it.
        if let Some(addr) = self.st.account.as_ref().map(|a| a.address.clone()) {
            crate::data::wallet::tick(&addr, now);
        }
        let v = crate::data::wallet::version();
        if v != self.keep_seen.wallet_ver {
            self.keep_seen.wallet_ver = v;
            if let Some(a) = self.st.account.as_ref() {
                let mut n = a.clone();
                account::fill(&mut n);
                if &n != a {
                    self.st.account = Some(n);
                    self.st.ver.account += 1;
                }
            }
        }
        if self.st.ver.account != self.keep_seen.saved_account {
            self.keep_seen.saved_account = self.st.ver.account;
            account::save(self.st.account.as_ref());
        }
    }
}
