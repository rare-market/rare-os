//! Steam achievements, read from the files the Steam client keeps on this device. Nothing here
//! talks to Steam's servers.
//!
//! Two sources, merged per game:
//!
//! 1. `appcache/stats/` (binary KeyValues, the same encoding as shortcuts.vdf):
//!    - `UserGameStatsSchema_<appid>.bin`: `<appid> { gamename, stats { <stat id> { type, bits {
//!      <bit> { name (API name), bit, display { name { english, .. }, desc { .. }, hidden, icon,
//!      icon_gray } } } } } }`. Achievements are the bits of stats of type 4 (achievements) or 5
//!      (group achievements); any stat with a `bits` block counts.
//!    - `UserGameStats_<account id>_<appid>.bin`: `cache { crc, PendingChanges, <stat id> { data
//!      (int32 bitfield of unlocked bits), AchievementTimes { <bit> (unix seconds) } } }`.
//!      The file name carries the 32-bit account id; a 64-bit SteamID is accepted too.
//!    These are written the moment a game unlocks something, so they are the primary source.
//! 2. `userdata/<account id>/config/librarycache/<appid>.json`: the library page's cache, a JSON
//!    array of `[key, { version, data }]` pairs whose `achievements` entry holds `vecHighlight`,
//!    `vecUnachieved` and `vecAchievedHidden` lists of `{ strID, strName, strDescription,
//!    bAchieved, rtUnlocked, strImage, bHidden, flAchieved }` plus `nTotal` / `nAchieved`. It is
//!    a partial list (Steam keeps a handful per group) that the server fills, so it only adds
//!    names, icons and global percentages, and covers games whose stats files are missing.
//!
//! Every parser here is defensive: a truncated or odd file yields what parsed cleanly.

use rare_library::shortcuts::{self, Bin};
use rare_library::{steam, vdf};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// SteamID64 of account id 0 (individual account, public universe).
pub const STEAMID64_BASE: u64 = 76_561_197_960_265_728;
/// Schema files of big games run to a few MB; anything far past that is not a schema.
const MAX_FILE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    /// Steam's API name (`ACH_WIN_ONE_GAME`): the key the trophy registry maps to a trophy id.
    pub api_name: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub icon_gray_url: Option<String>,
    pub hidden: bool,
    pub unlocked: bool,
    /// Unix seconds. None when locked, or unlocked before Steam kept times (2009).
    pub unlock_time: Option<u64>,
    /// Share of Steam players who have it, when the library cache knows.
    pub global_percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameAchievements {
    pub appid: u32,
    /// From the schema; empty when only the library cache had data (the UI uses the library title).
    pub name: String,
    /// The Steam user these unlocks belong to, as a SteamID64 (decimal string, too big for JS numbers).
    pub steam_id: String,
    pub achievements: Vec<Achievement>,
    pub total: u32,
    pub unlocked: u32,
    /// Newest unlock, unix seconds.
    pub last_unlock: Option<u64>,
    /// "stats" (appcache/stats) or "librarycache" (library cache only).
    pub source: &'static str,
}

// ---------- binary KeyValues helpers ----------

fn int_of(v: &Bin) -> Option<i64> {
    match v {
        Bin::Int(i) => Some(*i),
        Bin::Str(s) => s.trim().parse().ok(),
        Bin::Float(f) => Some(*f as i64),
        Bin::Obj(_) => None,
    }
}

fn bool_of(v: Option<&Bin>) -> bool {
    match v {
        Some(Bin::Int(i)) => *i != 0,
        Some(Bin::Str(s)) => matches!(s.trim(), "1" | "true" | "True"),
        _ => false,
    }
}

