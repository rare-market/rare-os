//! App-level data that outlives the screens. A screen is rebuilt every time it is entered (the tab
//! switch remounts it), so anything it fetched itself was gone on the next visit and popped in
//! again. The rule here: data lives at the app level, a screen only reads it.
//!
//! - [`hold`] / [`take`] / [`shared`]: UI-thread objects that keep a screen's data (the
//!   Marketplace's answers, Trade's store) across visits, so the next build draws the last values.
//! - [`save`] / [`load`]: the last values on disk (`ui:native:<key>` in the launcher's state cache,
//!   ~/.cache/rare/state), so the first view after boot shows the last known numbers.
//! - [`front`] / [`due`]: background refreshes run on a schedule while the launcher is in front and
//!   pause while a game has the screen.
//! - [`nudge`] / [`epoch`]: real events (a sent transaction, a notice, waking from sleep, the
//!   network coming back, the launcher coming to the front) refresh at once.
//!
//! Golden images (RR_FIXTURE) and unit tests never read or write the disk copy.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static FRONT: AtomicBool = AtomicBool::new(true);
static EPOCH: AtomicU64 = AtomicU64::new(0);
/// A follow-up nudge (wall ms; 0 none): indexers answer a few seconds after a transaction lands.
static AGAIN_MS: AtomicU64 = AtomicU64::new(0);

fn wall_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn fixture() -> bool {
    std::env::var_os("RR_FIXTURE").is_some()
}

// ---------------------------------------------------------------- schedule

/// The launcher has the screen (no game, or Rare over a game). Coming to the front refreshes at once.
pub fn set_front(on: bool) {
    let was = FRONT.swap(on, Ordering::AcqRel);
    if on && !was {
        bump();
    }
}

pub fn front() -> bool {
    FRONT.load(Ordering::Acquire)
}

/// A background refresh last started at `last` (clock secs; 0 never) is due again after `every`
/// secs while the launcher is in front. Paused while a game has the screen.
pub fn due(last: f64, every: f64, now: f64) -> bool {
    last <= 0.0 || (front() && now - last >= every)
}

fn bump() {
    EPOCH.fetch_add(1, Ordering::AcqRel);
    crate::features::bus::wake();
}

/// Something happened that changes what the screens show (a sent transaction, a notice, waking
/// from sleep, the network coming back): refresh now, and once more a few seconds later.
pub fn nudge() {
    AGAIN_MS.store(wall_ms() + 5_000, Ordering::Release);
    bump();
}

/// Moves on every [`nudge`]; a feed that saw an older epoch refreshes now.
pub fn epoch() -> u64 {
    let again = AGAIN_MS.load(Ordering::Acquire);
    if again > 0 && wall_ms() >= again && AGAIN_MS.compare_exchange(again, 0, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        bump();
    }
    EPOCH.load(Ordering::Acquire)
}

// ---------------------------------------------------------------- the disk copy

fn net() -> Option<&'static rare_market::net::StoreNet> {
    (!cfg!(test) && !fixture()).then(crate::screens::wallet::live::market_net)
}

fn disk_key(key: &str) -> String {
    format!("ui:native:{key}")
}

/// Keeps the last value of `key` for the next start (written behind, off the UI thread).
pub fn save<T: Serialize>(key: &str, v: &T) {
    let Some(n) = net() else { return };
    match serde_json::to_string(v) {
        Ok(j) => {
            if let Err(e) = n.store.put(&disk_key(key), j) {
                log::warn!("keep {key}: {e}");
            }
        }
        Err(e) => log::warn!("keep {key}: {e}"),
    }
}

/// The last value saved for `key` (None in golden runs, or when nothing was saved).
pub fn load<T: DeserializeOwned>(key: &str) -> Option<T> {
    let h = net()?.store.peek(&disk_key(key))?;
    serde_json::from_str(&h.body).ok()
}

// ---------------------------------------------------------------- UI-thread objects

thread_local! {
    static HELD: RefCell<HashMap<&'static str, Box<dyn Any>>> = RefCell::new(HashMap::new());
}

/// Keeps `v` for the next screen that asks for `name` (a screen that is going away).
pub fn hold<T: 'static>(name: &'static str, v: T) {
    HELD.with(|h| h.borrow_mut().insert(name, Box::new(v)));
}

/// What [`hold`] kept under `name`, taken out.
pub fn take<T: 'static>(name: &'static str) -> Option<T> {
    HELD.with(|h| h.borrow_mut().remove(name)).and_then(|b| b.downcast::<T>().ok()).map(|b| *b)
}

/// One app-level object every screen that asks for `name` shares, made by `init` the first time.
pub fn shared<T: 'static>(name: &'static str, init: impl FnOnce() -> T) -> Rc<RefCell<T>> {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        if let Some(rc) = h.get(name).and_then(|b| b.downcast_ref::<Rc<RefCell<T>>>()) {
            return rc.clone();
        }
        let rc = Rc::new(RefCell::new(init()));
        h.insert(name, Box::new(rc.clone()));
        rc
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_objects_outlive_the_screen() {
        hold("t.feed", vec![1, 2, 3]);
        assert_eq!(take::<Vec<i32>>("t.feed"), Some(vec![1, 2, 3]));
        assert_eq!(take::<Vec<i32>>("t.feed"), None);
        let a = shared("t.store", || 5u32);
        *a.borrow_mut() = 7;
        assert_eq!(*shared("t.store", || 0u32).borrow(), 7);
    }

    #[test]
    fn refreshes_wait_while_a_game_has_the_screen() {
        assert!(due(0.0, 20.0, 5.0));
        assert!(!due(100.0, 20.0, 110.0));
        assert!(due(100.0, 20.0, 121.0));
        let e = epoch();
        set_front(false);
        assert!(!due(100.0, 20.0, 500.0));
        set_front(true);
        assert!(epoch() > e);
        let e = epoch();
        nudge();
        assert!(epoch() > e);
    }
}
