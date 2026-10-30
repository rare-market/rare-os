//! EmulationStation-DE: the ROM systems behind the ES-DE entry. Reads ES-DE's settings for
//! the ROM folder, counts games per system folder, and takes play counts, playtime and last
//! played from ES-DE's gamelists. Nothing is written.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RomSystem {
    /// ES-DE system id, the ROM folder name ("gc", "ps2").
    pub id: String,
    pub name: String,
    /// Games found in the folder.
    pub count: usize,
    /// Unix seconds, from the gamelist.
    pub last_played: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RomGame {
    pub name: String,
    pub system: String,
    pub last_played: Option<u64>,
    pub play_count: u32,
    /// Seconds, as ES-DE records it.
    pub playtime: u64,
    /// The gamelist's path, relative to the system's ROM folder ("./Game (USA).iso").
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Emulation {
    pub rom_dir: String,
    pub systems: Vec<RomSystem>,
    /// Most recently played games, newest first.
    pub recent: Vec<RomGame>,
}

/// Folder names EmuDeck creates next to the ROM systems that hold launchers, not games.
const NOT_SYSTEMS: &[&str] = &[
    "emulators", "desktop", "cloud", "epic", "lutris", "steam", "generic-applications", "kodi", "android", "remoteplay", "windows", "ports",
];

/// Files in ROM folders that are never games.
const NOT_ROMS: &[&str] = &["txt", "xml", "jpg", "jpeg", "png", "webp", "gif", "mp4", "sav", "srm", "state", "cfg", "ini", "db", "json", "log", "md", "nfo", "sh", "bak", "dat", "keys"];

/// Display names for the systems people actually have (ES-DE's full names, shortened).
fn system_name(id: &str) -> Option<&'static str> {
    Some(match id {
        "3do" => "3DO",
        "arcade" | "mame" | "fbneo" => "Arcade",
        "atari2600" => "Atari 2600",
        "atarilynx" => "Atari Lynx",
        "atarijaguar" => "Atari Jaguar",
        "dreamcast" => "Dreamcast",
        "gamecube" | "gc" => "GameCube",
        "gb" => "Game Boy",
        "gba" => "Game Boy Advance",
        "gbc" => "Game Boy Color",
        "gamegear" => "Game Gear",
        "genesis" | "megadrive" => "Genesis",
        "mastersystem" => "Master System",
        "megacd" | "segacd" => "Sega CD",
        "model2" => "Sega Model 2",
        "model3" => "Sega Model 3",
        "n3ds" => "Nintendo 3DS",
        "n64" => "Nintendo 64",
        "nds" => "Nintendo DS",
        "nes" | "famicom" => "NES",
        "ngp" | "ngpc" => "Neo Geo Pocket",
        "neogeo" => "Neo Geo",
        "pcengine" | "tg16" => "TurboGrafx-16",
        "ps2" => "PlayStation 2",
        "ps3" => "PlayStation 3",
        "ps4" => "PlayStation 4",
        "psp" => "PSP",
        "psvita" => "PS Vita",
        "psx" => "PlayStation",
        "saturn" => "Saturn",
        "scummvm" => "ScummVM",
        "snes" | "sfc" => "SNES",
        "switch" => "Switch",
        "virtualboy" => "Virtual Boy",
        "wii" => "Wii",
        "wiiu" => "Wii U",
        "xbox" => "Xbox",
        "xbox360" => "Xbox 360",
        "dos" => "DOS",
        "pico8" => "PICO-8",
        _ => return None,
    })
}

/// Value of `<string name="KEY" value="..."/>` in es_settings.xml.
pub fn setting(xml: &str, key: &str) -> Option<String> {
    let needle = format!("name=\"{key}\"");
    let line = xml.lines().find(|l| l.contains(&needle))?;
    let v = line.split("value=\"").nth(1)?.split('"').next()?;
    Some(unescape(v))
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn tag(block: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&format!("</{name}>"))? + start;
    Some(unescape(block[start..end].trim()))
}

/// ES-DE's "20251228T174628" (local time, read as UTC: a minute-level ordering is all we need).
pub fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.len() < 15 || s.as_bytes()[8] != b'T' {
        return None;
    }
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(4, 6)?, n(6, 8)?, n(9, 11)?, n(11, 13)?, n(13, 15)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || y < 1970 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + se) as u64)
}

/// The `<game>` entries of one gamelist.xml. Folder entries are skipped.
pub fn parse_gamelist(xml: &str, system: &str) -> Vec<RomGame> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<game>").or_else(|| rest.find("<game ")) {
        let after = &rest[i..];
        let Some(end) = after.find("</game>") else { break };
        let block = &after[..end];
        let name = tag(block, "name").or_else(|| tag(block, "path").map(|p| p.rsplit('/').next().unwrap_or(&p).to_string())).unwrap_or_default();
        if !name.is_empty() {
            out.push(RomGame {
                name,
                system: system.to_string(),
                last_played: tag(block, "lastplayed").and_then(|t| parse_time(&t)),
                play_count: tag(block, "playcount").and_then(|t| t.parse().ok()).unwrap_or(0),
                playtime: tag(block, "playtime").and_then(|t| t.parse().ok()).unwrap_or(0),
                path: tag(block, "path"),
            });
        }
        rest = &after[end + 7..];
    }
    out
}

/// `<fullname>` per `<name>` in a custom es_systems.xml.
pub fn custom_names(xml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for block in xml.split("<system>").skip(1) {
        if let (Some(n), Some(f)) = (tag(block, "name"), tag(block, "fullname")) {
            out.insert(n, f);
        }
    }
    out
}