/// A display string: either plain, or `{ english, german, .., token }`. English first, then any language.
fn localized(v: Option<&Bin>) -> String {
    match v {
        Some(Bin::Str(s)) => s.clone(),
        Some(Bin::Obj(items)) => items
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("english"))
            .or_else(|| items.iter().find(|(k, v)| !k.eq_ignore_ascii_case("token") && matches!(v, Bin::Str(s) if !s.is_empty())))
            .and_then(|(_, v)| if let Bin::Str(s) = v { Some(s.clone()) } else { None })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Achievement icons are file names on Steam's community CDN (or, rarely, full URLs).
pub fn icon_url(appid: u32, icon: &str) -> Option<String> {
    let icon = icon.trim();
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with("https://") || icon.starts_with("http://") {
        return Some(icon.to_string());
    }
    Some(format!("https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{appid}/{icon}"))
}

// ---------- schema ----------

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaEntry {
    pub stat_id: String,
    pub bit: u32,
    pub api_name: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub icon_gray: String,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub game_name: String,
    pub entries: Vec<SchemaEntry>,
}

/// Parses `UserGameStatsSchema_<appid>.bin`. Entries keep the schema's order.
pub fn parse_schema(appid: u32, bytes: &[u8]) -> Schema {
    let root = shortcuts::parse(bytes);
    // The top key is the appid; fall back to the first object for files that spell it differently.
    let app = root
        .get(&appid.to_string())
        .or_else(|| root.entries().iter().find(|(_, v)| matches!(v, Bin::Obj(_))).map(|(_, v)| v));
    let Some(app) = app else { return Schema::default() };
    let mut out = Schema { game_name: app.str("gamename").unwrap_or("").to_string(), entries: Vec::new() };
    let Some(stats) = app.get("stats") else { return out };
    for (stat_key, stat) in stats.entries() {
        let Some(bits) = stat.get("bits") else { continue };
        let stat_id = stat.get("id").and_then(int_of).map(|i| i.to_string()).unwrap_or_else(|| stat_key.clone());
        for (bit_key, b) in bits.entries() {
            let Some(api_name) = b.str("name").filter(|s| !s.is_empty()) else { continue };
            let bit = b.get("bit").and_then(int_of).or_else(|| bit_key.parse().ok());
            let Some(bit) = bit.filter(|b| (0..32).contains(b)) else { continue };
            let display = b.get("display");
            let d = |k: &str| display.and_then(|d| d.get(k));
            out.entries.push(SchemaEntry {
                stat_id: stat_id.clone(),
                bit: bit as u32,
                api_name: api_name.to_string(),
                name: localized(d("name")),
                description: localized(d("desc")),
                icon: d("icon").and_then(|v| if let Bin::Str(s) = v { Some(s.clone()) } else { None }).unwrap_or_default(),
                icon_gray: d("icon_gray").and_then(|v| if let Bin::Str(s) = v { Some(s.clone()) } else { None }).unwrap_or_default(),
                hidden: bool_of(d("hidden")),
            });
        }
    }
    out
}

// ---------- user stats ----------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatBits {
    /// Unlocked bits.
    pub data: u32,
    /// bit -> unix seconds
    pub times: HashMap<u32, u64>,
}

/// Parses `UserGameStats_<user>_<appid>.bin`: stat id -> unlocked bits and their times.
pub fn parse_user_stats(bytes: &[u8]) -> HashMap<String, StatBits> {
    let root = shortcuts::parse(bytes);
    let cache = root.get("cache").unwrap_or(&root);
    let mut out = HashMap::new();
    for (key, stat) in cache.entries() {
        if !matches!(stat, Bin::Obj(_)) {
            continue; // crc, PendingChanges
        }
        let data = stat.get("data").and_then(int_of).map(|d| d as u32).unwrap_or(0);
        let mut times = HashMap::new();
        if let Some(t) = stat.get("AchievementTimes") {
            for (bit, v) in t.entries() {
                if let (Ok(bit), Some(secs)) = (bit.parse::<u32>(), int_of(v)) {
                    // Times are 32-bit on disk; read them unsigned.
                    let secs = (secs as u32) as u64;
                    if bit < 32 && secs > 0 {
                        times.insert(bit, secs);
                    }
                }
            }
        }
        out.insert(key.clone(), StatBits { data, times });
    }
    out
}

