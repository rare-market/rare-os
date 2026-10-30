//! Price alerts on rare.market's account alerts API (rare-notify `api`), for this device. The
//! API is scoped by the device id, so nothing is signed: the Rare Wallet address only rides along
//! as `addr` when there is one. Network calls block, so the list is read on a thread into a cache
//! the screens read; adds run on the sheet's press (8 second timeout), deletes go out on a thread.

use super::backend::{AlertsApi, NewAlert};
use super::data::types::PriceAlert;
use crate::screens::wallet::live::market_net;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static CACHE: Mutex<Option<Vec<PriceAlert>>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

pub fn device() -> &'static str {
    static D: OnceLock<String> = OnceLock::new();
    D.get_or_init(|| {
        let st = rare_notify::storage::FileStorage::new(rare_core::paths::data_dir().join("notify"));
        rare_notify::device::device_id(&st)
    })
}

/// Reads the list from the server on a thread; `done` runs after the cache is set.
pub fn refresh_async() {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("rare-alerts".into())
        .spawn(|| {
            if let Ok(l) = rare_notify::api::list_alerts(market_net(), device()) {
                *CACHE.lock().unwrap() = Some(l);
            }
            BUSY.store(false, Ordering::SeqCst);
        })
        .ok();
}

/// Whether any alert is active (the stream stays up for it with no accounts).
pub fn has_active() -> bool {
    CACHE.lock().unwrap().as_ref().is_some_and(|l| l.iter().any(|a| a.active))
}

pub struct LiveAlerts;

impl AlertsApi for LiveAlerts {
    fn list(&mut self) -> Result<Vec<PriceAlert>, String> {
        match CACHE.lock().unwrap().clone() {
            Some(l) => Ok(l),
            None => {
                refresh_async();
                Err("Could not load price alerts".into())
            }
        }
    }

    fn add(&mut self, a: &NewAlert) -> Result<String, String> {
        let n = rare_notify::api::NewAlert { coin_type: a.coin_type.clone(), symbol: a.symbol.clone(), dir: a.dir, target: a.target, addr: a.addr.clone() };
        let id = rare_notify::api::add_alert(market_net(), device(), &n).map_err(|e| e.0)?;
        // The list is read again so the Settings row shows it.
        if let Ok(l) = rare_notify::api::list_alerts(market_net(), device()) {
            *CACHE.lock().unwrap() = Some(l);
        }
        Ok(id)
    }

    fn remove(&mut self, id: &str) -> Result<(), String> {
        if let Some(l) = CACHE.lock().unwrap().as_mut() {
            l.retain(|x| x.id != id);
        }
        let id = id.to_string();
        std::thread::Builder::new()
            .name("rare-alert-del".into())
            .spawn(move || {
                let _ = rare_notify::api::delete_alert(market_net(), &id, device());
                if let Ok(l) = rare_notify::api::list_alerts(market_net(), device()) {
                    *CACHE.lock().unwrap() = Some(l);
                }
            })
            .ok();
        Ok(())
    }
}

/// Whether the live API is used (not in fixtures, shots or tests).
pub fn live() -> bool {
    std::env::var("RR_FIXTURE").is_err() && std::env::var("RR_SHOT").is_err() && std::env::var("RR_NO_SOCIAL").is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::data::types::AlertDir;

    #[test]
    fn remove_drops_the_cached_row_at_once() {
        *CACHE.lock().unwrap() = Some(vec![PriceAlert { id: "a1".into(), active: true, dir: AlertDir::Above, ..Default::default() }]);
        assert!(has_active());
        let _ = LiveAlerts.remove("a1");
        assert!(!has_active());
        assert_eq!(LiveAlerts.list().unwrap().len(), 0);
    }
}
