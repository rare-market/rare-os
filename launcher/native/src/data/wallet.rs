//! The wallet's data per account, at the app level: the portfolio (total, SUI and coins with their
//! USD values), the first page of activity and the cross-game inventory. The Wallet tab, Home's
//! account card and the profile chip all read it here, so leaving and coming back shows the last
//! values at once and a new start shows the last known numbers from disk (`keep`).
//!
//! The app keeps the current account fresh in the background ([`tick`]): portfolio and activity
//! every 20 s and the inventory every 60 s while the launcher is in front, paused while a game has
//! the screen, and at once on a `keep::nudge` (a sent transaction, a notice, waking up). A refresh
//! changes values in place; a failed one keeps the last values.

use super::keep;
use crate::screens::wallet::live::{self, Answer};
use crate::screens::wallet::types::{ActivityItem, Portfolio};
use rare_market::inventory::Inventory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Portfolio and activity while the launcher is in front.
pub const FAST_S: f64 = 20.0;
/// The inventory (rare.market) while the launcher is in front.
pub const SLOW_S: f64 = 60.0;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub pf: Option<Portfolio>,
    /// The first page of activity and the cursor after it.
    pub act: Option<Vec<ActivityItem>>,
    pub cursor: Option<String>,
    pub inv: Option<Inventory>,
    /// The total from the Accounts page's value-only read (when the portfolio was never read).
    pub total: Option<f64>,
    /// The last read failed and nothing is held for it.
    #[serde(skip)]
    pub pf_err: bool,
    #[serde(skip)]
    pub act_err: bool,
    #[serde(skip)]
    pub inv_err: bool,
}

impl Held {
    /// The account's total in USD: the portfolio's, else the last value-only read.
    pub fn total_usd(&self) -> Option<f64> {
        self.pf.as_ref().and_then(|p| p.total_usd).or(self.total)
    }
}

#[derive(Default)]
struct Entry {
    held: Held,
    /// Clock secs the last fast / slow refresh started (0 never).
    fast_at: f64,
    slow_at: f64,
    flight: bool,
    epoch: u64,
}

static DATA: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
static VER: AtomicU64 = AtomicU64::new(1);

fn key(addr: &str) -> String {
    format!("wallet.{}", addr.to_lowercase())
}

fn with<R>(addr: &str, f: impl FnOnce(&mut Entry) -> R) -> R {
    let mut g = DATA.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    let k = addr.to_lowercase();
    let e = m.entry(k).or_insert_with(|| Entry { held: keep::load::<Held>(&key(addr)).unwrap_or_default(), epoch: keep::epoch(), ..Default::default() });
    f(e)
}

/// Moves whenever any account's data changed.
pub fn version() -> u64 {
    VER.load(Ordering::Acquire)
}

fn changed() {
    VER.fetch_add(1, Ordering::AcqRel);
    crate::features::bus::wake();
}

/// What is held for `addr` now (memory, else the copy from the last run; empty the first time).
pub fn get(addr: &str) -> Held {
    with(addr, |e| e.held.clone())
}

/// Reads everything for `addr` now (Retry, a new account).
pub fn refresh(addr: &str) {
    with(addr, |e| {
        e.fast_at = 0.0;
        e.slow_at = 0.0;
    });
    tick(addr, rare_render::clock::secs());
}

/// Keeps `addr` fresh: starts the reads that are due. Cheap; call it every update.
pub fn tick(addr: &str, now: f64) {
    let ep = keep::epoch();
    let job = with(addr, |e| {
        if e.flight {
            return None;
        }
        if e.epoch != ep {
            e.epoch = ep;
            e.fast_at = 0.0;
            e.slow_at = 0.0;
        }
        let fast = keep::due(e.fast_at, FAST_S, now);
        if !fast {
            return None;
        }
        let slow = keep::due(e.slow_at, SLOW_S, now);
        e.fast_at = now;
        if slow {
            e.slow_at = now;
        }
        e.flight = true;
        Some(slow)
    });
    let Some(inv) = job else { return };
    let rx = live::load(addr, inv);
    let a = addr.to_string();
    let spawned = std::thread::Builder::new()
        .name("rare-wallet-data".into())
        .spawn(move || {
            for ans in rx {
                apply(&a, ans);
            }
            with(&a, |e| e.flight = false);
            keep::save(&key(&a), &get(&a));
        });
    if spawned.is_err() {
        with(addr, |e| e.flight = false);
    }
}

fn apply(addr: &str, ans: Answer) {
    let moved = with(addr, |e| {
        let h = &mut e.held;
        let before = h.clone();
        match ans {
            Answer::Portfolio(_, Ok(p)) => {
                h.pf = Some(p);
                h.pf_err = false;
            }
            Answer::Portfolio(_, Err(err)) => {
                log::warn!("wallet portfolio: {err}");
                h.pf_err = h.pf.is_none();
            }
            Answer::Activity(_, Ok((items, cursor))) => {
                h.act = Some(items);
                h.cursor = cursor;
                h.act_err = false;
            }
            Answer::Activity(_, Err(err)) => {
                log::warn!("wallet activity: {err}");
                h.act_err = h.act.is_none();
            }
            Answer::Inventory(_, Ok(i)) => {
                h.inv = Some(i);
                h.inv_err = false;
            }
            Answer::Inventory(_, Err(err)) => {
                log::warn!("wallet inventory: {err}");
                h.inv_err = h.inv.is_none();
            }
            Answer::AccountValue(_, v) => {
                if v.is_some() {
                    h.total = v;
                }
            }
        }
        *h != before
    });
    if moved {
        changed();
    }
}

/// An Accounts page value-only read (kept, so the list shows it next time at once).
pub fn take_value(addr: &str, v: Option<f64>) {
    apply(addr, Answer::AccountValue(addr.to_string(), v));
    if v.is_some() {
        keep::save(&key(addr), &get(addr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_read_keeps_the_last_values() {
        let a = "0xtest-keep-wallet";
        let pf = Portfolio { total_usd: Some(12.5), ..Default::default() };
        apply(a, Answer::Portfolio(a.into(), Ok(pf.clone())));
        let v = version();
        apply(a, Answer::Portfolio(a.into(), Err("offline".into())));
        let h = get(a);
        assert_eq!(h.pf, Some(pf));
        assert!(!h.pf_err);
        assert_eq!(version(), v);
        assert_eq!(h.total_usd(), Some(12.5));
    }
}