/// Joins a schema with the user's bits. A bit counts as unlocked when it is set in `data`; a
/// time without the bit set (a reset achievement) does not.
pub fn merge_stats(appid: u32, schema: &Schema, user: &HashMap<String, StatBits>) -> Vec<Achievement> {
    schema
        .entries
        .iter()
        .map(|e| {
            let st = user.get(&e.stat_id);
            let unlocked = st.is_some_and(|s| (s.data >> e.bit) & 1 == 1);
            let unlock_time = if unlocked { st.and_then(|s| s.times.get(&e.bit).copied()) } else { None };
            Achievement {
                api_name: e.api_name.clone(),
                name: if e.name.is_empty() { e.api_name.clone() } else { e.name.clone() },
                description: e.description.clone(),
                icon_url: icon_url(appid, &e.icon),
                icon_gray_url: icon_url(appid, &e.icon_gray),
                hidden: e.hidden,
                unlocked,
                unlock_time,
                global_percent: None,
            }
        })
        .collect()
}

// ---------- library cache (JSON) ----------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LibraryCache {
    pub achievements: Vec<Achievement>,
    pub total: u32,
    pub achieved: u32,
}

/// Parses `userdata/<user>/config/librarycache/<appid>.json`.
pub fn parse_library_cache(bytes: &[u8]) -> Option<LibraryCache> {
    let doc: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let data = doc.as_array()?.iter().find_map(|e| {
        let pair = e.as_array()?;
        (pair.first()?.as_str()? == "achievements").then(|| pair.get(1)?.get("data").cloned()).flatten()
    })?;
    let mut out = LibraryCache {
        total: data.get("nTotal").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        achieved: data.get("nAchieved").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        achievements: Vec::new(),
    };
    for (list, hidden) in [("vecHighlight", false), ("vecUnachieved", false), ("vecAchievedHidden", true)] {
        for a in data.get(list).and_then(|v| v.as_array()).into_iter().flatten() {
            let Some(id) = a.get("strID").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) else { continue };
            if out.achievements.iter().any(|x| x.api_name == id) {
                continue;
            }
            let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let unlocked = a.get("bAchieved").and_then(|v| v.as_bool()).unwrap_or(false);
            let image = s("strImage");
            out.achievements.push(Achievement {
                api_name: id.to_string(),
                name: if s("strName").is_empty() { id.to_string() } else { s("strName") },
                description: s("strDescription"),
                icon_url: (!image.is_empty()).then(|| image.clone()),
                icon_gray_url: None,
                hidden: hidden || a.get("bHidden").and_then(|v| v.as_bool()).unwrap_or(false),
                unlocked,
                unlock_time: if unlocked { a.get("rtUnlocked").and_then(|v| v.as_u64()).filter(|t| *t > 0) } else { None },
                global_percent: a.get("flAchieved").and_then(|v| v.as_f64()).map(|f| f as f32),
            });
        }
    }
    Some(out)
}

/// Adds what the library cache knows to a stats-file list: global percentages, missing icons,
/// and unlocks the server already has that the stats file lacks (another device's unlock).
pub fn enrich(list: &mut [Achievement], cache: &LibraryCache) {
    for a in list.iter_mut() {
        let Some(c) = cache.achievements.iter().find(|c| c.api_name == a.api_name) else { continue };
        a.global_percent = a.global_percent.or(c.global_percent);
        if a.icon_url.is_none() {
            a.icon_url = c.icon_url.clone();
        }
        if c.unlocked && !a.unlocked {
            a.unlocked = true;
            a.unlock_time = c.unlock_time;
        }
    }
}