/// Game files under a system folder, a few levels deep (multi-disc and Xbox games sit in folders).
fn count_roms(dir: &Path, depth: usize) -> usize {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') || name == "media" || name == "systeminfo.txt" || name == "metadata.txt" {
            continue;
        }
        if p.is_dir() {
            if depth > 0 {
                // A folder of one game counts once (ES-DE shows it as one entry).
                n += count_roms(&p, depth - 1).min(1);
            }
            continue;
        }
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !ext.is_empty() && !NOT_ROMS.contains(&ext.as_str()) && !name.ends_with(".m3u.txt") {
            n += 1;
        }
    }
    n
}

/// ES-DE's data folders in a home: the AppImage's ~/ES-DE, the old ~/.emulationstation, Flatpak.
fn esde_dirs(home: &Path) -> Vec<PathBuf> {
    ["ES-DE", ".emulationstation", ".var/app/org.es_de.frontend/ES-DE"]
        .iter()
        .map(|d| home.join(d))
        .filter(|d| d.join("settings/es_settings.xml").is_file() || d.join("es_settings.xml").is_file() || d.join("gamelists").is_dir())
        .collect()
}

/// The ROM folder: the ES-DE setting when it exists, else the usual EmuDeck and ES-DE places.
/// A setting from another user's home (a copied config) is remapped into this home.
fn rom_dir(home: &Path, configured: Option<String>) -> Option<PathBuf> {
    let mut tries = Vec::new();
    if let Some(c) = configured.filter(|c| !c.is_empty()) {
        let c = c.replace("%HOMEPATH%", &home.to_string_lossy()).replace("%ESPATH%", &home.join("ES-DE").to_string_lossy());
        let c = c.strip_prefix("~/").map(|r| home.join(r)).unwrap_or_else(|| PathBuf::from(&c));
        tries.push(c.clone());
        // /home/<user>/X or /var/home/<user>/X from a copied config: try X in this home.
        let s = c.to_string_lossy().to_string();
        for base in ["/var/home/", "/home/"] {
            if let Some(rest) = s.strip_prefix(base) {
                if let Some((_, inner)) = rest.split_once('/') {
                    tries.push(home.join(inner));
                }
            }
        }
    }
    tries.push(home.join("Emulation/roms"));
    tries.push(home.join("ROMs"));
    tries.into_iter().find(|p| p.is_dir())
}

/// Scans ES-DE's systems for one home. None when ES-DE was never set up there.
pub fn scan_in(home: &Path) -> Option<Emulation> {
    let dir = esde_dirs(home).into_iter().next()?;
    let settings = fs::read_to_string(dir.join("settings/es_settings.xml")).or_else(|_| fs::read_to_string(dir.join("es_settings.xml"))).unwrap_or_default();
    let roms = rom_dir(home, setting(&settings, "ROMDirectory"));
    let custom = fs::read_to_string(dir.join("custom_systems/es_systems.xml")).map(|t| custom_names(&t)).unwrap_or_default();

    let mut played: BTreeMap<String, Vec<RomGame>> = BTreeMap::new();
    if let Ok(rd) = fs::read_dir(dir.join("gamelists")) {
        for e in rd.flatten() {
            let sys = e.file_name().to_string_lossy().into_owned();
            if let Ok(text) = fs::read_to_string(e.path().join("gamelist.xml")) {
                played.insert(sys.clone(), parse_gamelist(&text, &sys));
            }
        }
    }

    let mut systems = Vec::new();
    if let Some(roms) = &roms {
        let mut ids: Vec<String> = fs::read_dir(roms).map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        ids.sort();
        for id in ids {
            if NOT_SYSTEMS.contains(&id.as_str()) {
                continue;
            }
            // Model 2 keeps its zips one level down (roms/model2/roms).
            let count = count_roms(&roms.join(&id), 2);
            if count == 0 {
                continue;
            }
            let last = played.get(&id).and_then(|g| g.iter().filter_map(|g| g.last_played).max());
            let name = system_name(&id).map(String::from).or_else(|| custom.get(&id).cloned()).unwrap_or_else(|| id.to_uppercase());
            systems.push(RomSystem { id, name, count, last_played: last });
        }
    }
    // Most played systems first, then the biggest.
    systems.sort_by(|a, b| b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0)).then(b.count.cmp(&a.count)).then(a.name.cmp(&b.name)));

    // One entry per game name: ES-DE can list the same game twice (a copy in a subfolder).
    let mut recent: Vec<RomGame> = Vec::new();
    for g in played.into_values().flatten().filter(|g| g.last_played.is_some()) {
        let key = g.name.trim_end_matches(".xiso").to_string();
        match recent.iter_mut().find(|r| r.name.trim_end_matches(".xiso") == key && r.system == g.system) {
            Some(r) => {
                r.play_count += g.play_count;
                r.playtime += g.playtime;
                r.last_played = r.last_played.max(g.last_played);
            }
            None => recent.push(RomGame { name: key, ..g }),
        }
    }
    recent.sort_by(|a, b| b.last_played.cmp(&a.last_played));
    recent.truncate(8);

    if systems.is_empty() && recent.is_empty() {
        return None;
    }
    Some(Emulation { rom_dir: roms.map(|r| r.to_string_lossy().into_owned()).unwrap_or_default(), systems, recent })
}

pub fn scan() -> Option<Emulation> {
    scan_in(&dirs::home_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("19700101T000000"), Some(0));
        assert_eq!(parse_time("20251228T174628"), Some(1766943988));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn parses_settings() {
        let x = r#"<?xml version="1.0"?>
<string name="MediaDirectory" value="/home/bazzite/Emulation/tools/downloaded_media" />
<string name="ROMDirectory" value="/home/bazzite/Emulation/roms" />"#;
        assert_eq!(setting(x, "ROMDirectory").as_deref(), Some("/home/bazzite/Emulation/roms"));
        assert_eq!(setting(x, "Nope"), None);
    }
}
