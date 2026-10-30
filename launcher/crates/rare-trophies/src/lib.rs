//! Rare Trophies, native side: Steam achievements read from this device's Steam files
//! (steam.rs), a watcher that tells the UI about new unlocks, and the one POST the claim flow
//! needs (the attestation request to rare.market, which sends no CORS headers).

pub mod badge;
pub mod case;
pub mod epic;
pub mod gog;
pub mod milestones;
pub mod rarity;
pub mod score;
pub mod steam;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, UNIX_EPOCH};
use rare_core::{Ctx, Error, Event};

/// Every game's achievements read from this device's Steam files, with Steam's global unlock
/// percentages filled in (cached a week; blocking).
pub fn trophies_steam() -> Vec<steam::GameAchievements> {
    let mut list = steam::scan(None);
    for g in list.iter_mut() {
        if let Some(r) = rarity::steam(g.appid) {
            rarity::apply(g, &r);
        }
    }
    list
}

/// The same with only the percentages already cached (no network; fast).
pub fn trophies_steam_cached() -> Vec<steam::GameAchievements> {
    let mut list = steam::scan(None);
    for g in list.iter_mut() {
        if let Some(r) = rarity::cached(g.appid) {
            rarity::apply(g, &r);
        }
    }
    list
}

/// One game's achievements (blocking).
pub fn trophies_steam_app(appid: u32) -> Option<steam::GameAchievements> {
    let mut g = steam::scan(Some(appid)).into_iter().next()?;
    if let Some(r) = rarity::cached(appid) {
        rarity::apply(&mut g, &r);
    }
    Some(g)
}

/// A game's achievements from a store other than Steam (Epic, GOG).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtGame {
    /// The library id: "epic:<app>", "gog:<product>".
    pub id: String,
    pub name: String,
    pub achievements: Vec<steam::Achievement>,
    pub total: u32,
    pub unlocked: u32,
    pub last_unlock: Option<u64>,
    /// "epic" | "gog"
    pub source: &'static str,
}

impl ExtGame {
    pub fn new(id: String, name: String, achievements: Vec<steam::Achievement>, source: &'static str) -> ExtGame {
        let unlocked = achievements.iter().filter(|a| a.unlocked).count() as u32;
        let last_unlock = achievements.iter().filter_map(|a| a.unlock_time).max();
        ExtGame { id, name, total: achievements.len() as u32, unlocked, last_unlock, achievements, source }
    }
}

/// POST https://rare.market/v1/trophies/attest. Answers `{ status, body }` so service errors reach the UI. Blocking.
pub fn trophy_attest(body: serde_json::Value) -> Result<serde_json::Value, Error> {
    let req = ureq::post("https://rare.market/v1/trophies/attest").set("User-Agent", "RareLauncher/0.1").timeout(Duration::from_secs(25));
    let resp = match req.send_json(body) {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(Error::new(e.to_string())),
    };
    let status = resp.status();
    let body: serde_json::Value = resp.into_json().unwrap_or(serde_json::Value::Null);
    Ok(serde_json::json!({ "status": status, "body": body }))
}

/// A new unlock the watcher saw, for the UI's pop-up.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewUnlock {
    pub appid: u32,
    pub game_name: String,
    pub api_name: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub unlock_time: Option<u64>,
}

/// Appid of a file that can carry unlocks: a user stats file or a library cache entry.
fn appid_of(name: &str) -> Option<u32> {
    if let Some((_, app)) = steam::user_stats_name(name) {
        return Some(app);
    }
    name.strip_suffix(".json").and_then(|s| s.parse().ok())
}

/// appid -> signature of its files (mtime and size), across the watched folders.
fn file_stamps() -> HashMap<u32, u64> {
    let mut out: HashMap<u32, DefaultHasher> = HashMap::new();
    for dir in steam::watched_dirs() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(app) = appid_of(&name) else { continue };
            let Ok(m) = e.metadata() else { continue };
            let h = out.entry(app).or_default();
            name.hash(h);
            m.len().hash(h);
            m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).hash(h);
        }
    }
    out.into_iter().map(|(k, h)| (k, h.finish())).collect()
}

/// Unlocks in `now` that `before` did not have.
pub fn diff(before: &HashSet<String>, game: &steam::GameAchievements, name_hint: &str) -> Vec<NewUnlock> {
    game.achievements
        .iter()
        .filter(|a| a.unlocked && !before.contains(&a.api_name))
        .map(|a| NewUnlock {
            appid: game.appid,
            game_name: if game.name.is_empty() { name_hint.to_string() } else { game.name.clone() },
            api_name: a.api_name.clone(),
            name: a.name.clone(),
            description: a.description.clone(),
            icon_url: a.icon_url.clone(),
            unlock_time: a.unlock_time,
        })
        .collect()
}

fn unlocked_set(g: &steam::GameAchievements) -> HashSet<String> {
    g.achievements.iter().filter(|a| a.unlocked).map(|a| a.api_name.clone()).collect()
}

/// Polls the stats and library cache files every few seconds; when a game's files change it
/// rereads that game only and emits `steam-achievements` `{ unlocks: [...] }` for new unlocks.
/// The first pass only learns what is already unlocked.
pub fn watch(ctx: Ctx) {
    std::thread::spawn(move || {
        let mut stamps = file_stamps();
        let mut known: HashMap<u32, HashSet<String>> = steam::scan(None).iter().map(|g| (g.appid, unlocked_set(g))).collect();
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let now = file_stamps();
            let changed: Vec<u32> = now.iter().filter(|(k, v)| stamps.get(k) != Some(v)).map(|(k, _)| *k).collect();
            stamps = now;
            let mut unlocks = Vec::new();
            for appid in changed {
                let Some(g) = steam::scan(Some(appid)).into_iter().next() else { continue };
                let before = known.get(&appid).cloned().unwrap_or_default();
                unlocks.extend(diff(&before, &g, ""));
                known.insert(appid, unlocked_set(&g));
            }
            if !unlocks.is_empty() {
                ctx.send(Event::Other { name: "steam-achievements".into(), payload: serde_json::json!({ "unlocks": unlocks }) });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn diff_reports_only_new_unlocks() {
        let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam-achievements");
        let g = steam::scan_root(&fx, Some(480)).remove(0);
        let mut before = unlocked_set(&g);
        assert!(diff(&before, &g, "").is_empty());
        before.remove("ACH_WIN_ONE_GAME");
        let new = diff(&before, &g, "");
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].api_name, "ACH_WIN_ONE_GAME");
        assert_eq!(new[0].game_name, "Spacewar");
        assert_eq!(new[0].unlock_time, Some(1_700_000_000));
    }

    #[test]
    fn appids_from_file_names() {
        assert_eq!(appid_of("UserGameStats_12345678_480.bin"), Some(480));
        assert_eq!(appid_of("620.json"), Some(620));
        assert_eq!(appid_of("UserGameStatsSchema_480.bin"), None);
        assert_eq!(appid_of("achievement_progress.json"), None);
    }
}