fn summarize(appid: u32, name: String, steam_id: u64, achievements: Vec<Achievement>, total_hint: u32, source: &'static str) -> GameAchievements {
    let unlocked = achievements.iter().filter(|a| a.unlocked).count() as u32;
    GameAchievements {
        appid,
        name,
        steam_id: steam_id.to_string(),
        last_unlock: achievements.iter().filter_map(|a| a.unlock_time).max(),
        total: total_hint.max(achievements.len() as u32),
        unlocked,
        achievements,
        source,
    }
}

// ---------- users and files ----------

/// A Steam account that signed in on this install.
#[derive(Debug, Clone, PartialEq)]
pub struct SteamUser {
    pub account_id: u32,
    pub most_recent: bool,
}

impl SteamUser {
    pub fn steam_id64(&self) -> u64 {
        STEAMID64_BASE + self.account_id as u64
    }
}

/// 32-bit account id from either form of id in a file or folder name.
pub fn account_id(id: u64) -> Option<u32> {
    if id > u32::MAX as u64 {
        id.checked_sub(STEAMID64_BASE).and_then(|a| u32::try_from(a).ok())
    } else {
        u32::try_from(id).ok().filter(|a| *a > 0)
    }
}

/// Users of one Steam install: config/loginusers.vdf (SteamID64 keys, MostRecent) plus the
/// account folders in userdata/. The most recent user comes first.
pub fn users(root: &Path) -> Vec<SteamUser> {
    let mut out: Vec<SteamUser> = Vec::new();
    if let Ok(text) = fs::read_to_string(root.join("config/loginusers.vdf")) {
        let v = vdf::parse(&text);
        for (id, u) in v.get("users").map(|u| u.entries().to_vec()).unwrap_or_default() {
            if let Some(a) = id.parse::<u64>().ok().and_then(account_id) {
                if !out.iter().any(|x| x.account_id == a) {
                    out.push(SteamUser { account_id: a, most_recent: u.str("MostRecent") == Some("1") });
                }
            }
        }
    }
    if let Ok(rd) = fs::read_dir(root.join("userdata")) {
        for e in rd.flatten() {
            if let Some(a) = e.file_name().to_str().and_then(|s| s.parse::<u64>().ok()).and_then(account_id) {
                if !out.iter().any(|x| x.account_id == a) {
                    out.push(SteamUser { account_id: a, most_recent: false });
                }
            }
        }
    }
    out.sort_by_key(|u| !u.most_recent);
    out
}

fn read_small(p: &Path) -> Option<Vec<u8>> {
    let m = fs::metadata(p).ok()?;
    if !m.is_file() || m.len() > MAX_FILE {
        return None;
    }
    fs::read(p).ok()
}

/// (user id as written, appid) from `UserGameStats_<user>_<appid>.bin`.
pub fn user_stats_name(name: &str) -> Option<(u64, u32)> {
    let rest = name.strip_prefix("UserGameStats_")?.strip_suffix(".bin")?;
    let (user, app) = rest.split_once('_')?;
    Some((user.parse().ok()?, app.parse().ok()?))
}

