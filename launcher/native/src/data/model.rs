//! The library and device models, the same JSON as src-tauri's model.rs and lib/types.ts, so the
//! backend's cache (~/.cache/rare/library.json) and the rare-library crate's values load as is.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Art {
    #[serde(default)]
    pub cover: Vec<String>,
    #[serde(default)]
    pub hero: Vec<String>,
    #[serde(default)]
    pub logo: Vec<String>,
    #[serde(default)]
    pub header: Vec<String>,
    #[serde(default)]
    pub icon: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSpec {
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub appid: Option<u64>,
    /// `exec`: run through umu-run (Proton) in Rare's prefix (cloud saves find its save folders).
    #[serde(default)]
    pub windows: bool,
    /// `steamShortcut`: Steam's 64-bit id of a non-Steam game (the top 32 bits name its Proton prefix).
    #[serde(default)]
    pub gameid: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub source: String,
    pub title: String,
    #[serde(default = "yes")]
    pub installed: bool,
    #[serde(default)]
    pub install_dir: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<f64>,
    #[serde(default)]
    pub last_played: Option<i64>,
    #[serde(default)]
    pub playtime_minutes: Option<i64>,
    #[serde(default)]
    pub art: Art,
    #[serde(default)]
    pub launch: Option<LaunchSpec>,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub foreign: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub source: String,
    pub available: bool,
    pub count: u32,
    pub note: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub games: Vec<Game>,
    #[serde(default)]
    pub sources: Vec<SourceStatus>,
    #[serde(default)]
    pub foreign_steam_libraries: Vec<String>,
    /// Steam games the account owns that are not installed here (Library > Not installed).
    #[serde(default)]
    pub owned: Vec<Game>,
    #[serde(default)]
    pub scanned_at: i64,
    #[serde(default)]
    pub emulation_app: Option<String>,
    /// ES-DE's systems and recent ROMs, for the emulator's page.
    #[serde(default)]
    pub emulation: Option<Emulation>,
    /// Cloud games: kept apart from `games` so only
    /// Library > Cloud, search, favourites and collections the player makes, and Recently played
    /// once one was played, show them. `split_cloud` moves them here.
    #[serde(default)]
    pub cloud: Vec<Game>,
    /// What a game is when a store or the disc says so (rare-library's keys): data/groups.rs.
    #[serde(default)]
    pub keys: std::collections::BTreeMap<String, String>,
}

