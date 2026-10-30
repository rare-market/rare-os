//! Perks on the profile (perks/store.ts): whether this account holds a Rareling, the chosen theme,
//! and the profile's badge and panel. Rarelings are OFF in this release (`rarelings_on()` is the
//! rare-perks `rarelings` feature, off), so every view here answers None; building the launcher with
//! `--features rarelings` turns the whole path on.

use crate::screens::perks::Msg;
use crate::screens::wallet::markup::El;
use std::sync::Mutex;

pub fn on() -> bool {
    rare_perks::config::rarelings_on()
}

#[derive(Clone, Default)]
struct Held {
    address: Option<String>,
    owned: bool,
    count: u32,
    checking: bool,
    ver: u64,
    /// Launches that have not opened yet, soonest first (rare.market `/launchpad/launches.json`).
    drops: Vec<crate::screens::perks::Launch>,
}

static HELD: Mutex<Held> = Mutex::new(Held { address: None, owned: false, count: 0, checking: false, ver: 0, drops: Vec::new() });

fn held() -> Held {
    HELD.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn ver() -> u64 {
    held().ver
}

/// Checks the account for a Rareling in the background (wallet, then its kiosks).
pub fn check(address: Option<String>) {
    if !on() {
        return;
    }
    {
        let mut h = HELD.lock().unwrap_or_else(|e| e.into_inner());
        if h.checking || (h.address == address && address.is_some()) {
            return;
        }
        h.checking = true;
        h.address = address.clone();
    }
    std::thread::Builder::new()
        .name("rr-perks".into())
        .spawn(move || {
            let (mut owned, mut count) = (false, 0u32);
            if let (Some(a), Some(sui)) = (address.as_deref(), crate::screens::wallet::live::sui()) {
                let reader = rare_perks::detect::ChainReader { chain: std::sync::Arc::new(sui.clone()), rt: crate::screens::wallet::live::rt().handle().clone() };
                let ty = rare_perks::config::rareling_type(None);
                if let Ok(rare_perks::detect::RarelingResult::Owned { count: c, .. }) = rare_perks::detect::check_rareling(a, ty.as_deref(), &reader) {
                    owned = true;
                    count = c as u32;
                }
            }
            let drops = if owned { upcoming_drops() } else { Vec::new() };
            let mut h = HELD.lock().unwrap_or_else(|e| e.into_inner());
            h.drops = drops;
            h.owned = owned;
            h.count = count;
            h.checking = false;
            h.ver += 1;
            super::bus::wake();
        })
        .ok();
}

/// The early-access list: launches that have not opened yet (`upcomingLaunches`). Empty offline.
fn upcoming_drops() -> Vec<crate::screens::perks::Launch> {
    use rare_perks::launches::{starts_ms, upcoming_launches, Launch};
    let net = crate::screens::market::feed::net();
    let Ok(v) = rare_market::api::get::<serde_json::Value>(&*net, "/launchpad/launches.json") else { return Vec::new() };
    let list: Vec<Launch> = v.as_array().map(|a| a.iter().filter_map(|x| serde_json::from_value(x.clone()).ok()).collect()).unwrap_or_default();
    let image = |slug: &str| v.as_array().and_then(|a| a.iter().find(|x| x["slug"].as_str() == Some(slug))).and_then(|x| x["imageUrl"].as_str().map(str::to_string));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    upcoming_launches(&list, now).into_iter().map(|l| crate::screens::perks::Launch { image_url: image(&l.slug), starts_ms: starts_ms(&l), slug: l.slug, name: l.name }).collect()
}

pub fn theme() -> String {
    super::config::get_str("rarelingTheme").unwrap_or_else(|| "rare".into())
}

pub fn set_theme(id: &str) {
    super::config::set(&[("rarelingTheme", serde_json::json!(id))]);
}

/// The profile's badge next to the name (holders only).
pub fn badge() -> Option<El<Msg>> {
    let h = held();
    crate::screens::perks::badge(on(), h.owned, h.count)
}

/// The profile's perks panel: locked for everyone else, themes and early access for holders.
pub fn section(games: &[crate::screens::sui::catalog::SuiGame], tz_min: i32) -> Option<El<Msg>> {
    if !on() {
        return None;
    }
    let owned = std::collections::BTreeMap::new();
    let tokens = std::collections::BTreeMap::new();
    let d = crate::screens::sui::tab::Data { owned: &owned, tokens: &tokens };
    crate::screens::perks::section_view(true, held().owned, &theme(), &held().drops, games, &d, tz_min)
}