/// Every game with achievements for the install's most recent user (the first user with any
/// data when nobody is marked most recent). `only` limits the scan to one appid.
pub fn scan_root(root: &Path, only: Option<u32>) -> Vec<GameAchievements> {
    let users = users(root);
    let stats_dir = root.join("appcache/stats");
    // account id -> appid -> stats file
    let mut files: HashMap<u32, BTreeMap<u32, PathBuf>> = HashMap::new();
    if let Ok(rd) = fs::read_dir(&stats_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((uid, appid)) = user_stats_name(&name) else { continue };
            if only.is_some_and(|o| o != appid) {
                continue;
            }
            if let Some(a) = account_id(uid) {
                files.entry(a).or_default().insert(appid, e.path());
            }
        }
    }
    let cache_dir = |a: u32| root.join(format!("userdata/{a}/config/librarycache"));
    let has_data = |u: &SteamUser| files.contains_key(&u.account_id) || cache_dir(u.account_id).is_dir();
    let Some(user) = users.iter().find(|u| u.most_recent && has_data(u)).or_else(|| users.iter().find(|u| has_data(u))).cloned() else {
        return Vec::new();
    };
    let sid = user.steam_id64();

    let mut out: BTreeMap<u32, GameAchievements> = BTreeMap::new();
    for (appid, path) in files.get(&user.account_id).cloned().unwrap_or_default() {
        let Some(schema_bytes) = read_small(&stats_dir.join(format!("UserGameStatsSchema_{appid}.bin"))) else { continue };
        let schema = parse_schema(appid, &schema_bytes);
        if schema.entries.is_empty() {
            continue;
        }
        let user_stats = read_small(&path).map(|b| parse_user_stats(&b)).unwrap_or_default();
        let mut list = merge_stats(appid, &schema, &user_stats);
        if let Some(c) = read_small(&cache_dir(user.account_id).join(format!("{appid}.json"))).and_then(|b| parse_library_cache(&b)) {
            enrich(&mut list, &c);
        }
        out.insert(appid, summarize(appid, schema.game_name.clone(), sid, list, 0, "stats"));
    }
    // Games only the library cache knows (stats files cleared, or never cached on this device).
    if let Ok(rd) = fs::read_dir(cache_dir(user.account_id)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(appid) = name.strip_suffix(".json").and_then(|s| s.parse::<u32>().ok()) else { continue };
            if out.contains_key(&appid) || only.is_some_and(|o| o != appid) {
                continue;
            }
            let Some(c) = read_small(&e.path()).and_then(|b| parse_library_cache(&b)) else { continue };
            if c.total == 0 && c.achievements.is_empty() {
                continue;
            }
            out.insert(appid, summarize(appid, String::new(), sid, c.achievements.clone(), c.total, "librarycache"));
        }
    }
    out.into_values().collect()
}

/// All Steam installs on this device. The same game in two installs keeps the fuller list.
pub fn scan(only: Option<u32>) -> Vec<GameAchievements> {
    let mut by_app: BTreeMap<u32, GameAchievements> = BTreeMap::new();
    for r in steam::find_roots() {
        for g in scan_root(&r.path, only) {
            let better = by_app.get(&g.appid).map_or(true, |cur|(g.source == "stats", g.unlocked, g.achievements.len()) > (cur.source == "stats", cur.unlocked, cur.achievements.len()));
            if better {
                by_app.insert(g.appid, g);
            }
        }
    }
    by_app.into_values().collect()
}