impl Library {
    /// Moves the cloud games out of `games` (the scan and the cache list them with the rest).
    pub fn split_cloud(&mut self) {
        let (cloud, games): (Vec<Game>, Vec<Game>) = std::mem::take(&mut self.games).into_iter().partition(|g| g.source == "cloud");
        self.games = games;
        self.cloud.extend(cloud);
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RomSystem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub count: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RomGame {
    pub name: String,
    pub system: String,
    #[serde(default)]
    pub last_played: Option<i64>,
    #[serde(default)]
    pub playtime: u64,
    /// The gamelist path inside the system's ROM folder (Delete removes it).
    #[serde(default)]
    pub path: Option<String>,
}

impl RomGame {
    /// The id a ROM goes by for Uninstall: "rom:<system>/<path in the system folder>".
    pub fn uninstall_id(&self) -> Option<String> {
        self.path.as_ref().map(|p| format!("rom:{}/{}", self.system, p.trim_start_matches("./")))
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Emulation {
    #[serde(default)]
    pub systems: Vec<RomSystem>,
    #[serde(default)]
    pub recent: Vec<RomGame>,
}

/// What uninstalling a game does, for the sheet (rare-library uninstall::Plan).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct UninstallInfo {
    /// Steam removes it (and may ask to confirm in its own window).
    pub steam: bool,
    /// Bytes the game frees.
    pub size: u64,
    /// Saves Rare found next to the game (a Wine prefix, a web game's storage, an app's data):
    /// their size and what they are ("prefix", "web", "app"). The sheet asks whether to keep them.
    pub saves: Option<(u64, &'static str)>,
    /// Where it is installed.
    pub place: Option<String>,
    /// Free space on the drive it is on.
    pub free: Option<u64>,
}

/// One Proton build Steam can use (bridge.ts ProtonTool).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProtonTool {
    pub name: String,
    pub label: String,
    /// "valve", "experimental", "ge", "cachyos" or "other".
    pub kind: String,
    #[serde(default)]
    pub stale: bool,
}

/// bridge.ts ProtonStatus.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProtonStatus {
    pub tools: Vec<ProtonTool>,
    #[serde(default)]
    pub global: Option<String>,
    #[serde(default)]
    pub game: Option<String>,
    #[serde(default)]
    pub installable: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    #[serde(default)]
    pub minutes: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub battery: Option<Battery>,
    /// The Wi-Fi network's name; None when offline.
    #[serde(default)]
    pub wifi: Option<String>,
    /// Remote access (SSH) is on: the top bar shows an icon.
    #[serde(default)]
    pub ssh: bool,
}

/// Quick settings' device controls (types.ts Perf, Levels, Radios), what Quick settings shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Perf {
    pub profile: String,
    pub tdp: Option<f32>,
    pub tdp_min: f32,
    pub tdp_max: f32,
    pub fps_limit: Option<u32>,
    pub cpu_boost: Option<bool>,
    pub fan: Option<String>,
    pub watts: [f32; 3],
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    pub brightness: Option<f32>,
    pub volume: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radios {
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub airplane: bool,
}

/// store label (types.ts SOURCE_LABEL)
pub fn source_label(s: &str) -> &'static str {
    match s {
        "steam" => "Steam",
        "epic" => "Epic Games",
        "gog" => "GOG",
        "itch" => "itch.io",
        "local" => "Local",
        "web" => "Web",
        "apps" => "Emulators",
        "rare" => "Rare Store",
        "cloud" => "Cloud",
        "android" => "Android",
        "emu" => "Emulation",
        _ => "",
    }
}

/// A path as the UI shows it: home folders become ~ (types.ts prettyPath).
pub fn pretty_path(p: &str) -> String {
    let mut out = String::new();
    let mut rest = p;
    loop {
        let found = ["/var/home/", "/home/"].iter().filter_map(|pre| rest.find(pre).map(|i| (i, pre.len()))).min_by_key(|(i, _)| *i);
        match found {
            Some((i, len)) => {
                out.push_str(&rest[..i]);
                let after = &rest[i + len..];
                let end = after.find('/').unwrap_or(after.len());
                out.push('~');
                rest = &after[end..];
            }
            None => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

pub fn now_secs() -> i64 {
    // SAFETY: time(NULL) only reads the clock.
    unsafe { libc::time(std::ptr::null_mut()) as i64 }
}

/// "Played 2 h ago" (components/ui.tsx fmtAgo).
/// Times before this (2001-01-01) are placeholders, not a real play.
pub const REAL_TIME: i64 = 978_307_200;

pub fn fmt_ago(t: Option<i64>) -> String {
    let Some(t) = t.filter(|t| *t >= REAL_TIME) else { return "Never played".into() };
    let s = now_secs() - t;
    if s < 3600 {
        return "Played just now".into();
    }
    if s < 86400 {
        return format!("Played {} h ago", (s as f64 / 3600.0).round());
    }
    if s < 86400 * 2 {
        return "Played yesterday".into();
    }
    if s < 86400 * 30 {
        return format!("Played {} days ago", (s as f64 / 86400.0).round());
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    // SAFETY: localtime_r fills tm.
    unsafe { libc::localtime_r(&tt, &mut tm) };
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // Older than about half a year: with the year, so "Jan 1" never reads as this week's.
    if s > 86400 * 180 {
        return format!("Played {} {}, {}", M[tm.tm_mon.clamp(0, 11) as usize], tm.tm_mday, tm.tm_year + 1900);
    }
    format!("Played {} {}", M[tm.tm_mon.clamp(0, 11) as usize], tm.tm_mday)
}

/// One game on a paired PC (rare-stream StreamApp).
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct PcApp {
    pub id: u32,
    pub name: String,
    /// Box art Rare cached from the PC (a local file).
    pub art: Option<String>,
}

/// Home's "On your PC" row: the first paired PC that answers and its games (stream/logic.ts
/// homeHost, ordered).
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct PcShelf {
    pub uuid: String,
    pub name: String,
    pub online: bool,
    pub can_wake: bool,
    pub current_game: Option<u32>,
    pub apps: Vec<PcApp>,
}

impl PcShelf {
    /// Streams run as "stream:<host uuid>:<app id>".
    pub fn run_id(&self, app: u32) -> String {
        format!("stream:{}:{app}", self.uuid)
    }

    /// The second line under a cover (logic.ts playState + tileLine).
    pub fn tile_line(&self, app: u32, running: &[String]) -> String {
        if running.iter().any(|r| *r == self.run_id(app)) {
            return "Playing".into();
        }
        if !self.online {
            return if self.can_wake { format!("{} is asleep", self.name) } else { format!("{} is off", self.name) };
        }
        match self.current_game {
            Some(g) if g == app => "Running on your PC".into(),
            _ => self.name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert_eq!(pretty_path("/var/home/playtron/.local/share/Steam"), "~/.local/share/Steam");
        assert_eq!(pretty_path("/home/rare/Games"), "~/Games");
        assert_eq!(pretty_path("Steam"), "Steam");
    }

    #[test]
    fn library_json() {
        let j = r#"{"games":[{"id":"steam:1","source":"steam","title":"X","art":{"cover":["a"]},"origin":"Steam","foreign":false}],"sources":[],"foreignSteamLibraries":[],"scannedAt":1}"#;
        let l: Library = serde_json::from_str(j).unwrap();
        assert_eq!(l.games[0].art.cover, vec!["a"]);
        assert!(l.games[0].installed);
    }
}