/// Files whose change can mean a new unlock, for the watcher's cheap signature.
pub fn watched_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for r in steam::find_roots() {
        out.push(r.path.join("appcache/stats"));
        for u in users(&r.path) {
            out.push(r.path.join(format!("userdata/{}/config/librarycache", u.account_id)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- a tiny binary KeyValues writer for edge cases ----
    enum W {
        O(&'static str, Vec<W>),
        S(&'static str, &'static str),
        I(&'static str, i32),
    }
    fn enc(items: &[W], out: &mut Vec<u8>) {
        for w in items {
            match w {
                W::O(k, c) => {
                    out.push(0);
                    out.extend_from_slice(k.as_bytes());
                    out.push(0);
                    enc(c, out);
                    out.push(8);
                }
                W::S(k, v) => {
                    out.push(1);
                    out.extend_from_slice(k.as_bytes());
                    out.push(0);
                    out.extend_from_slice(v.as_bytes());
                    out.push(0);
                }
                W::I(k, v) => {
                    out.push(2);
                    out.extend_from_slice(k.as_bytes());
                    out.push(0);
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    fn file(items: Vec<W>) -> Vec<u8> {
        let mut b = Vec::new();
        enc(&items, &mut b);
        b.push(8);
        b
    }

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam-achievements")
    }

    #[test]
    fn parses_fixture_schema() {
        let bytes = fs::read(fixture().join("appcache/stats/UserGameStatsSchema_480.bin")).unwrap();
        let s = parse_schema(480, &bytes);
        assert_eq!(s.game_name, "Spacewar");
        let names: Vec<&str> = s.entries.iter().map(|e| e.api_name.as_str()).collect();
        assert_eq!(names, ["ACH_WIN_ONE_GAME", "ACH_WIN_100_GAMES", "ACH_TRAVEL_FAR_ACCUM", "ACH_TRAVEL_FAR_SINGLE", "ACH_SECRET_ENDING", "ACH_LAST_BIT"]);
        let win = &s.entries[0];
        assert_eq!((win.stat_id.as_str(), win.bit), ("1", 0));
        assert_eq!(win.name, "Winner");
        assert_eq!(win.description, "Win one game.");
        assert_eq!(win.icon, "0f3a8a1b7d1d0e1f2a3b4c5d6e7f8a9b0c1d2e3f.jpg");
        assert!(!win.hidden);
        // hidden written as an int on one entry and as a string on another
        assert!(s.entries[4].hidden);
        // a German-only name falls back to the first language
        assert_eq!(s.entries[3].name, "Weit gereist");
        // second stat block, bit 31
        assert_eq!((s.entries[5].stat_id.as_str(), s.entries[5].bit), ("2", 31));
    }

    #[test]
    fn parses_fixture_user_stats() {
        let bytes = fs::read(fixture().join("appcache/stats/UserGameStats_12345678_480.bin")).unwrap();
        let u = parse_user_stats(&bytes);
        assert_eq!(u["1"].data, 0b1_0101);
        assert_eq!(u["1"].times[&0], 1_700_000_000);
        assert_eq!(u["2"].data, 1 << 31);
        assert!(!u.contains_key("crc"));
    }

    #[test]
    fn scans_a_fixture_install() {
        let games = scan_root(&fixture(), None);
        assert_eq!(games.iter().map(|g| g.appid).collect::<Vec<_>>(), [480, 620]);
        let sw = &games[0];
        assert_eq!(sw.source, "stats");
        assert_eq!(sw.steam_id, (STEAMID64_BASE + 12_345_678).to_string());
        assert_eq!((sw.total, sw.unlocked), (6, 4));
        let by = |id: &str| sw.achievements.iter().find(|a| a.api_name == id).unwrap();
        assert!(by("ACH_WIN_ONE_GAME").unlocked);
        assert_eq!(by("ACH_WIN_ONE_GAME").unlock_time, Some(1_700_000_000));
        assert_eq!(
            by("ACH_WIN_ONE_GAME").icon_url.as_deref(),
            Some("https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/480/0f3a8a1b7d1d0e1f2a3b4c5d6e7f8a9b0c1d2e3f.jpg")
        );
        // global percent from the library cache
        assert_eq!(by("ACH_WIN_ONE_GAME").global_percent, Some(61.5));
        assert!(!by("ACH_WIN_100_GAMES").unlocked);
        // bit set, no time recorded (pre-2009 style)
        assert!(by("ACH_TRAVEL_FAR_ACCUM").unlocked);
        assert_eq!(by("ACH_TRAVEL_FAR_ACCUM").unlock_time, None);
        // a time for a bit that is not set (reset) does not unlock
        assert!(!by("ACH_TRAVEL_FAR_SINGLE").unlocked);
        assert!(by("ACH_SECRET_ENDING").unlocked && by("ACH_SECRET_ENDING").hidden);
        // bit 31 of a negative int32
        assert!(by("ACH_LAST_BIT").unlocked);
        assert_eq!(by("ACH_LAST_BIT").unlock_time, Some(3_000_000_000));
        assert_eq!(sw.last_unlock, Some(3_000_000_000));

        // Portal 2 has only a library cache entry.
        let p2 = &games[1];
        assert_eq!(p2.source, "librarycache");
        assert_eq!((p2.total, p2.unlocked), (51, 2));
        assert_eq!(p2.achievements.len(), 3);
        assert_eq!(p2.achievements[0].unlock_time, Some(1_690_000_000));
        assert!(p2.achievements[2].hidden);
    }

    #[test]
    fn scan_can_target_one_game() {
        let one = scan_root(&fixture(), Some(620));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].appid, 620);
    }

    #[test]
    fn users_come_from_loginusers_and_userdata() {
        let u = users(&fixture());
        assert_eq!(u[0], SteamUser { account_id: 12_345_678, most_recent: true });
        assert!(u.iter().any(|x| x.account_id == 87_654_321 && !x.most_recent));
    }

    #[test]
    fn ids_and_file_names() {
        assert_eq!(account_id(STEAMID64_BASE + 42), Some(42));
        assert_eq!(account_id(42), Some(42));
        assert_eq!(account_id(0), None);
        assert_eq!(user_stats_name("UserGameStats_12345678_480.bin"), Some((12_345_678, 480)));
        assert_eq!(user_stats_name("UserGameStats_76561197972611406_480.bin"), Some((76_561_197_972_611_406, 480)));
        assert_eq!(user_stats_name("UserGameStatsSchema_480.bin"), None);
        assert_eq!(icon_url(1, ""), None);
        assert_eq!(icon_url(1, "https://x/y.jpg").as_deref(), Some("https://x/y.jpg"));
    }

    #[test]
    fn schema_without_bit_field_uses_the_key_and_string_ints() {
        let b = file(vec![W::O(
            "7",
            vec![W::O(
                "stats",
                vec![W::O(
                    "3",
                    vec![
                        W::S("type", "4"),
                        W::O("bits", vec![W::O("5", vec![W::S("name", "ACH_X"), W::O("display", vec![W::S("name", "Plain name"), W::S("hidden", "1")])])]),
                    ],
                )],
            )],
        )]);
        let s = parse_schema(7, &b);
        assert_eq!(s.entries.len(), 1);
        assert_eq!((s.entries[0].stat_id.as_str(), s.entries[0].bit), ("3", 5));
        assert_eq!(s.entries[0].name, "Plain name");
        assert!(s.entries[0].hidden);
        let user = file(vec![W::O("cache", vec![W::I("crc", 1), W::O("3", vec![W::I("data", 1 << 5)])])]);
        let list = merge_stats(7, &s, &parse_user_stats(&user));
        assert!(list[0].unlocked);
        assert_eq!(list[0].icon_url, None);
    }

    #[test]
    fn truncated_files_never_panic() {
        let bytes = fs::read(fixture().join("appcache/stats/UserGameStatsSchema_480.bin")).unwrap();
        for n in [0, 1, 5, bytes.len() / 3, bytes.len() / 2, bytes.len() - 1] {
            let _ = parse_schema(480, &bytes[..n]);
            let _ = parse_user_stats(&bytes[..n]);
        }
        assert_eq!(parse_library_cache(b"not json"), None);
        assert_eq!(parse_library_cache(b"[[\"friends\",{}]]"), None);
    }

    #[test]
    fn library_cache_unlocks_fill_in() {
        let mut list = vec![Achievement {
            api_name: "A".into(),
            name: "A".into(),
            description: String::new(),
            icon_url: None,
            icon_gray_url: None,
            hidden: false,
            unlocked: false,
            unlock_time: None,
            global_percent: None,
        }];
        let cache = LibraryCache {
            achievements: vec![Achievement { unlocked: true, unlock_time: Some(5), icon_url: Some("https://i".into()), global_percent: Some(2.0), ..list[0].clone() }],
            total: 1,
            achieved: 1,
        };
        enrich(&mut list, &cache);
        assert!(list[0].unlocked);
        assert_eq!(list[0].unlock_time, Some(5));
        assert_eq!(list[0].icon_url.as_deref(), Some("https://i"));
    }
}
